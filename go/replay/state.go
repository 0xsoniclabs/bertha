// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

package replay

import (
	"context"
	"fmt"
	"log/slog"
	"math"
	"math/big"
	"os"
	"slices"
	"strings"

	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/amount"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
	"github.com/0xsoniclabs/sonic/inter/state"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/tosca/go/geth_adapter"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus"
	"github.com/ethereum/go-ethereum/consensus/beacon"
	"github.com/ethereum/go-ethereum/consensus/ethash"
	"github.com/ethereum/go-ethereum/core"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/params"
	// Uncomment to enable experimental Carmen features.
	//_ "github.com/0xsoniclabs/carmen/go/experimental"
)

//go:generate mockgen -source=state.go -destination=state_mock.go -package=replay

// State is an abstraction of the Chain State Database. It tracks the balances,
// nonces, codes, and storage states of accounts in the blockchain and provides
// transaction support for modifying these states.
//
// This type is an adapter for the Carmen state database, providing custom top
// level methods for managing instances in the context of the replay tool.
type State struct {
	// TODO: replace with Carmen facade
	db             carmen.StateDB
	stateParameter StateParameters
}

// StateParameters is a configuration struct for creating a new State instance.
type StateParameters struct {
	Directory   string
	WithArchive bool
	Schema      carmen.Schema
	Variant     carmen.Variant
}

