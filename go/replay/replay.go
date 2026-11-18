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
	"github.com/0xsoniclabs/bertha/replay/ethereum"
	"github.com/0xsoniclabs/bertha/replay/sonic"
	"github.com/0xsoniclabs/bertha/replay/state"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	_ "github.com/0xsoniclabs/carmen/go/state/gostate"
	"github.com/0xsoniclabs/sonic/integration/makefakegenesis"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

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
	OverwriteStateRoot      OverwriteStateRootPolicy
	NoStateRootCheck        bool
	NoReceiptsCheck         bool
	LogDBSize               bool
	ConfirmAllPrompts       bool
}

// OverwriteStateRootPolicy controls how the replay reacts to the state root
// computed from executing a block relative to the state root stored in the
// block database.
type OverwriteStateRootPolicy int

const (
	// OverwriteStateRootPolicyOff verifies computed state roots against the
	// stored ones and aborts the replay on any mismatch. Blocks with no stored
	// state root are skipped with a warning (logged at most once per run).
	OverwriteStateRootPolicyOff OverwriteStateRootPolicy = iota

	// OverwriteStateRootPolicyUninitialized writes computed state roots back to
	// the block database only when the block has no stored state root yet.
	// Otherwise it behaves like OverwriteStateRootPolicyOff and aborts on any
	// mismatch.
	OverwriteStateRootPolicyUninitialized

	// OverwriteStateRootPolicyOn unconditionally writes computed state roots
	// back to the block database and skips verification.
	OverwriteStateRootPolicyOn
)

