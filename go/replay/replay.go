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

// Package replay allows to replay the block history.
package replay

import (
	"context"
	"errors"
	"fmt"
	"io"
	"iter"
	"log/slog"
	"math"
	"math/big"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"

	// _ "[github.com/ianlancetaylor/cgosymbolizer](http://github.com/ianlancetaylor/cgosymbolizer)" // Enable to resolve symbols across cgo calls (this breaks Go symbols)

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	_ "github.com/0xsoniclabs/carmen/go/state/gostate"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/drivercall"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/driverpos"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/0xsoniclabs/tracy"
	"github.com/Fantom-foundation/lachesis-base/inter/idx"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/params"
	"github.com/ethereum/go-ethereum/trie"
)

//go:generate mockgen -source=replay.go -destination=replay_mock.go -package=replay

type ReplayArgs struct {
	JSONGenesisFile         string
	BlockDBDir              string
	StateDBDir              string
	InitDBDir               string
	KeepDB                  bool
	WithArchive             bool
	ArchiveRate             float64
	DBSchema                carmen.Schema
	DBVariant               carmen.Variant
	UsePipeline             bool
	Interpreter             string
	StartBlock              uint64
	EndBlock                uint64
	SnapshotInterval        uint64
	SnapshotStartBlock      uint64
	SnapshotEndBlock        uint64
	SnapshotNumToKeep       uint64
	WriteRulesUpdateHeights bool
	OverwriteStateRoot      bool
	NoStateRootCheck        bool
	NoReceiptsCheck         bool
	LogDBSize               bool
	ConfirmAllPrompts       bool
}

func Replay(ctx context.Context, args ReplayArgs) (err error) {
	endBlock := fmt.Sprintf("%d", args.EndBlock)
	if args.EndBlock == math.MaxUint64 {
		endBlock = "max"
	}
	slog.Info("Replay configuration",
		"interpreter", args.Interpreter,
		"db_schema", args.DBSchema,
		"db_variant", args.DBVariant,
		"pipeline", args.UsePipeline,
		"start_block", args.StartBlock,
		"end_block", endBlock,
	)

	// Open the block database
	blockDb, blockDbCleanup, err := openBlockDb(&args)
	if err != nil {
		return fmt.Errorf("failed to open block database: %w", err)
	}
	defer func() { err = errors.Join(err, blockDbCleanup()) }()

	// Load genesis data
	slog.Info("Loading genesis file", "file", args.JSONGenesisFile)
	genesis, err := ReadGenesisFromFile(args.JSONGenesisFile)
	if err != nil {
		return fmt.Errorf("failed to read genesis file %q: %w", args.JSONGenesisFile, err)
	}
	chainID := genesis.ChainID

	// Create the metadata store
	slog.Info("Initializing metadata store")
	metadataStore, err := NewBlockDBMetadataStore(blockDb, genesis.Rules, slog.Default(), args.WriteRulesUpdateHeights)
	if err != nil {
		return fmt.Errorf("failed to create metadata store for chain ID %d: %w", chainID, err)
	}

	// Create the interpreter
	interpreter, err := tosca.NewInterpreter(args.Interpreter)
	if err != nil {
		return fmt.Errorf("failed to create interpreter %q: %w", args.Interpreter, err)
	}

	// Prepare the state database directory
	snapshotHandler, err := prepareStateDbDir(&args)
	if err != nil {
		return fmt.Errorf("failed to prepare state database directory: %w", err)
	}
	if snapshotHandler == nil {
		return // aborted by user input
	}
	if !args.KeepDB {
		slog.Warn("State database will be deleted after replay (use --keep-db to keep it)")
		defer func() { err = errors.Join(err, cleanupStateDbDir(&args, snapshotHandler, err)) }()
	}

	// Open the state database
	slog.Info("Opening state database", "directory", args.StateDBDir)
	state, err := NewState(StateParameters{
		Directory:   args.StateDBDir,
		WithArchive: args.WithArchive,
		Schema:      args.DBSchema,
		Variant:     args.DBVariant,
	})
	if err != nil {
		return fmt.Errorf("failed to open state database: %w", err)
	}
	chain := &stateChainAdapter{
		chainID:          chainID,
		metadataStore:    metadataStore,
		blockHashHistory: &blockHashHistory{},
		interpreter:      interpreter,
		state:            state,
		schema:           args.DBSchema,
		snapshotHandler:  snapshotHandler,
	}
	defer func() {
		slog.Info("Closing state database", "directory", args.StateDBDir)
		// The state needs to be accessed through the stateChainAdapter
		// because it might have been closed and reopened for a snapshot creation.
		// The stateChainAdapter always holds the currently open state.
		if chain.state != nil {
			err = errors.Join(err, chain.state.Close())
		}
	}()

	// Prepare the state
	if err := prepareState(&args, chain, genesis); err != nil {
		return fmt.Errorf("failed to prepare state: %w", err)
	}

	// Create the progress logger
	if args.LogDBSize {
		slog.Warn("DB size log enabled. This will trigger a flush with every progress report and reduce performance")
	}
	progressLogger := startProgressLogger(slog.Default(), chain, args.StateDBDir, args.LogDBSize)
	onBlockDone := func(block *types.Block) error { return progressLogger.LogProgress(block) }
	defer func() { progressLogger.LogSummary() }()

	blocks := blockDb.GetRange(chainID, args.StartBlock, args.EndBlock)

	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot: New(args.OverwriteStateRoot, args.ConfirmAllPrompts),
		skipStateRootCheck: args.NoStateRootCheck,
		stateRootNotSet:    false,
		skipReceiptsCheck:  args.NoReceiptsCheck,
	}

	// Create the archive verifier if archive mode is enabled.
	var verifier *archiveVerifier
	if args.WithArchive {
		var cancel context.CancelCauseFunc
		ctx, cancel = context.WithCancelCause(ctx)
		defer cancel(nil)
		verifier, err = newArchiveVerifier(ctx, cancel, chain, metadataStore, interpreter, chainID, args.ArchiveRate)
		if err != nil {
			return fmt.Errorf("failed to create archive verifier: %w", err)
		}
		if verifier != nil {
			defer func() { err = errors.Join(err, verifier.close()) }()
		}
	}

	// Pick the replay method.
	run := runReplayLoop
	if args.UsePipeline {
		run = runReplayPipeline
	}

	return run(ctx, blocks, chain, blockDb, replayLoopContext, onBlockDone, verifier)
}

