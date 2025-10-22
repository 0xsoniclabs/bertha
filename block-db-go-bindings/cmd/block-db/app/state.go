package app

import (
	"fmt"
	"log/slog"
	"math/big"
	"os"

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
		LiveCache:    10 * 1024 * 1024 * 1024, // 10GB
		ArchiveCache: 10 * 1024 * 1024 * 1024, // 10GB
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
	for _, account := range genesis.Accounts {
		address := account.Address
		s.db.AddBalance(cc.Address(address), amount.NewFromUint256(&account.Balance))
		if len(account.Code) != 0 {
			s.db.SetCode(cc.Address(address), account.Code)
		}
		if account.Nonce != 0 {
			s.db.SetNonce(cc.Address(address), account.Nonce)
		}
		for key, value := range account.Storage {
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
	metadata Metadata,
) (types.Receipts, error) {

	chainConfig := opera.CreateTransientEvmChainConfig(
		chainId,
		metadata.Upgrades,
		idx.Block(block.NumberU64()),
	)
	rules := metadata.GetRulesAtBlock(block.NumberU64())

	processor := evmcore.NewStateProcessor(
		chainConfig,
		historyAdapter{history: s.blockHashHistory},
		rules.Upgrades,
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
	gasLimit := block.GasLimit()

	s.blockHashHistory.SetBlockHash(block.NumberU64()-1, block.ParentHash())

	s.db.BeginBlock()
	var usedGas uint64
	processed := processor.Process(
		evmBlock,
		stateDb,
		vmConfig,
		gasLimit,
		&usedGas,
		nil,
	)

	// Check that all transactions were processed (i.e., none were skipped).
	for i, processed := range processed {
		if processed.Receipt == nil {
			return nil, fmt.Errorf("found block with skipped txs at index %d", i)
		}
	}

	// Retrieve the receipts from the processed transactions.
	receipts := make(types.Receipts, len(processed))
	for i, proc := range processed {
		receipts[i] = proc.Receipt
	}

	// Apply corrections if any are provided.
	if fixes := metadata.Corrections[block.NumberU64()]; len(fixes) > 0 {
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
