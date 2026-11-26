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

package ethereum

import (
	"context"
	"fmt"
	"math"
	"math/big"

	"github.com/0xsoniclabs/bertha/replay/state"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/freeze"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	sonicstate "github.com/0xsoniclabs/sonic/inter/state"
	"github.com/0xsoniclabs/tosca/go/geth_adapter"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/consensus"
	"github.com/ethereum/go-ethereum/consensus/beacon"
	"github.com/ethereum/go-ethereum/consensus/ethash"
	"github.com/ethereum/go-ethereum/consensus/misc/eip4844"
	"github.com/ethereum/go-ethereum/core"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/params"
	"github.com/ethereum/go-ethereum/trie"
)

//go:generate mockgen -source=executor.go -destination=executor_mock.go -package=ethereum

// Executor is the Ethereum implementation of the replay's BlockExecutor.
type Executor struct {
	chainID     uint64
	interpreter tosca.Interpreter
}

// NewExecutor returns an Executor for the Ethereum network with the given chain
// ID.
func NewExecutor(chainID uint64, interpreter tosca.Interpreter) *Executor {
	return &Executor{chainID: chainID, interpreter: interpreter}
}

func (e *Executor) ExecuteBlock(state *state.State, block *types.Block, hashHistory evmcore.DummyChain) (types.Receipts, *types.Header, error) {
	receipts, usedGas, err := e.applyBlock(state, block, hashHistory, false)
	if err != nil {
		return nil, nil, err
	}

	// Reconstruct the complete header by filling in fields that are derived
	// from processing the block, which are not stored in the block DB when
	// importing from era files.
	completeHeader := block.Header()
	completeHeader.GasUsed = usedGas
	completeHeader.Bloom = types.MergeBloom(receipts)
	completeHeader.ReceiptHash = types.DeriveSha(receipts, trie.NewStackTrie(nil))
	return receipts, completeHeader, nil
}

func (e *Executor) ExecuteBlockOnArchive(state *state.State, block *types.Block, _ *types.Header, hashHistory evmcore.DummyChain) (types.Receipts, error) {
	receipts, _, err := e.applyBlock(state, block, hashHistory, true)
	return receipts, err
}

// applyBlock applies the given block to state, see applyBlockWithProcessor.
func (e *Executor) applyBlock(
	state *state.State,
	block *types.Block,
	hashHistory evmcore.DummyChain,
	isArchive bool,
) (types.Receipts, uint64, error) {
	chainConfig, upgrades := getChainConfigAndUpgrades(block, e.chainID)

	processor := evmcore.NewStateProcessorForReplay(chainConfig, hashHistory, upgrades)

	return applyBlockWithProcessor(state, block, e.interpreter, processor, chainConfig, isArchive)
}

type Processor interface {
	ProcessWithDifficulty(
		block *evmcore.EvmBlock,
		statedb sonicstate.StateDB,
		freezeChecker freeze.Checker,
		cfg vm.Config,
		gasLimit uint64,
		usedGas *uint64,
		trueTxOffset int,
		onNewLog func(*core_types.Log),
		difficulty *big.Int,
		remainingSize uint64,
	) evmcore.ProcessSummary
}