func openBlockDb(args *ReplayArgs) (blockdb.BlockDB, func() error, error) {
	slog.Info("Opening block database", "directory", args.BlockDBDir)
	if args.WriteRulesUpdateHeights {
		slog.Info("Rules update heights writing enabled")
	}
	if args.OverwriteStateRoot {
		slog.Info("State root overwriting enabled")
	}
	var dbOpener func(string) (blockdb.RocksDB, error)
	if args.OverwriteStateRoot || args.WriteRulesUpdateHeights {
		dbOpener = blockdb.OpenRocksDBForWriting
	} else {
		dbOpener = blockdb.OpenRocksDBForReading
	}
	blockDb, err := dbOpener(args.BlockDBDir)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to open database: %w", err)
	}
	cleanup := func() error {
		return blockDb.Close()
	}
	return blockDb, cleanup, nil
}

func prepareStateDbDir(args *ReplayArgs) (*SnapshotHandler, error) {
	if args.SnapshotInterval > 0 && args.SnapshotNumToKeep == 0 {
		return nil, fmt.Errorf("snapshot interval %d is incompatible with --snapshot-num-to-keep=0", args.SnapshotInterval)
	}

	// If the state database directory is not specified, create a temporary directory.
	var err error
	if args.StateDBDir == "" {
		if args.StartBlock > 0 && args.InitDBDir == "" {
			return nil, fmt.Errorf("existing state or initial database directory must be specified when starting from a non-genesis block")
		}
		args.StateDBDir = os.TempDir()
		args.StateDBDir, err = os.MkdirTemp(args.StateDBDir, "replay_chain_state_")
		if err != nil {
			return nil, fmt.Errorf("failed to create temporary state database directory: %w", err)
		}
	}
	slog.Info("Creating state database", "directory", args.StateDBDir)
	if args.InitDBDir != "" {
		slog.Info("Copying initial state database", "src", args.InitDBDir, "dest", args.StateDBDir)
		if isEmpty, err := utils.IsEmptyOrMissingDir(args.StateDBDir); err != nil {
			return nil, fmt.Errorf("failed to check if state database directory %q is empty: %w", args.StateDBDir, err)
		} else if !isEmpty {
			return nil, fmt.Errorf("state database directory %q is not empty. Please specify an empty directory to be initialized or use a temporary directory", args.StateDBDir)
		}
		err = os.CopyFS(args.StateDBDir, os.DirFS(args.InitDBDir))
		if err != nil {
			return nil, fmt.Errorf("failed to copy initial state database %q in destination directory %q: %w", args.InitDBDir, args.StateDBDir, err)
		}
	}

	snapshotHandler := NewSnapshotHandler(args.SnapshotInterval, args.SnapshotStartBlock, args.SnapshotEndBlock, args.SnapshotNumToKeep)

	if args.SnapshotInterval > 0 {
		matches, err := filepath.Glob(args.StateDBDir + "_snapshot_*")
		if err != nil {
			return nil, fmt.Errorf("failed to check existing snapshots in state database directory %q: %w", args.StateDBDir, err)
		}
		if len(matches) > 0 {
			slog.Warn("Existing snapshots found for state database directory", "directory", args.StateDBDir, "snapshots_found", len(matches))
			if !args.ConfirmAllPrompts {
				fmt.Printf("Do you want to delete the existing snapshots and continue (y/n)? ")
				var response string
				if _, err := fmt.Scanln(&response); err != nil {
					return nil, fmt.Errorf("failed to read user input: %w", err)
				}
				if strings.ToLower(strings.TrimSpace(response)) != "y" {
					slog.Error("Execution aborted by the user")
					return nil, nil
				}
			}
		}

		end := fmt.Sprintf("%d", args.SnapshotEndBlock)
		if args.SnapshotEndBlock == math.MaxUint64 {
			end = "max"
		}
		slog.Info("Intermediate state database snapshots enabled", "interval_blocks", args.SnapshotInterval, "start_block", args.SnapshotStartBlock, "end_block", end, "snapshots_to_keep", args.SnapshotNumToKeep)
		if strings.Contains(string(args.DBVariant), "flat") {
			slog.Warn("Snapshots are currently not supported with flat database variants; consider disabling snapshots or using a different variant")
		}
	}
	return snapshotHandler, nil
}

