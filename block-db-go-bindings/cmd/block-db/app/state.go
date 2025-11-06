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
	"github.com/0xsoniclabs/tracy"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus/ethash"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
	"github.com/holiman/uint256"
	// Uncomment to enable experimental Carmen features.
	//_ "github.com/0xsoniclabs/carmen/go/experimental"
)

/*
func init() {
	lfvm.RegisterExperimentalInterpreterConfigurations()
}
*/

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

	// TODO: take this from the metadata;
	chainConfig = params.SepoliaChainConfig

	rules := metadata.GetRulesAtBlock(block.NumberU64())

	processor := evmcore.NewStateProcessor(
		chainConfig,
		historyAdapter{history: s.blockHashHistory},
		rules.Upgrades,
	)

	prevRandao := block.Header().MixDigest
	if chainConfig.MergeNetsplitBlock.Cmp(block.Number()) > 0 {
		// Before the Merge, PrevRandao is not used; set to zero. This indicates
		// to the EVM that the difficulty should be used instead.
		prevRandao = common.Hash{}
	}

	evmBlock := &evmcore.EvmBlock{
		EvmHeader: evmcore.EvmHeader{
			Number:      block.Number(),
			ParentHash:  block.ParentHash(),
			Time:        inter.Timestamp(block.Time() * 1e9),
			GasLimit:    block.GasLimit(),
			PrevRandao:  prevRandao,
			BaseFee:     block.BaseFee(),
			BlobBaseFee: big.NewInt(1),
			Coinbase:    block.Coinbase(),
		},
		Transactions: block.Transactions(),
	}

	stateDb := evmstore.CreateCarmenStateDb(s.db)

	vmConfig := opera.GetVmConfig(rules)

	/*
		if block.NumberU64() == 1 || block.NumberU64() == 1178593 {

			// For transaction processing, Tosca's LFVM is used.
			factory := tosca.GetInterpreterFactory("lfvm-logging")
			if factory == nil {
				panic("LFVM logging interpreter factory not found")
			}
			interpreter, err := factory(nil)
			if err != nil {
				return nil, fmt.Errorf("failed to create LFVM interpreter: %w", err)
			}
			lfvmFactory := geth_adapter.NewGethInterpreterFactory(interpreter)

			vmConfig.Interpreter = lfvmFactory
		}
	*/

	// TODO: take these from the metadata; they should be enabled for Sonic
	// chains, but disabled for Ethereum chains like Sepolia.
	vmConfig.ChargeExcessGas = false
	vmConfig.IgnoreGasFeeCap = false
	vmConfig.InsufficientBalanceIsNotAnError = false
	vmConfig.SkipTipPaymentToCoinbase = false

	gasLimit := block.GasLimit()

	s.blockHashHistory.SetBlockHash(block.NumberU64()-1, block.ParentHash())

	zone := tracy.ZoneBegin("TransactionProcessing")
	s.db.BeginBlock()
	var usedGas uint64
	processed := processor.ProcessWithDifficulty(
		evmBlock,
		stateDb,
		vmConfig,
		gasLimit,
		&usedGas,
		nil,
		block.Difficulty(),
	)

	// Check that all transactions were processed (i.e., none were skipped).
	// for i, processed := range processed {
	// 	if processed.Receipt == nil {
	// 		return nil, fmt.Errorf("found block with skipped txs at index %d", i)
	// 	}
	// }

	// Retrieve the receipts from the processed transactions.
	receipts := types.Receipts{}
	for _, proc := range processed {
		if proc.Receipt != nil {
			receipts = append(receipts, proc.Receipt)
		}
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
	zone.End()

	// Apply block rewards.
	// Derived from https://github.com/0xsoniclabs/go-ethereum/blob/949ae6d396a5798262c0d228a8de0e3fa504e00c/consensus/beacon/consensus.go#L329-L342
	if block.Number().Uint64() < 1_450_409 { // < for Sepolia, triggered by total difficulty limit (TODO: move to metadata)
		accumulateRewards(chainConfig, stateDb, block.Header(), block.Uncles())
	} else {
		// Withdrawals processing.
		for _, w := range block.Withdrawals() {
			// Convert amount from gwei to wei.
			amount := new(uint256.Int).SetUint64(w.Amount)
			amount = amount.Mul(amount, uint256.NewInt(params.GWei))
			stateDb.AddBalance(w.Address, amount, tracing.BalanceIncreaseWithdrawal)
		}
		// No block reward which is issued by consensus layer instead.
	}

	zone = tracy.ZoneBegin("CommitBlock")
	s.db.EndBlock(block.NumberU64())
	zone.End()
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

// accumulateRewards credits the coinbase of the given block with the mining
// reward. The total reward consists of the static block reward and rewards for
// included uncles. The coinbase of each uncle block is also rewarded.
// Copied from
// https://github.com/0xsoniclabs/go-ethereum/blob/949ae6d396a5798262c0d228a8de0e3fa504e00c/consensus/ethash/consensus.go#L570
func accumulateRewards(config *params.ChainConfig, stateDB addBalancer, header *types.Header, uncles []*types.Header) {
	// Select the correct block reward based on chain progression
	blockReward := ethash.FrontierBlockReward
	if config.IsByzantium(header.Number) {
		blockReward = ethash.ByzantiumBlockReward
	}
	if config.IsConstantinople(header.Number) {
		blockReward = ethash.ConstantinopleBlockReward
	}
	/*
		if header.Number.Uint64() >= 1450409 { // < for Sepolia (TODO: move to metadata)
			blockReward = uint256.NewInt(0)
		}
	*/
	// Accumulate the rewards for the miner and any included uncles
	reward := new(uint256.Int).Set(blockReward)
	r := new(uint256.Int)
	hNum, _ := uint256.FromBig(header.Number)
	for _, uncle := range uncles {
		uNum, _ := uint256.FromBig(uncle.Number)
		r.AddUint64(uNum, 8)
		r.Sub(r, hNum)
		r.Mul(r, blockReward)
		r.Rsh(r, 3)
		stateDB.AddBalance(uncle.Coinbase, r, tracing.BalanceIncreaseRewardMineUncle)

		r.Rsh(blockReward, 5)
		reward.Add(reward, r)
	}
	stateDB.AddBalance(header.Coinbase, reward, tracing.BalanceIncreaseRewardMineBlock)
}

type addBalancer interface {
	AddBalance(addr common.Address, amount *uint256.Int, reason tracing.BalanceChangeReason) uint256.Int
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