type Processor interface {
	ProcessWithDifficulty(
		block *evmcore.EvmBlock,
		statedb state.StateDB,
		cfg vm.Config,
		gasLimit uint64,
		usedGas *uint64,
		trueTxOffset int,
		onNewLog func(*core_types.Log),
		difficulty *big.Int,
		remainingSize uint64,
	) evmcore.ProcessSummary
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
		if strings.HasPrefix(string(params.Variant), "rust") {
			archive = "file"
		} else if strings.HasPrefix(string(params.Variant), "go-geth2") {
			archive = carmen.LevelDbArchive
		} else {
			archive = carmen.S5Archive
		}
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
	return &State{
		db:             db,
		stateParameter: params,
	}, nil
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

// GetArchiveBlockHeight returns the highest block currently available in the
// archive. If the archive is empty, empty is true.
func (s *State) GetArchiveBlockHeight() (height uint64, empty bool, err error) {
	return s.db.GetArchiveBlockHeight()
}

// GetStateRoot returns the current state root hash of the state database.
func (s *State) GetStateRoot() future.Future[result.Result[common.Hash]] {
	return future.Then(s.db.GetCommitment(), func(res result.Result[cc.Hash]) result.Result[common.Hash] {
		value, err := res.Get()
		if err != nil {
			return result.Err[common.Hash](err)
		}
		return result.Ok(common.Hash(value))
	})
}

// ApplyGenesis applies the accounts of the given genesis on this state. The
// genesis transactions are not executed since they are part of block 1, which
// is replayed like any other block.
func (s *State) ApplyGenesis(genesis *makefakegenesis.GenesisJson) error {
	// apply the genesis accounts to the state
	s.db.BeginBlock()
	s.db.BeginTransaction()
	for _, account := range genesis.Accounts {
		address := account.Address
		var balance amount.Amount
		if account.Balance != nil {
			balance = amount.NewFromUint256(account.Balance)
		}
		s.db.AddBalance(cc.Address(address), balance)
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
	if _, err := endBlockAndCommit(s, 0); err != nil {
		return err
	}
	return s.db.Check()
}

// ApplyBlock applies the given block to this state, processing all transactions
// and updating the state accordingly. The envelopes of failed bundles are
// processed at their positions between the block's transactions. It returns the
// receipts of the transactions in the block and the gas used by all processed
// transactions, or an error if the block could not be processed.
//
// When isArchive is true, the block is re-executed against the archive state
// for the parent block. In this mode, BeginBlock/EndBlock are not called and
// the state is not committed.
func (s *State) ApplyBlock(
	block *types.Block,
	interpreter tosca.Interpreter,
	processor Processor,
	rules opera.Rules,
	corrections map[common.Address]Correction,
	failedBundles []FailedBundle,
	chainConfig *params.ChainConfig,
	onLog func(*core_types.Log),
	isArchive bool,
) (types.Receipts, uint64, error) {
	transactions := block.Transactions()
	if len(failedBundles) > 0 {
		var err error
		transactions, err = insertFailedBundles(transactions, failedBundles)
		if err != nil {
			return nil, 0, fmt.Errorf("failed to insert failed bundles into block %d: %w", block.NumberU64(), err)
		}
	}

	var stateDB *evmstore.CarmenStateDB
	var vmStateDB carmen.VmStateDB

	if isArchive {
		if block.NumberU64() == 0 {
			return nil, 0, fmt.Errorf("cannot apply genesis block in archive mode")
		}
		archiveBlock := block.NumberU64() - 1
		archiveDB, err := s.db.GetArchiveStateDB(archiveBlock)
		if err != nil {
			return nil, 0, fmt.Errorf("failed to get archive state for block %d: %w", archiveBlock, err)
		}
		defer archiveDB.Release()
		stateDB = evmstore.CreateNonCommittableCarmenStateDb(archiveDB, nil)
		vmStateDB = archiveDB
	} else {
		stateDB = evmstore.CreateCarmenStateDb(s.db, nil)
		vmStateDB = s.db
		s.db.BeginBlock()
	}

	zone := tracy.ZoneBegin("TransactionProcessing")
	defer zone.End()

	evmBlock, err := convert.ToEvmBlock(block, transactions, chainConfig, isEthereum(chainConfig.ChainID.Uint64()))
	if err != nil {
		return nil, 0, err
	}

	var vmConfig vm.Config
	if !isEthereum(chainConfig.ChainID.Uint64()) {
		// Apply Sonic-specific VM settings that are not applicable to Ethereum chains.
		vmConfig = opera.GetVmConfig(rules)
	}
	vmConfig.Interpreter = geth_adapter.NewGethInterpreterFactory(interpreter)

	// EVM for the system calls; the chain is not needed for the current system calls.
	evm := vm.NewEVM(evmcore.NewEVMBlockContextWithDifficulty(&evmBlock.EvmHeader, nil, nil, big.NewInt(0)), stateDB, chainConfig, vmConfig)
	defer evm.Release()

	if isEthereum(chainConfig.ChainID.Uint64()) && chainConfig.IsCancun(block.Number(), block.Time()) {
		// EIP-4788: store the parent beacon block root in the beacon roots contract.
		if beaconRoot := block.BeaconRoot(); beaconRoot != nil {
			core.ProcessBeaconBlockRoot(*beaconRoot, evm, nil)
			stateDB.EndTransaction()
		}
	}

	var usedGas uint64
	processed := processor.ProcessWithDifficulty(
		evmBlock,
		stateDB,
		vmConfig,
		block.GasLimit(),
		&usedGas,
		0, // Tx index offset
		onLog,
		block.Difficulty(),
		math.MaxUint64,
	)

	// Retrieve the receipts from the processed transactions and check that none
	// of them were skipped and that none of the failed bundles added receipts.
	// Transactions of failed bundles are not part of the block, and a skipped
	// bundle is returned as its envelope.
	receipts := make(types.Receipts, 0, len(processed.ProcessedTransactions))
	isFailedBundle := func(hash common.Hash) bool {
		return slices.ContainsFunc(failedBundles, func(b FailedBundle) bool { return b.Tx.Hash() == hash })
	}
	for _, proc := range processed.ProcessedTransactions {
		hash := proc.Transaction.Hash()
		if proc.Receipt == nil {
			if isFailedBundle(hash) {
				return nil, 0, fmt.Errorf("failed bundle %v was skipped", hash)
			}
			return nil, 0, fmt.Errorf("tx %v was skipped", hash)
		}
		if cause := processed.CausedBy[hash]; isFailedBundle(cause) {
			return nil, 0, fmt.Errorf("failed bundle %v added a receipt for tx %v", cause, hash)
		}
		receipts = append(receipts, proc.Receipt)
	}

	if isEthereum(chainConfig.ChainID.Uint64()) {
		// Credit the withdrawals and call the request contracts as system calls.
		// The logs are only needed to collect the requests, which replay does not need.
		_, _, err := core.PostExecution(context.Background(), chainConfig, block.Number(), block.Time(), nil, block.Withdrawals(), evm, uint32(len(block.Transactions())+1))
		stateDB.EndTransaction()
		if err != nil {
			return nil, 0, fmt.Errorf("failed to process post-execution of block %d: %w", block.NumberU64(), err)
		}
	}

	// Apply corrections if any are provided.
	applyCorrections(vmStateDB, corrections, block, slog.Default())

	if isEthereum(chainConfig.ChainID.Uint64()) {
		// Credit the block and uncle rewards pre-merge.
		beacon.New(ethash.NewFaker()).Finalize(chainConfigReader{config: chainConfig}, block.Header(), stateDB, block.Body())
	}

	if !isArchive {
		endBlockZone := tracy.ZoneBegin("EndBlock")
		// Discard the handle as we don't need to wait for the commit to finish here.
		_, err := endBlockAndCommit(s, block.NumberU64())
		endBlockZone.End()
		if err != nil {
			return nil, 0, err
		}
	}

	return receipts, usedGas, vmStateDB.Check()
}

// insertFailedBundles inserts the envelopes of the given failed bundles, sorted
// by position, before the transaction at their position.
func insertFailedBundles(txs types.Transactions, failedBundles []FailedBundle) (types.Transactions, error) {
	res := make(types.Transactions, 0, len(txs)+len(failedBundles))
	next := uint64(0)
	for _, failedBundle := range failedBundles {
		if failedBundle.Pos > uint64(len(txs)) {
			return nil, fmt.Errorf("position %d of failed bundle exceeds the number of transactions %d", failedBundle.Pos, len(txs))
		}
		res = append(res, txs[next:failedBundle.Pos]...)
		res = append(res, failedBundle.Tx)
		next = failedBundle.Pos
	}
	return append(res, txs[next:]...), nil
}

func applyCorrections(stateDB carmen.VmStateDB, corrections map[common.Address]Correction, block *types.Block, logger utils.Logger) {
	if len(corrections) == 0 {
		return
	}
	stateDB.BeginTransaction()
	logger.Info("Applying corrections", "block", block.NumberU64())
	for addr, acc := range corrections {
		if acc.Balance != nil {
			logger.Info("Correcting balance",
				"address", addr.Hex(),
				"old", stateDB.GetBalance(cc.Address(addr)).ToBig().String(),
				"new", acc.Balance.ToBig().String(),
			)
			setBalance(stateDB, addr, acc.Balance.ToBig())
		}
		if len(acc.Storage) > 0 {
			logger.Info("Correcting storage",
				"address", addr.Hex(),
				"storage_keys", len(acc.Storage),
			)
			for key, value := range acc.Storage {
				logger.Info("Correcting storage entry",
					"address", addr.Hex(),
					"key", fmt.Sprintf("0x%x", key[:]),
					"old", fmt.Sprintf("0x%x", stateDB.GetState(cc.Address(addr), key)),
					"new", fmt.Sprintf("0x%x", value[:]),
				)
				stateDB.SetState(cc.Address(addr), key, value)
			}
		}
	}
	stateDB.EndTransaction()

}

func setBalance(stateDB carmen.VmStateDB, address common.Address, balance *big.Int) {
	addr := cc.Address(address)
	cur := stateDB.GetBalance(addr).ToBig()
	switch cur.Cmp(balance) {
	case -1:
		diff, _ := amount.NewFromBigInt(new(big.Int).Sub(balance, cur))
		stateDB.AddBalance(addr, diff)
	case 1:
		diff, _ := amount.NewFromBigInt(new(big.Int).Sub(cur, balance))
		stateDB.SubBalance(addr, diff)
	}
}

// chainConfigReader provides the Config method of consensus.ChainHeaderReader;
// its other methods panic.
type chainConfigReader struct {
	consensus.ChainHeaderReader
	config *params.ChainConfig
}

func (r chainConfigReader) Config() *params.ChainConfig {
	return r.config
}

// endBlockAndCommit calls `EndBlock` on the state database and commits
// the staged block to the archive.
// It returns the handle to the archive commit operation, or an error if the operation failed.
func endBlockAndCommit(s *State, blockNum uint64) (*carmen.WaitHandle, error) {
	staged, err := s.db.EndBlock(blockNum)
	if err != nil {
		return nil, fmt.Errorf("failed to end block %d: %w", blockNum, err)
	}
	if staged == nil {
		return nil, fmt.Errorf("failed to end block %d: staged state is nil", blockNum)
	}
	handle, err := staged.Commit()
	if err != nil {
		return nil, fmt.Errorf("failed to commit block %d: %w", blockNum, err)
	}
	return handle, nil
}