func cleanupStateDbDir(args *ReplayArgs, snapshotHandler *SnapshotHandler, outerError error) error {
	slog.Info("Removing state database directory", "directory", args.StateDBDir)
	err := os.RemoveAll(args.StateDBDir)
	snapshotDirs := snapshotHandler.GetSnapshotDirs(args.StateDBDir)
	if len(snapshotDirs) > 0 {
		if outerError == nil || errors.Is(outerError, context.Canceled) {
			for _, dir := range snapshotDirs {
				slog.Info("Removing state database snapshot directory", "directory", dir)
				err = errors.Join(err, os.RemoveAll(dir))
			}
		} else {
			slog.Info("Replay terminated with error")
			for _, dir := range snapshotDirs {
				slog.Info("Available snapshot", "directory", dir)
			}
		}
	}
	return err
}

func prepareState(args *ReplayArgs, chain *stateChainAdapter, genesis *Genesis) error {
	if args.StartBlock == 0 {
		slog.Info("Applying genesis")
		// Apply genesis data to the state database.
		if err := chain.state.ApplyGenesis(genesis); err != nil {
			return fmt.Errorf("failed to apply genesis data: %w", err)
		}
	}
	stateRoot, err := chain.state.GetStateRoot().Await().Get()
	if err != nil {
		return fmt.Errorf("failed to get state root: %w", err)
	}
	slog.Info("Loaded state", "root_hash", stateRoot)
	return nil
}

