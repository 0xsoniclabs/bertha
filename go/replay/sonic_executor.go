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
	"fmt"
	"log"
	"log/slog"

	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/gossip"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/evmmodule"
	"github.com/0xsoniclabs/sonic/gossip/evmstore"
	"github.com/0xsoniclabs/sonic/inter"
	"github.com/0xsoniclabs/sonic/inter/state"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/drivercall"
	"github.com/0xsoniclabs/tosca/go/geth_adapter"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/Fantom-foundation/lachesis-base/hash"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	gethlog "github.com/ethereum/go-ethereum/log"
)

// sonicExecutor is the Sonic implementation of BlockExecutor. It executes
// blocks with Sonic's block processing (gossip.Ledger) on the live state and
// with State.ApplyBlock on the archive.
type sonicExecutor struct {
	chainID       uint64
	metadataStore MetadataStore
	interpreter   tosca.Interpreter
	ledger        gossip.Ledger
	store         *ledgerStore
}

// newSonicExecutor returns a sonicExecutor whose ledger's head is the block of
// the given header. The live state it executes blocks on must hold the state
// after that block.
func newSonicExecutor(
	chainID uint64,
	metadataStore MetadataStore,
	interpreter tosca.Interpreter,
	head *types.Header,
) (*sonicExecutor, error) {
	store, err := newLedgerStore(head, metadataStore)
	if err != nil {
		return nil, err
	}

	// NewLedger logs every block with the geth root logger it is created with.
	// Capture the current loggers.
	gethlogDefault := gethlog.Root()
	slogDefault := slog.Default()
	logWriter, logFlags := log.Writer(), log.Flags()
	// Discard geth logging temporarily.
	gethlog.SetDefault(gethlog.NewLogger(gethlog.DiscardHandler()))

	ledger := gossip.NewLedger(store, evmmodule.NewForReplay(geth_adapter.NewGethInterpreterFactory(interpreter)), nil, false)

	// Restore the loggers. gethlog.SetDefault comes first since it also
	// replaces slog's default and log's output.
	gethlog.SetDefault(gethlogDefault)
	slog.SetDefault(slogDefault)
	log.SetOutput(logWriter)
	log.SetFlags(logFlags)

	return &sonicExecutor{
		chainID:       chainID,
		metadataStore: metadataStore,
		interpreter:   interpreter,
		ledger:        ledger,
		store:         store,
	}, nil
}

func (e *sonicExecutor) ExecuteBlock(state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, *types.Header, error) {
	number := block.NumberU64()

	head, err := e.ledger.Head()
	if err != nil {
		return nil, nil, fmt.Errorf("failed to get the ledger's head for block %d: %w", number, err)
	}
	if headNumber := head.Block().Number; number != headNumber+1 {
		return nil, nil, fmt.Errorf("block %d does not follow the ledger's head %d", number, headNumber)
	}

	nanos, _, err := inter.DecodeExtraData(block.Extra())
	if err != nil {
		return nil, nil, fmt.Errorf("failed to decode extra data of block %d: %w", number, err)
	}

	failedBundles := e.metadataStore.GetFailedBundlesAtBlock(number)
	txs, err := insertFailedBundles(block.Transactions(), failedBundles)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to insert failed bundles into block %d: %w", number, err)
	}

	// Commit all pending rules updates if the block is an epoch sealing block.
	if block.Transactions().Len() > 0 {
		if _, err := drivercall.ParseSealEpochArgs(block.Transactions()[0]); err == nil {
			if err := e.metadataStore.CommitRules(number); err != nil {
				return nil, nil, fmt.Errorf("failed to commit rules at block %d: %w", number, err)
			}
		}
	}
	rules := e.metadataStore.GetRulesAtBlock(number)

	e.store.stateDB = recordedRootStateDB{StateDB: evmstore.CreateCarmenStateDb(state.db, nil), root: block.Root()}
	e.store.hashHistory = hashHistory

	params := gossip.BlockParams{
		Time:                     inter.Timestamp(block.Time()*1e9 + uint64(nanos)),
		PrevRandao:               block.MixDigest(),
		GasLimit:                 block.GasLimit(),
		UserTransactionsGasLimit: block.GasLimit(),
		Rules:                    rules,
	}
	onLog := func(l *core_types.Log) { onNewLog(e.metadataStore, number, l) }

	executing, err := head.BeginBlock(params, onLog)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to begin block %d: %w", number, err)
	}

	summary := executing.Run(txs)
	receipts, err := blockReceipts(summary, failedBundles)
	if err != nil {
		return nil, nil, err
	}
	applyCorrections(state.db, e.metadataStore.GetCorrectionsAtBlock(number), block, slog.Default())

	finalized, err := executing.Finalize()
	if err != nil {
		return nil, nil, fmt.Errorf("failed to finalize block %d: %w", number, err)
	}
	candidate := finalized.Candidate()
	finalized.Commit().Publish()
	return receipts, candidate.Block.GetEthereumHeader(), state.db.Check()
}

func (e *sonicExecutor) ExecuteBlockOnArchive(state *State, block *types.Block, hashHistory *blockHashHistory) (types.Receipts, error) {
	receipts, _, err := e.applyBlock(state, block, hashHistory, nil, true)
	return receipts, err
}

