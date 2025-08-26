package app

import (
	"bytes"
	"crypto/sha256"
	"fmt"
	"log/slog"
	"math/big"
	"os"
	"slices"

	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

// State is an abstraction of the Chain State Database. It tracks the balances,
// nonces, codes, and storage states of accounts in the blockchain and provides
// transaction support for modifying these states.
//
// This type is an adapter for the Carmen state database, providing custom top
// level methods for managing instances in the context of the replay tool.
type State struct {
	// TODO: replace with Carmen facade
	db               carmen.StateDB
	blockHashHistory *blockHashHistory
}

// StateParameters is a configuration struct for creating a new State instance.
type StateParameters struct {
	Directory   string
	WithArchive bool
	Schema      carmen.Schema
	Variant     carmen.Variant
}

// NewState creates a new State instance with the given parameters. The
// resulting state database is empty.
//
// Successfully created instances must be closed using the Close method.
func NewState(params StateParameters) (*State, error) {
	dir := params.Directory
	err := os.MkdirAll(dir, 0700)
	if err != nil {
		return nil, fmt.Errorf("failed to create state dir %q; %v", dir, err)
	}

	archive := carmen.NoArchive
	if params.WithArchive {
		archive = carmen.S5Archive
	}

	state, err := carmen.NewState(carmen.Parameters{
		Directory:    dir,
		Variant:      params.Variant,
		Schema:       params.Schema,
		Archive:      archive,
		LiveCache:    20 * 1024 * 1024 * 1024, // 10GB
		ArchiveCache: 20 * 1024 * 1024 * 1024, // 10GB
	})
	if err != nil {
		return nil, fmt.Errorf("failed to create state: %v", err)
	}
	db := carmen.CreateCustomStateDBUsing(state, 0)
	return &State{db: db, blockHashHistory: &blockHashHistory{}}, nil
}

// Close closes the state database and releases any resources associated with it.
// After calling Close, the State instance should not be used anymore.
// If the state database was already closed, this method has no effect.
func (s *State) Close() error {
	if s.db == nil {
		return nil
	}
	err := s.db.Close()
	s.db = nil
	return err
}

// GetStateRoot returns the current state root hash of the state database.
func (s *State) GetStateRoot() common.Hash {
	return common.Hash(s.db.GetHash())
}

// ApplyGenesis applies the genesis data from the specified file on this state.
func (s *State) ApplyGenesis(genesis *Genesis) error {
	// apply the genesis accounts to the state
	s.db.BeginBlock()
	s.db.BeginTransaction()
	slices.SortFunc(genesis.Accounts, func(a, b Account) int {
		return bytes.Compare(a.Address[:], b.Address[:])
	})

	for _, account := range genesis.Accounts {
		address := account.Address
		fmt.Printf("Creating account %v\n", address)
		fmt.Printf("  Balance: %v\n", account.Balance.ToBig())
		s.db.AddBalance(cc.Address(address), amount.NewFromUint256(&account.Balance))
		if len(account.Code) != 0 {
			fmt.Printf("  Code: %x\n", sha256.Sum256(account.Code))
			s.db.SetCode(cc.Address(address), account.Code)
		}
		if account.Nonce != 0 {
			fmt.Printf("  Nonce: %v\n", account.Nonce)
			s.db.SetNonce(cc.Address(address), account.Nonce)
		}
		for key, value := range account.Storage {
			fmt.Printf("     %x => %x\n", key, value)
			s.db.SetState(cc.Address(address), cc.Key(key), cc.Value(value))
		}
	}
	s.db.EndTransaction()
	s.db.EndBlock(0)
	return s.db.Check()
}

// ApplyBlock applies the given block to this state, processing all transactions
// and updating the state accordingly. It returns the receipts of the transactions
// in the block, or an error if the block could not be processed.
func (s *State) ApplyBlock(
	chainId uint64,
	block *types.Block,
	corrections Corrections,
) (types.Receipts, error) {

	// TODO: use Tosca's processor interface instead of Geth's processor.

	// ERROR[08-26|09:29:26.180] Failed to run block-db                   error="state root mismatch after applying block 10517: expected 655abc99c80bc267099ec8dc15e1f1f4d9ecd97a8bf3a58b08a806efdb5c9fe2, got 3c1ee9cc5158794f8d56fc44e490383b3b6a47d19e24493a085b873582f05e45"
	// ERROR[08-26|17:58:23.808] Failed to run block-db                   error="receipt cumulative gas used mismatch for block 30661, tx 11: expected 421079, got 422440"
	// ERROR[08-26|18:48:46.969] Failed to run block-db                   error="receipt cumulative gas used mismatch for block 45517, tx 0: expected 52748, got 51387"
	// ERROR[08-26|19:28:38.396] Failed to run block-db                   error="receipt cumulative gas used mismatch for block 50862, tx 584: expected 13600051, got 13679258"

	allegro := opera.GetAllegroUpgrades()
	allegroSingleProposer := allegro
	allegroSingleProposer.SingleProposerBlockFormation = true

	/*
	   Upgrade at block 10516: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	   Upgrade at block 16847: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:true}
	   Upgrade at block 45516: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	   Upgrade at block 49188: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:true}
	   Upgrade at block 51557: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	   Upgrade at block 61155: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:true}
	   Upgrade at block 61594: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	   Upgrade at block 63079: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:true}
	   Upgrade at block 63373: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	   Upgrade at block 90409: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:true}
	   Upgrade at block 106860: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	   Upgrade at block 161032: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:true}
	   Upgrade at block 161899: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	   Upgrade at block 251425: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:true}
	   Upgrade at block 253298: {Berlin:true London:true Llr:false Sonic:true Allegro:true Brio:false SingleProposerBlockFormation:false}
	*/

	upgradeHeights := []opera.UpgradeHeight{
		{Upgrades: allegro, Height: 10517},
		{Upgrades: allegroSingleProposer, Height: 16848},
		{Upgrades: allegro, Height: 45517},
		{Upgrades: allegroSingleProposer, Height: 49189},
		{Upgrades: allegro, Height: 51558},
		{Upgrades: allegroSingleProposer, Height: 61156},
		{Upgrades: allegro, Height: 61595},
		{Upgrades: allegroSingleProposer, Height: 63080},
		{Upgrades: allegro, Height: 63374},
		{Upgrades: allegroSingleProposer, Height: 90410},
		{Upgrades: allegro, Height: 106861},
		{Upgrades: allegroSingleProposer, Height: 161033},
		{Upgrades: allegro, Height: 161900},
		{Upgrades: allegroSingleProposer, Height: 251426},
		{Upgrades: allegro, Height: 253299},
	}

	rules := opera.Rules{}
	for _, upgrade := range upgradeHeights {
		if upgrade.Height <= idx.Block(block.NumberU64()) {
			rules.Upgrades = upgrade.Upgrades
		}
	}

	chainConfig := opera.CreateTransientEvmChainConfig(
		chainId,
		upgradeHeights,
		idx.Block(block.NumberU64()),
	)

	processor := evmcore.NewStateProcessor(
		chainConfig,
		historyAdapter{history: s.blockHashHistory},
	)

	evmBlock := &evmcore.EvmBlock{
		EvmHeader: evmcore.EvmHeader{
			Number:      block.Number(),
			ParentHash:  block.ParentHash(),
			Time:        inter.Timestamp(block.Time() * 1e9),
			GasLimit:    block.GasLimit(),
			PrevRandao:  block.Header().MixDigest,
			BaseFee:     block.BaseFee(),
			BlobBaseFee: big.NewInt(1),
		},
		Transactions: block.Transactions(),
	}

	stateDb := evmstore.CreateCarmenStateDb(s.db)

	vmConfig := opera.GetVmConfig(rules)
	//fmt.Printf("Applying block %d with VM config: %t\n", block.NumberU64(), vmConfig.ChargeExcessGas)
	gasLimit := block.GasLimit()

	s.blockHashHistory.SetBlockHash(block.NumberU64()-1, block.ParentHash())

	s.db.BeginBlock()
	var usedGas uint64
	receipts, _, skipped := processor.Process(
		evmBlock,
		stateDb,
		vmConfig,
		gasLimit,
		&usedGas,
		nil,
	)

	if len(skipped) > 0 {
		return nil, fmt.Errorf("found block with skipped txs: %d", len(skipped))
	}

	// Apply corrections if any are provided.
	if fixes := corrections[block.NumberU64()]; len(fixes) > 0 {
		s.db.BeginTransaction()
		slog.Info("Applying corrections", "block", block.NumberU64())
		for addr, acc := range fixes {
			slog.Info("Correcting account",
				"address", addr.Hex(),
				"old_balance", s.db.GetBalance(cc.Address(addr)).ToBig().String(),
				"new_balance", acc.Balance.ToBig().String(),
			)
			s.setBalance(addr, acc.Balance.ToBig())
		}
		s.db.EndTransaction()
	}

	s.db.EndBlock(block.NumberU64())
	return receipts, s.db.Check()
}

func (s *State) setBalance(address common.Address, balance *big.Int) {
	addr := cc.Address(address)
	cur := s.db.GetBalance(addr).ToBig()
	switch cur.Cmp(balance) {
	case -1:
		diff, _ := amount.NewFromBigInt(new(big.Int).Sub(balance, cur))
		s.db.AddBalance(addr, diff)
	case 1:
		diff, _ := amount.NewFromBigInt(new(big.Int).Sub(cur, balance))
		s.db.SubBalance(addr, diff)
	}
}

// --- block hash history tracking ---

// blockHashHistory keeps track of the last 256 block hashes. This is required
// for the BLOCKHASH opcode in the EVM.
type blockHashHistory struct {
	historicHashes [256]common.Hash
}

func (b *blockHashHistory) GetBlockHash(number uint64) common.Hash {
	return b.historicHashes[number%256]
}

func (b *blockHashHistory) SetBlockHash(number uint64, hash common.Hash) {
	b.historicHashes[number%256] = hash
}

// --- block hash history adapter ---

// historyAdapter implements the evmcore.DummyChain interface, allowing it to
// be used with the EVM state processor to serve historic block hashes.
type historyAdapter struct {
	history *blockHashHistory
}

func (h historyAdapter) GetHeader(_ common.Hash, number uint64) *evmcore.EvmHeader {
	// The only information required from the header is the block number, the
	// block's hash, and the parent hash. Everything else is ignored by the EVM.
	return &evmcore.EvmHeader{
		Number:     big.NewInt(int64(number)),
		Hash:       h.history.GetBlockHash(number),
		ParentHash: h.history.GetBlockHash(number - 1),
	}
}