// runReplayLoop processes the blocks from the given iterator, applying them
// to the chain and checking the results against the expected values in the
// blocks. This is the main business logic of the replay command.
func runReplayLoop(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	chain Chain,
	blockDB blockdb.BlockDB,
	replayLoopContext ReplayLoopContext,
	onBlockDone func(block *types.Block) error,
	verifier *archiveVerifier,
) error {
	for block, err := range blocks {
		tracy.FrameMark()
		if err != nil {
			return fmt.Errorf("failed to get next block: %w", err)
		}
		if ctx.Err() != nil {
			return ctx.Err()
		}

		gethBlock, err := convert.ConvertToGethBlock(block)
		if err != nil {
			return fmt.Errorf("failed to convert block %d: %w", block.Number, err)
		}

		var hashOfParentBlock common.Hash
		if gethBlock.NumberU64() > 0 {
			hashOfParentBlock = chain.GetBlockHash(gethBlock.NumberU64() - 1)
		}

		// If the archive verifier is enabled, capture the current state of the block hash history.
		var hashHistoryClone *blockHashHistory
		if verifier != nil {
			hashHistoryClone = chain.CloneBlockHashHistory()
		}

		// Run the transactions in the block against the state database.
		receipts, stateRoot, err := chain.ApplyBlock(gethBlock)
		if err != nil {
			return fmt.Errorf("failed to apply block %d: %w", block.Number, err)
		}

		// Create a snapshot if one is due at this block.
		stateRoot, err = chain.MaybeSnapshot(gethBlock.NumberU64(), stateRoot)
		if err != nil {
			return fmt.Errorf("failed to create snapshot at block %d: %w", block.Number, err)
		}

		// Check the receipts against the expected values in the block.
		if err := checkBlockResults(chain, block, receipts, stateRoot, hashOfParentBlock, blockDB, &replayLoopContext, slog.Default()); err != nil {
			return fmt.Errorf("failed to check block results for block %d: %w", block.Number, err)
		}

		// Report the progress of the replay.
		if onBlockDone != nil {
			if err := onBlockDone(gethBlock); err != nil {
				return fmt.Errorf("failed to execute onBlockDone callback for block %d: %w", block.Number, err)
			}
		}

		// Submit block for archive verification if enabled.
		if verifier != nil {
			verifier.submit(block, hashHistoryClone)
		}
	}
	return nil
}

// runReplayPipeline processes the blocks from the given iterator using a
// multi-stage pipeline, applying them to the chain and checking the results
// against the expected values in the blocks. This is an optimized version of
// the replay logic that can achieve higher throughput.
func runReplayPipeline(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	chain Chain,
	blockDB blockdb.BlockDB,
	replayLoopContext ReplayLoopContext,
	onBlockDone func(block *types.Block) error,
	verifier *archiveVerifier,
) error {
	const channelSize = 128

	// Pipeline stages:
	//  - decoding of blocks
	//  - applying blocks to the chain
	//  - checking results

	// Result of first stage: decoded block
	type decodedBlock struct {
		proto *blockdb.Block
		geth  *types.Block
	}

	// Result of second stage: applied block results
	type processResult struct {
		decoded           *decodedBlock
		receipts          types.Receipts
		stateRoot         future.Future[result.Result[common.Hash]]
		hashOfParentBlock common.Hash
		// This needs to be deep-copied, it is just a pointer to avoid copying
		// the whole struct every time it is passed through the channels.
		hashHistory *blockHashHistory
	}

	// Channels between stages
	decodedBlocks := make(chan *decodedBlock, channelSize)
	results := make(chan *processResult, channelSize)
	abort := make(chan struct{})

	// Utility to collect errors.
	issue := atomic.Pointer[error]{}
	var abortOnce sync.Once
	reportIssue := func(err error) {
		issue.Store(&err)
		abortOnce.Do(func() { close(abort) })
	}

	// WaitGroup to ensure all goroutines finish before returning.
	// This is critical to avoid closing the database while an iterator
	// is still active in Stage 1.
	var wg sync.WaitGroup

	// Stage 1: Decode blocks
	wg.Go(func() {
		defer close(decodedBlocks)
		signer := types.LatestSignerForChainID(new(big.Int).SetUint64(chain.ChainID()))
		for block, err := range blocks {
			tracy.FrameMark()
			if err != nil {
				reportIssue(fmt.Errorf("failed to get next block: %w", err))
				return
			}
			if ctx.Err() != nil {
				reportIssue(ctx.Err())
				return
			}
			gethBlock, err := convert.ConvertToGethBlock(block)
			if err != nil {
				reportIssue(fmt.Errorf("failed to convert block %d: %w", block.Number, err))
				return
			}

			// Prefetch transaction signatures to speed up processing.
			for _, tx := range gethBlock.Transactions() {
				_, _ = types.Sender(signer, tx) // just pre-fetching
			}

			decoded := &decodedBlock{
				proto: block,
				geth:  gethBlock,
			}

			select {
			case decodedBlocks <- decoded:
				continue
			case <-abort:
				return
			}
		}
	})

	// Stage 2: Apply blocks
	wg.Go(func() {
		defer close(results)
		for decoded := range decodedBlocks {
			gethBlock := decoded.geth
			var hashOfParentBlock common.Hash
			if gethBlock.NumberU64() > 0 {
				hashOfParentBlock = chain.GetBlockHash(gethBlock.NumberU64() - 1)
			}
			// If the archive verifier is enabled, capture the current state of the block hash history.
			var hashHistoryClone *blockHashHistory
			if verifier != nil {
				hashHistoryClone = chain.CloneBlockHashHistory()
			}
			receipts, stateRootFuture, err := chain.ApplyBlock(gethBlock)
			if err != nil {
				reportIssue(fmt.Errorf("failed to apply block %d: %w", gethBlock.NumberU64(), err))
				return
			}
			// Create a snapshot if one is due at this block.
			stateRootFuture, err = chain.MaybeSnapshot(gethBlock.NumberU64(), stateRootFuture)
			if err != nil {
				reportIssue(fmt.Errorf("failed to create snapshot at block %d: %w", gethBlock.NumberU64(), err))
				return
			}
			result := &processResult{
				decoded:           decoded,
				receipts:          receipts,
				stateRoot:         stateRootFuture,
				hashOfParentBlock: hashOfParentBlock,
				hashHistory:       hashHistoryClone,
			}
			select {
			case results <- result:
				continue
			case <-abort:
				return
			}
		}
	})

	// Stage 3: Check results
	wg.Go(func() {
		for result := range results {
			block := result.decoded.proto

			err := checkBlockResults(chain, block, result.receipts, result.stateRoot, result.hashOfParentBlock, blockDB, &replayLoopContext, slog.Default())
			if err != nil {
				reportIssue(fmt.Errorf("failed to check block results for block %d: %w", block.Number, err))
				return
			}

			// Report the progress of the replay.
			if onBlockDone != nil {
				if err := onBlockDone(result.decoded.geth); err != nil {
					reportIssue(fmt.Errorf("failed to execute onBlockDone callback for block %d: %w", block.Number, err))
					return
				}
			}

			// Submit block for archive verification if enabled.
			if verifier != nil {
				verifier.submit(block, result.hashHistory)
			}
		}
	})

	wg.Wait()

	err := issue.Load()
	if err == nil {
		return nil
	}
	return *err
}