// applyBlock applies the given block with the metadata of the block to state,
// see State.ApplyBlock.
func (e *sonicExecutor) applyBlock(
	state *State,
	block *types.Block,
	hashHistory *blockHashHistory,
	onLog func(*core_types.Log),
	isArchive bool,
) (types.Receipts, uint64, error) {
	number := block.NumberU64()
	chainConfig := opera.CreateTransientEvmChainConfig(e.chainID, e.metadataStore.GetUpgradeHeights(), idx.Block(number))
	rules := e.metadataStore.GetRulesAtBlock(number)

	processor := evmcore.NewStateProcessorForReplay(chainConfig, hashHistory, rules.Upgrades)

	corrections := e.metadataStore.GetCorrectionsAtBlock(number)
	failedBundles := e.metadataStore.GetFailedBundlesAtBlock(number)

	return state.ApplyBlock(
		block, e.interpreter, processor, rules, corrections, failedBundles, chainConfig, onLog, isArchive,
	)
}

// ledgerStore is the store of a sonicExecutor's ledger. It stores nothing on
// disk and keeps only the head block in memory, which the ledger replaces when
// it commits a block. It opens the state of the block being applied and serves
// the hashes of older blocks from the hash history of that block.
//
// The ledger accesses it only from within sonicExecutor.ExecuteBlock, which
// sets the fields of the block being applied.
type ledgerStore struct {
	metadataStore MetadataStore
	head          *inter.Block
	stateDB       state.StateDB
	hashHistory   *blockHashHistory
}

var _ gossip.LedgerStore = (*ledgerStore)(nil)

// newLedgerStore returns a ledgerStore whose head is the block of the given
// header.
func newLedgerStore(header *types.Header, metadataStore MetadataStore) (*ledgerStore, error) {
	nanos, duration, err := inter.DecodeExtraData(header.Extra)
	if err != nil {
		return nil, fmt.Errorf("failed to decode extra data of block %d: %w", header.Number, err)
	}
	head := &inter.Block{}
	head.Number = header.Number.Uint64()
	head.ParentHash = header.ParentHash
	head.StateRoot = header.Root
	head.Time = inter.Timestamp(header.Time*1e9 + uint64(nanos))
	head.Difficulty = header.Difficulty.Uint64()
	head.GasLimit = header.GasLimit
	head.GasUsed = header.GasUsed
	head.BaseFee = header.BaseFee
	head.PrevRandao = header.MixDigest
	head.TransactionsHashRoot = header.TxHash
	head.ReceiptsHashRoot = header.ReceiptHash
	head.LogBloom = header.Bloom
	head.Duration = uint64(duration)
	return &ledgerStore{metadataStore: metadataStore, head: head}, nil
}

func (s *ledgerStore) GetLatestBlockIndex() idx.Block {
	return idx.Block(s.head.Number)
}

func (s *ledgerStore) GetLatestBlock() *inter.Block {
	return s.head
}

func (s *ledgerStore) GetBlock(n idx.Block) *inter.Block {
	if uint64(n) != s.head.Number {
		return nil
	}
	return s.head
}

func (s *ledgerStore) SetBlock(_ idx.Block, block *inter.Block) {
	s.head = block
}

// GetUpgradeHeights returns the upgrades of the block following the head,
// which are the only ones the ledger reads.
func (s *ledgerStore) GetUpgradeHeights() []opera.UpgradeHeight {
	number := s.head.Number + 1
	rules := s.metadataStore.GetRulesAtBlock(number)
	return []opera.UpgradeHeight{{Upgrades: rules.Upgrades, Height: idx.Block(number)}}
}

func (s *ledgerStore) GetLiveStateDb(hash.Hash) (state.StateDB, error) {
	return s.stateDB, nil
}

// Header returns the complete header of the head block and, for older blocks,
// a header with just the hashes the EVM reads.
func (s *ledgerStore) Header(hash common.Hash, number uint64) *evmcore.EvmHeader {
	if number == s.head.Number {
		return evmcore.ToEvmHeader(s.head, s.head.ParentHash, s.metadataStore.GetRulesAtBlock(number))
	}
	return s.hashHistory.Header(hash, number)
}

func (s *ledgerStore) SetBlockIndex(common.Hash, idx.Block)           {}
func (s *ledgerStore) SetTx(common.Hash, *types.Transaction)          {}
func (s *ledgerStore) SetTxPosition(common.Hash, evmstore.TxPosition) {}
func (s *ledgerStore) SetReceipts(idx.Block, types.Receipts)          {}
func (s *ledgerStore) IndexLogs(...*types.Log)                        {}
func (s *ledgerStore) SetCachedEvmBlock(idx.Block, *evmcore.EvmBlock) {}

// recordedRootStateDB reports the recorded state root as its hash, so the
// ledger's blocks keep their recorded hashes without waiting for Carmen's
// root, which checkBlockResults verifies.
type recordedRootStateDB struct {
	state.StateDB
	root common.Hash
}

func (db recordedRootStateDB) GetStateHash() common.Hash {
	return db.root
}