func Replay(ctx context.Context, args ReplayArgs) (err error) {
	// check combinations of flags that are not supported
	if args.StartBlock > args.EndBlock {
		return fmt.Errorf("start block %d is greater than end block %d", args.StartBlock, args.EndBlock)
	}
	if args.StartBlock > 0 && args.InitDBDir == "" && args.StateDBDir == "" {
		return fmt.Errorf("when starting from a non-genesis block (block 0), an existing state database or initial database directory must be specified")
	}
	if !args.WithArchive && args.ArchiveRate > 0 {
		return fmt.Errorf("when using a non-zero archive rate, archive mode must be enabled (pass --with-archive)")
	}
	if args.SnapshotStartBlock > args.SnapshotEndBlock {
		return fmt.Errorf("snapshot start block %d is greater than snapshot end block %d", args.SnapshotStartBlock, args.SnapshotEndBlock)
	}
	if args.SnapshotInterval > 0 && args.SnapshotNumToKeep == 0 {
		return fmt.Errorf("when using a non-zero snapshot interval %d, --snapshot-num-to-keep must be greater than 0", args.SnapshotInterval)
	}

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
	genesis, err := makefakegenesis.LoadGenesisJson(args.JSONGenesisFile)
	if err != nil {
		return fmt.Errorf("failed to read genesis file %q: %w", args.JSONGenesisFile, err)
	}
	chainID := genesis.Rules.NetworkID

	// Create the interpreter
	interpreter, err := tosca.NewInterpreter(args.Interpreter)
	if err != nil {
		return fmt.Errorf("failed to create interpreter %q: %w", args.Interpreter, err)
	}

	// Create the block executor
	firstReplayedBlock := max(args.StartBlock, 1) // block zero is not replayed
	headBlock := firstReplayedBlock - 1           // the live state starts with the head at the previous block
	var blockExecutor BlockExecutor
	var headHeader *types.Header
	if ethereum.IsEthereum(chainID) {
		blockExecutor = ethereum.NewExecutor(chainID, interpreter)
		headHeader, err = getRecordedHeader(blockDb, chainID, headBlock)
		if err != nil {
			return fmt.Errorf("failed to get head header: %w", err)
		}
	} else {
		// Create the metadata store
		slog.Info("Initializing metadata store")
		metadataStore, err := sonic.NewBlockDBMetadataStore(blockDb, genesis.Rules, slog.Default(), args.WriteRulesUpdateHeights)
		if err != nil {
			return fmt.Errorf("failed to create metadata store for chain ID %d: %w", chainID, err)
		}
		headHeader, err = getLedgerHead(blockDb, metadataStore, chainID, headBlock)
		if err != nil {
			return fmt.Errorf("failed to get head header: %w", err)
		}
		blockExecutor, err = sonic.NewExecutor(metadataStore, interpreter, headHeader)
		if err != nil {
			return fmt.Errorf("failed to open ledger: %w", err)
		}
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
	state, err := state.NewState(state.StateParameters{
		Directory:   args.StateDBDir,
		WithArchive: args.WithArchive,
		Schema:      args.DBSchema,
		Variant:     args.DBVariant,
	})
	if err != nil {
		return fmt.Errorf("failed to open state database: %w", err)
	}
	executor := &historyExecutor{
		chainID:          chainID,
		blockExecutor:    blockExecutor,
		blockHashHistory: &blockHashHistory{},
		headHeader:       headHeader,
		state:            state,
		schema:           args.DBSchema,
		snapshotHandler:  snapshotHandler,
	}
	defer func() {
		slog.Info("Closing state database", "directory", args.StateDBDir)
		// The state needs to be accessed through the historyExecutor
		// because it might have been closed and reopened for a snapshot creation.
		// The historyExecutor always holds the currently open state.
		if executor.state != nil {
			err = errors.Join(err, executor.state.Close())
		}
	}()

	// Prepare the state
	if err := prepareState(&args, executor, blockDb, genesis); err != nil {
		return fmt.Errorf("failed to prepare state: %w", err)
	}

	// Create the progress logger
	if args.LogDBSize {
		slog.Warn("DB size log enabled. This will trigger a flush with every progress report and reduce performance")
	}
	progressLogger := startProgressLogger(slog.Default(), executor, args.StateDBDir, args.LogDBSize)
	onBlockDone := func(block *types.Block) error { return progressLogger.LogProgress(block) }
	defer func() { progressLogger.LogSummary() }()

	blocks := blockDb.GetRange(chainID, args.StartBlock, args.EndBlock)

	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot:  args.OverwriteStateRoot,
		skipStateRootCheck:  args.NoStateRootCheck,
		stateRootNotSetSeen: false,
		skipReceiptsCheck:   args.NoReceiptsCheck,
	}

	// Create the archive verifier if archive mode is enabled.
	var verifier *archiveVerifier
	if args.WithArchive {
		var cancel context.CancelCauseFunc
		ctx, cancel = context.WithCancelCause(ctx)
		defer cancel(nil)
		verifier, err = newArchiveVerifier(ctx, cancel, executor, args.ArchiveRate, args.NoReceiptsCheck, slog.Default())
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

	return run(ctx, blocks, executor, blockDb, replayLoopContext, onBlockDone, verifier)
}

func openBlockDb(args *ReplayArgs) (blockdb.BlockDB, func() error, error) {
	slog.Info("Opening block database", "directory", args.BlockDBDir)
	var dbOpener func(string) (blockdb.RocksDB, error)
	if args.OverwriteStateRoot != OverwriteStateRootPolicyOff || args.WriteRulesUpdateHeights {
		slog.Warn("Block database will be opened for writing")
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
	// If the state database directory is not specified, create a temporary directory.
	var err error
	if args.StateDBDir == "" {
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

func prepareState(args *ReplayArgs, executor *historyExecutor, blockDb blockdb.BlockDB, genesis *makefakegenesis.GenesisJson) error {
	if args.StartBlock == 0 {
		slog.Info("Applying genesis")
		// Apply genesis data to the state database.
		if err := executor.state.ApplyGenesis(genesis); err != nil {
			return fmt.Errorf("failed to apply genesis data: %w", err)
		}
	}
	if err := seedBlockHashHistory(blockDb, executor.chainID, args.StartBlock, executor.blockHashHistory); err != nil {
		return fmt.Errorf("failed to seed block hash history: %w", err)
	}
	stateRoot, err := executor.state.GetStateRoot().Await().Get()
	if err != nil {
		return fmt.Errorf("failed to get state root: %w", err)
	}
	slog.Info("Loaded state", "root_hash", stateRoot)
	return nil
}

func seedBlockHashHistory(
	blockDb blockdb.BlockDB,
	chainID uint64,
	firstBlockToApply uint64,
	hashHistory *blockHashHistory,
) error {
	if firstBlockToApply == 0 {
		return nil
	}
	var start uint64 = 1
	if firstBlockToApply > blockHashHistoryLen-1 {
		start = firstBlockToApply - (blockHashHistoryLen - 1)
	}
	end := firstBlockToApply
	count := uint64(0)
	for block, err := range blockDb.GetRange(chainID, start, end) {
		if err != nil {
			return fmt.Errorf("failed to read block for hash history seeding: %w", err)
		}
		if block.Number == 0 {
			continue
		}
		hashHistory.SetBlockHash(block.Number-1, common.BytesToHash(block.ParentHash))
		count++
	}
	expected := end - start + 1
	if count != expected {
		return fmt.Errorf(
			"expected %d blocks in [%d, %d] for chain %d, got %d",
			expected, start, end, chainID, count,
		)
	}
	return nil
}

// getRecordedHeader returns the header of the recorded block of the given
// number.
func getRecordedHeader(blockDb blockdb.BlockDB, chainID, number uint64) (*types.Header, error) {
	recorded, err := blockDb.Get(chainID, number)
	if err != nil {
		return nil, fmt.Errorf("failed to read recorded block %d: %w", number, err)
	}
	if recorded == nil {
		return nil, fmt.Errorf("block %d is not recorded", number)
	}
	block, err := convert.ConvertToGethBlock(recorded)
	if err != nil {
		return nil, fmt.Errorf("failed to convert recorded block %d: %w", number, err)
	}
	return block.Header(), nil
}

// getLedgerHead returns the header of the recorded block of the given number
// with the gas used that gives it the hash the following block records as its
// parent hash. The block DB derives the gas used from the receipts, which miss
// the gas of failed bundles after the last transaction, so the gas used is
// searched up to the gas limit of the block's failed bundles.
func getLedgerHead(blockDb blockdb.BlockDB, metadataStore sonic.MetadataStore, chainID, number uint64) (*types.Header, error) {
	head, err := getRecordedHeader(blockDb, chainID, number)
	if err != nil {
		return nil, err
	}
	next, err := getRecordedHeader(blockDb, chainID, number+1)
	if err != nil {
		return nil, err
	}
	if head.Hash() == next.ParentHash {
		return head, nil
	}
	maxGasUsed := head.GasUsed
	for _, failedBundle := range metadataStore.GetFailedBundlesAtBlock(number) {
		maxGasUsed += failedBundle.Tx.Gas()
	}
	for gasUsed := head.GasUsed + 1; gasUsed <= maxGasUsed; gasUsed++ {
		head.GasUsed = gasUsed
		if head.Hash() == next.ParentHash {
			return head, nil
		}
	}
	return nil, fmt.Errorf("no gas used of block %d up to %d gives the hash %v recorded by block %d", number, maxGasUsed, next.ParentHash, number+1)
}

// ReplayLoopContext is a utility struct to hold flags to pass to the `replayLoop` functions.
type ReplayLoopContext struct {
	overwriteStateRoot  OverwriteStateRootPolicy
	skipStateRootCheck  bool
	stateRootNotSetSeen bool

	skipReceiptsCheck bool
}

// runReplayLoop processes the blocks from the given iterator, applying them
// to the state and checking the results against the expected values in the
// blocks. This is the main business logic of the replay command.
func runReplayLoop(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	executor HistoryExecutor,
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
			hashOfParentBlock = executor.GetBlockHash(gethBlock.NumberU64() - 1)
		}

		// If the archive verifier is enabled, capture the current state of the
		// block hash history and the header of the parent.
		var hashHistoryClone *blockHashHistory
		var parentHeader *types.Header
		if verifier != nil {
			hashHistoryClone = executor.CloneBlockHashHistory()
			parentHeader = executor.GetHeadHeader()
		}

		// Run the transactions in the block against the state database.
		receipts, stateRoot, err := executor.ApplyBlock(gethBlock)
		if err != nil {
			return fmt.Errorf("failed to apply block %d: %w", block.Number, err)
		}

		// Create a snapshot if one is due at this block.
		stateRoot, err = executor.MaybeSnapshot(gethBlock.NumberU64(), stateRoot)
		if err != nil {
			return fmt.Errorf("failed to create snapshot at block %d: %w", block.Number, err)
		}

		// Check the receipts against the expected values in the block.
		if err := checkBlockResults(executor, block, receipts, stateRoot, hashOfParentBlock, blockDB, &replayLoopContext, slog.Default()); err != nil {
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
			verifier.submit(block, parentHeader, hashHistoryClone)
		}
	}
	return nil
}

// runReplayPipeline processes the blocks from the given iterator using a
// multi-stage pipeline, applying them to the state and checking the results
// against the expected values in the blocks. This is an optimized version of
// the replay logic that can achieve higher throughput.
func runReplayPipeline(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	executor HistoryExecutor,
	blockDB blockdb.BlockDB,
	replayLoopContext ReplayLoopContext,
	onBlockDone func(block *types.Block) error,
	verifier *archiveVerifier,
) error {
	const channelSize = 128

	// Pipeline stages:
	//  - decoding of blocks
	//  - applying blocks to the state
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
		parentHeader      *types.Header
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
		signer := types.LatestSignerForChainID(new(big.Int).SetUint64(executor.ChainID()))
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
				hashOfParentBlock = executor.GetBlockHash(gethBlock.NumberU64() - 1)
			}
			// If the archive verifier is enabled, capture the current state of
			// the block hash history and the header of the parent.
			var hashHistoryClone *blockHashHistory
			var parentHeader *types.Header
			if verifier != nil {
				hashHistoryClone = executor.CloneBlockHashHistory()
				parentHeader = executor.GetHeadHeader()
			}
			receipts, stateRootFuture, err := executor.ApplyBlock(gethBlock)
			if err != nil {
				reportIssue(fmt.Errorf("failed to apply block %d: %w", gethBlock.NumberU64(), err))
				return
			}
			// Create a snapshot if one is due at this block.
			stateRootFuture, err = executor.MaybeSnapshot(gethBlock.NumberU64(), stateRootFuture)
			if err != nil {
				reportIssue(fmt.Errorf("failed to create snapshot at block %d: %w", gethBlock.NumberU64(), err))
				return
			}
			result := &processResult{
				decoded:           decoded,
				receipts:          receipts,
				stateRoot:         stateRootFuture,
				hashOfParentBlock: hashOfParentBlock,
				parentHeader:      parentHeader,
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

			err := checkBlockResults(executor, block, result.receipts, result.stateRoot, result.hashOfParentBlock, blockDB, &replayLoopContext, slog.Default())
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
				verifier.submit(block, result.parentHeader, result.hashHistory)
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