// Chain is an interface for an evolving block chain.
type Chain interface {
	ChainID() uint64
	IsMptConformant() bool
	IsVerkleConformant() bool
	ApplyBlock(*types.Block) (
		types.Receipts,
		future.Future[result.Result[common.Hash]],
		error,
	)
	// MaybeSnapshot creates a state database snapshot if one is due at the
	// given block number. It must be called after ApplyBlock and receives the
	// state-root future returned by ApplyBlock so it can be resolved before
	// the state pointer is swapped.
	MaybeSnapshot(
		blockNumber uint64,
		stateRoot future.Future[result.Result[common.Hash]],
	) (future.Future[result.Result[common.Hash]], error)
	GetBlockHash(blockNumber uint64) common.Hash
	CloneBlockHashHistory() *blockHashHistory
}

// ArchiveState exposes the subset of state operations required by the archive
// verifier. Implementations must be safe to call concurrently with block
// application on the same underlying state.
type ArchiveState interface {
	GetArchiveBlockHeight() (height uint64, empty bool, err error)
	ApplyArchiveBlock(
		block *types.Block,
		interpreter tosca.Interpreter,
		processor Processor,
		upgrades opera.Upgrades,
		corrections map[common.Address]Correction,
		chainConfig *params.ChainConfig,
	) (types.Receipts, error)
}

// StateFlusher exposes the state-flush operation required by the progress
// logger. Implementations must be safe to call concurrently with block
// application on the same underlying state.
type StateFlusher interface {
	FlushState() error
}