// applyBlockWithProcessor applies the given block to the state with the given
// processor and returns its receipts and gas used. With isArchive, it runs on
// the archive state of the parent block and commits nothing.
func applyBlockWithProcessor(
	s *state.State,
	block *types.Block,
	interpreter tosca.Interpreter,
	processor Processor,
	chainConfig *params.ChainConfig,
	isArchive bool,
) (types.Receipts, uint64, error) {
	var stateDB *evmstore.CarmenStateDB
	var vmStateDB carmen.VmStateDB

	if isArchive {
		if block.NumberU64() == 0 {
			return nil, 0, fmt.Errorf("cannot apply genesis block in archive mode")
		}
		archiveBlock := block.NumberU64() - 1
		archiveDB, err := s.DB.GetArchiveStateDB(archiveBlock)
		if err != nil {
			return nil, 0, fmt.Errorf("failed to get archive state for block %d: %w", archiveBlock, err)
		}
		defer archiveDB.Release()
		stateDB = evmstore.CreateNonCommittableCarmenStateDb(archiveDB, nil)
		vmStateDB = archiveDB
	} else {
		stateDB = evmstore.CreateCarmenStateDb(s.DB, nil)
		vmStateDB = s.DB
		s.DB.BeginBlock()
	}

	zone := tracy.ZoneBegin("TransactionProcessing")
	defer zone.End()

	evmBlock := toEvmBlock(block, chainConfig)

	vmConfig := vm.Config{Interpreter: geth_adapter.NewGethInterpreterFactory(interpreter)}

	// EVM for the system calls; the chain is not needed for the current system calls.
	evm := vm.NewEVM(evmcore.NewEVMBlockContextWithDifficulty(&evmBlock.EvmHeader, nil, nil, big.NewInt(0)), stateDB, chainConfig, vmConfig)
	defer evm.Release()

	if chainConfig.IsCancun(block.Number(), block.Time()) {
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
		freeze.NewRegistryChecker(false, stateDB),
		vmConfig,
		block.GasLimit(),
		&usedGas,
		0, // Tx index offset
		nil,
		block.Difficulty(),
		math.MaxUint64,
	)

	// Retrieve the receipts from the processed transactions and check that none
	// of them were skipped.
	receipts := make(types.Receipts, 0, len(processed.ProcessedTransactions))
	for _, proc := range processed.ProcessedTransactions {
		if proc.Receipt == nil {
			return nil, 0, fmt.Errorf("tx %v was skipped", proc.Transaction.Hash())
		}
		receipts = append(receipts, proc.Receipt)
	}

	// Credit the withdrawals and call the request contracts as system calls.
	// The logs are only needed to collect the requests, which replay does not need.
	_, _, err := core.PostExecution(
		context.Background(), chainConfig, block.Number(), block.Time(), nil,
		block.Withdrawals(), evm, uint32(len(block.Transactions())+1),
	)
	stateDB.EndTransaction()
	if err != nil {
		return nil, 0, fmt.Errorf("failed to process post-execution of block %d: %w", block.NumberU64(), err)
	}

	// Credit the block and uncle rewards pre-merge.
	beacon.New(ethash.NewFaker()).Finalize(chainConfigReader{config: chainConfig}, block.Header(), stateDB, block.Body())

	if !isArchive {
		endBlockZone := tracy.ZoneBegin("EndBlock")
		// Discard the handle as we don't need to wait for the commit to finish here.
		_, err := state.EndBlockAndCommit(s, block.NumberU64())
		endBlockZone.End()
		if err != nil {
			return nil, 0, err
		}
	}

	return receipts, usedGas, vmStateDB.Check()
}

// toEvmBlock converts the given block of an Ethereum chain to an EVM block that
// runs the block's transactions.
func toEvmBlock(block *types.Block, chainConfig *params.ChainConfig) *evmcore.EvmBlock {
	blobBaseFee := big.NewInt(1)
	if chainConfig.IsCancun(block.Number(), block.Time()) && block.ExcessBlobGas() != nil {
		blobBaseFee = eip4844.CalcBlobFee(chainConfig, block.Header())
	}
	prevRandao := block.MixDigest()
	if block.Difficulty().Sign() != 0 {
		// Before the Merge, PrevRandao is unused and set to zero, which tells the EVM
		// to use the difficulty instead.
		prevRandao = common.Hash{}
	}
	return &evmcore.EvmBlock{
		EvmHeader: evmcore.EvmHeader{
			Number:      block.Number(),
			ParentHash:  block.ParentHash(),
			Time:        inter.Timestamp(block.Time() * 1e9),
			GasLimit:    block.GasLimit(),
			PrevRandao:  prevRandao,
			BaseFee:     block.BaseFee(),
			BlobBaseFee: blobBaseFee,
			Coinbase:    block.Coinbase(),
		},
		Transactions: block.Transactions(),
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