// stateChainAdapter is an adapter that allows the State to be used as a Chain.
//
// Concurrency: two mutexes coordinate access to the adapter.
//
//   - stateSwitchMutex guards accesses to the underlying *State pointer.
//     RLock is held during operations that use the current state (block
//     application via ApplyBlock, archive queries via GetArchiveBlockHeight
//     and ApplyArchiveBlock, flushes via FlushState). Lock is held only for
//     the snapshot pointer swap in MaybeSnapshot, which closes and reopens
//     the state database.
//
//   - stateModifyMutex serializes live block application. It ensures that at
//     most one goroutine executes ApplyBlock at a time and protects
//     mutations to adapter fields performed during block application
//     (notably blockHashHistory writes).
//
// Together they allow live block application and concurrent archive
// verification / flushes to proceed in parallel while ensuring the state
// pointer is not swapped while any operation is using it. Lock ordering:
// callers acquiring both locks must take stateSwitchMutex before
// stateModifyMutex.
type stateChainAdapter struct {
	chainID          uint64
	metadataStore    MetadataStore
	blockHashHistory *blockHashHistory
	interpreter      tosca.Interpreter
	state            *State
	stateSwitchMutex sync.RWMutex
	stateModifyMutex sync.Mutex
	schema           carmen.Schema
	snapshotHandler  *SnapshotHandler
}

func (a *stateChainAdapter) ChainID() uint64 {
	return a.chainID
}

func (a *stateChainAdapter) IsMptConformant() bool {
	return a.schema == 5
}

func (a *stateChainAdapter) IsVerkleConformant() bool {
	return a.schema == 6
}

func (a *stateChainAdapter) GetBlockHash(blockNumber uint64) common.Hash {
	a.stateSwitchMutex.RLock()
	defer a.stateSwitchMutex.RUnlock()
	a.stateModifyMutex.Lock()
	defer a.stateModifyMutex.Unlock()
	return a.blockHashHistory.GetBlockHash(blockNumber)
}

func (a *stateChainAdapter) CloneBlockHashHistory() *blockHashHistory {
	a.stateSwitchMutex.RLock()
	defer a.stateSwitchMutex.RUnlock()
	a.stateModifyMutex.Lock()
	defer a.stateModifyMutex.Unlock()
	return a.blockHashHistory.Clone()
}

// ApplyBlock applies the given block to the state database and returns the
// resulting receipts and state-root future.
func (a *stateChainAdapter) ApplyBlock(block *types.Block) (
	types.Receipts,
	future.Future[result.Result[common.Hash]],
	error,
) {
	a.stateSwitchMutex.RLock()
	defer a.stateSwitchMutex.RUnlock()
	a.stateModifyMutex.Lock()
	defer a.stateModifyMutex.Unlock()

	zoneBlock := tracy.ZoneBegin("ProcessBlock")
	defer zoneBlock.End()

	// Block 0 is skipped since it is equivalent with the genesis data
	// import. The archive does not accept two blocks with the same number.
	if block.NumberU64() == 0 {
		return nil, a.state.GetStateRoot(), nil
	}

	// Commit all pending rules updates if the block is an epoch sealing block.
	if block.Transactions().Len() > 0 {
		_, err := drivercall.ParseSealEpochArgs(block.Transactions()[0])
		isEpochSealingTx := err == nil
		if isEpochSealingTx {
			if err := a.metadataStore.CommitRules(block.NumberU64()); err != nil {
				return nil, future.Future[result.Result[common.Hash]]{}, fmt.Errorf("failed to commit rules at block %d: %v", block.NumberU64(), err)
			}
		}
	}

	chainConfig, upgrades := getChainConfigAndUpgrades(block, a.chainID, a.metadataStore)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		a.blockHashHistory,
		upgrades,
	)

	corrections := a.metadataStore.GetCorrectionsAtBlock(block.NumberU64())

	onLog := func(l *core_types.Log) { onNewLog(a.metadataStore, block.NumberU64(), l) }

	// Apply the block to the state database.
	receipts, err := a.state.ApplyBlock(block, a.interpreter, processor, upgrades, corrections, chainConfig, onLog, false)
	if err != nil {
		stateRoot := future.Future[result.Result[common.Hash]]{}
		return nil, stateRoot, fmt.Errorf("failed to apply block %d: %w", block.NumberU64(), err)
	}

	// Reconstruct the complete header by filling in fields that are derived
	// from receipts, which are not stored in the block DB when importing
	// from era files.
	completeHeader := block.Header()
	if len(receipts) > 0 {
		completeHeader.GasUsed = receipts[len(receipts)-1].CumulativeGasUsed
	} else {
		completeHeader.GasUsed = 0
	}
	completeHeader.Bloom = types.MergeBloom(receipts)
	completeHeader.ReceiptHash = types.DeriveSha(receipts, trie.NewStackTrie(nil))
	*block = *block.WithSeal(completeHeader)

	a.blockHashHistory.SetBlockHash(block.NumberU64(), completeHeader.Hash())

	return receipts, a.state.GetStateRoot(), nil
}

// MaybeSnapshot creates a state database snapshot if one is due at the given
// block number. When a snapshot is triggered, the stateRoot future is awaited
// and returned as an immediate future so it remains valid after the underlying
// state database has been closed and reopened. When no snapshot is due, the
// stateRoot future is returned unchanged.
//
// The snapshot swap runs under an exclusive lock on stateSwitchMutex, so it
// will block until all in-flight operations holding stateSwitchMutex.RLock()
// complete. The stateRoot future must be resolved before the state pointer is
// swapped so it remains valid after the old state DB is closed and reopened.
func (a *stateChainAdapter) MaybeSnapshot(
	blockNumber uint64,
	stateRoot future.Future[result.Result[common.Hash]],
) (future.Future[result.Result[common.Hash]], error) {
	if !a.snapshotHandler.ShouldCreateSnapshot(blockNumber) {
		return stateRoot, nil
	}
	// Resolve the state-root future before swapping the state pointer so it
	// remains valid after the underlying state has been closed and reopened.
	stateRoot = future.Immediate(stateRoot.Await())

	a.stateSwitchMutex.Lock()
	defer a.stateSwitchMutex.Unlock()
	newState, err := a.snapshotHandler.Snapshot(blockNumber, a.state)
	if err != nil {
		return stateRoot, fmt.Errorf("failed to create snapshot at block %d: %w", blockNumber, err)
	}
	a.state = newState
	return stateRoot, nil
}

// FlushState flushes pending writes in the state database to disk.
// It is safe to call concurrently with block application (ApplyBlock) and
// other archive-side operations.
func (a *stateChainAdapter) FlushState() error {
	a.stateSwitchMutex.RLock()
	defer a.stateSwitchMutex.RUnlock()
	return a.state.db.Flush()
}

// GetArchiveBlockHeight returns the highest block currently available in the
// archive. It is safe to call concurrently with block application.
func (a *stateChainAdapter) GetArchiveBlockHeight() (height uint64, empty bool, err error) {
	a.stateSwitchMutex.RLock()
	defer a.stateSwitchMutex.RUnlock()
	return a.state.GetArchiveBlockHeight()
}

// ApplyArchiveBlock re-executes the given block against the archive state for
// its parent block and returns the resulting receipts. The live state is not
// modified. It is safe to call concurrently with block application.
func (a *stateChainAdapter) ApplyArchiveBlock(
	block *types.Block,
	interpreter tosca.Interpreter,
	processor Processor,
	upgrades opera.Upgrades,
	corrections map[common.Address]Correction,
	chainConfig *params.ChainConfig,
) (types.Receipts, error) {
	a.stateSwitchMutex.RLock()
	defer a.stateSwitchMutex.RUnlock()
	return a.state.ApplyBlock(block, interpreter, processor, upgrades, corrections, chainConfig, nil, true)
}

func getChainConfigAndUpgrades(block *types.Block, chainID uint64, metadata MetadataStore) (*params.ChainConfig, opera.Upgrades) {
	if cfg := ethereumChainConfigMap[chainID]; cfg != nil {
		rules := cfg.Rules(block.Number(), false, block.Time())
		return cfg, opera.Upgrades{
			Berlin: rules.IsBerlin,
			London: rules.IsLondon,
			Llr:    false,

			Sonic:   rules.IsCancun,
			Allegro: rules.IsPrague,
			Brio:    rules.IsOsaka,

			SingleProposerBlockFormation: false,
			GasSubsidies:                 false,
			TransactionBundles:           false,
		}
	}
	chainConfig := opera.CreateTransientEvmChainConfig(
		chainID,
		metadata.GetUpgradeHeights(),
		idx.Block(block.NumberU64()),
	)
	return chainConfig, metadata.GetUpgradesAtBlock(block.NumberU64())
}

// getExpectedStateRoot returns the expected state root for the given block, based on the chain type.
func getExpectedStateRoot(chain Chain, block *blockdb.Block) common.Hash {
	var stateRoot []byte
	if chain.IsMptConformant() {
		stateRoot = block.StateRoot
	} else if chain.IsVerkleConformant() {
		stateRoot = block.VerkleStateRoot
	}
	return common.BytesToHash(stateRoot)
}

// updateStateRoot updates the state root in the block based on the chain type.
func updateStateRoot(chain Chain, block *blockdb.Block, stateRoot common.Hash) {
	if chain.IsMptConformant() {
		block.StateRoot = stateRoot.Bytes()
	} else if chain.IsVerkleConformant() {
		block.VerkleStateRoot = stateRoot.Bytes()
	}
}

// ReplayLoopContext is a utility struct to hold flags to pass to the `replayLoop` functions.
type ReplayLoopContext struct {
	overwriteStateRoot FlagWithConfirmation
	skipStateRootCheck bool
	stateRootNotSet    bool

	skipReceiptsCheck bool
}

// FlagWithConfirmation is a utility struct to hold a boolean flag along with a confirmation flag to track user confirmation.
type FlagWithConfirmation struct {
	flag      bool
	confirmed bool
}

func New(flag bool, confirmAll bool) FlagWithConfirmation {
	return FlagWithConfirmation{
		flag:      flag,
		confirmed: confirmAll,
	}
}

func (f *FlagWithConfirmation) IsEnabled() bool {
	return f.flag
}

func (f *FlagWithConfirmation) Disable() {
	f.flag = false
}

func (f *FlagWithConfirmation) IsConfirmed() bool {
	return f.confirmed
}

func (f *FlagWithConfirmation) Confirm() {
	f.confirmed = true
}

// --- block hash history tracking ---

// blockHashHistory keeps track of the last 256 block hashes. This is required
// for the BLOCKHASH opcode in the EVM.
// It implements the evmcore.DummyChain interface, allowing it to
// be used with the EVM state processor to serve historic block hashes.
type blockHashHistory struct {
	historicHashes [256]common.Hash
}

func (h *blockHashHistory) Clone() *blockHashHistory {
	if h == nil {
		return nil
	}
	clone := *h
	return &clone
}

func (h *blockHashHistory) GetBlockHash(number uint64) common.Hash {
	return h.historicHashes[number%256]
}

func (h *blockHashHistory) SetBlockHash(number uint64, hash common.Hash) {
	h.historicHashes[number%256] = hash
}

func (h *blockHashHistory) Header(_ common.Hash, number uint64) *evmcore.EvmHeader {
	// The only information required from the header is the block number, the
	// block's hash, and the parent hash. Everything else is ignored by the EVM.
	return &evmcore.EvmHeader{
		Number:     big.NewInt(int64(number)),
		Hash:       h.GetBlockHash(number),
		ParentHash: h.GetBlockHash(number - 1),
	}
}

// --- rules update handling ---

func onNewLog(metadataStore MetadataStore, blockNumber uint64, l *core_types.Log) {
	// https://github.com/0xsoniclabs/sonic/blob/c3816115c9ae51682aa475c715aabbe10e0dcef4/gossip/blockproc/drivermodule/driver_txs.go#L351
	if l.Address == driver.ContractAddress &&
		len(l.Topics) > 0 &&
		l.Topics[0] == driverpos.Topics.UpdateNetworkRules &&
		len(l.Data) >= 64 {
		diff, err := decodeDataBytes(l)
		if err != nil {
			slog.Warn("Failed to decode UpdateNetworkRules event data", "block", blockNumber, "error", err)
			return
		}
		err = metadataStore.PatchRules(blockNumber, diff)
		if err != nil {
			slog.Warn("Failed to patch rules", "block", blockNumber, "error", err)
		}
	}
}

// https://github.com/0xsoniclabs/sonic/blob/c3816115c9ae51682aa475c715aabbe10e0dcef4/gossip/blockproc/drivermodule/driver_txs.go#L296
func decodeDataBytes(l *core_types.Log) ([]byte, error) {
	if len(l.Data) < 32 {
		return nil, io.ErrUnexpectedEOF
	}
	start := new(big.Int).SetBytes(l.Data[24:32]).Uint64()
	if start+32 > uint64(len(l.Data)) {
		return nil, io.ErrUnexpectedEOF
	}
	size := new(big.Int).SetBytes(l.Data[start+24 : start+32]).Uint64()
	if start+32+size > uint64(len(l.Data)) {
		return nil, io.ErrUnexpectedEOF
	}
	return l.Data[start+32 : start+32+size], nil
}
