// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

// Package replay allows to replay the block history.
package replay

import (
	"context"
	"errors"
	"fmt"
	"iter"
	"log/slog"
	"math/big"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	// _ "[github.com/ianlancetaylor/cgosymbolizer](http://github.com/ianlancetaylor/cgosymbolizer)" // Enable to resolve symbols across cgo calls (this breaks Go symbols)

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	_ "github.com/0xsoniclabs/carmen/go/state/gostate"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
)

//go:generate mockgen -source=replay.go -destination=replay_mock.go -package=replay

type ReplayArgs struct {
	JSONGenesisFile    string
	BlockDBDir         string
	StateDBDir         string
	InitDBDir          string
	KeepDB             bool
	WithArchive        bool
	DBSchema           carmen.Schema
	DBVariant          carmen.Variant
	UsePipeline        bool
	StartBlock         uint64
	EndBlock           uint64
	SnapshotInterval   uint64
	SnapshotStartBlock uint64
	SnapshotEndBlock   uint64
	SnapshotNumToKeep  uint64
	OverwriteStateRoot bool
	NoStateRootCheck   bool
	LogDBSize          bool
	ConfirmAllPrompts  bool
}

func Replay(ctx context.Context, args ReplayArgs) (err error) {
	snapshotHandler := NewSnapshotHandler(args.SnapshotInterval, args.SnapshotStartBlock, args.SnapshotEndBlock, args.SnapshotNumToKeep)

	slog.Info("Loading genesis file", "file", args.JSONGenesisFile)
	// Create a temporary directory for the state database
	if args.StateDBDir == "" {
		if args.StartBlock > 0 && args.InitDBDir == "" {
			return fmt.Errorf("existing state or initial database directory must be specified when starting from a non-genesis block")
		}
		args.StateDBDir = os.TempDir()
		args.StateDBDir, err = os.MkdirTemp(args.StateDBDir, "replay_chain_state_")
	}
	if err != nil {
		return fmt.Errorf("failed to create temporary state database directory: %w", err)
	}
	slog.Info("Creating state database", "directory", args.StateDBDir)
	if args.InitDBDir != "" {
		slog.Info("Copying initial state database", "source_directory", args.InitDBDir, "destination_directory", args.StateDBDir)
		if isEmpty, err := utils.IsEmptyOrMissingDir(args.StateDBDir); err != nil {
			return fmt.Errorf("failed to check if state database directory %q is empty: %w", args.StateDBDir, err)
		} else if !isEmpty {
			return fmt.Errorf("state database directory %q is not empty. Please specify an empty directory to be initialized or use a temporary directory", args.StateDBDir)
		}
		err = os.CopyFS(args.StateDBDir, os.DirFS(args.InitDBDir))
		if err != nil {
			return fmt.Errorf("failed to copy initial state database %q in destination directory %q: %w", args.InitDBDir, args.StateDBDir, err)
		}
	}

	if !args.KeepDB {
		slog.Warn("State database will be deleted after replay (use --keep-db to keep it)")
		defer func() {
			slog.Info("Removing state database directory", "directory", args.StateDBDir)
			err = errors.Join(err, os.RemoveAll(args.StateDBDir))
			snapshotDirs := snapshotHandler.GetSnapshotDirs(args.StateDBDir)
			if len(snapshotDirs) > 0 {
				if err == nil || errors.Is(err, context.Canceled) {
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
		}()

	}

	if args.SnapshotInterval > 0 {
		matches, err := filepath.Glob(args.StateDBDir + "_snapshot_*")
		if err != nil {
			return fmt.Errorf("failed to check existing snapshots in state database directory %q: %w", args.StateDBDir, err)
		}
		if len(matches) > 0 {
			slog.Warn("Existing snapshots found for state database directory", "directory", args.StateDBDir, "snapshots_found", len(matches))
			if !args.ConfirmAllPrompts {
				fmt.Printf("Do you want to delete the existing snapshots and continue (y/n)? ")
				var response string
				if _, err := fmt.Scanln(&response); err != nil {
					return fmt.Errorf("failed to read user input: %w", err)
				}
				if strings.ToLower(strings.TrimSpace(response)) != "y" {
					slog.Error("Execution aborted by the user")
					return nil
				}
			}
		}

		slog.Info("Intermediate state database snapshots enabled", "interval_blocks", args.SnapshotInterval, "start_block", args.SnapshotStartBlock, "end_block", args.SnapshotEndBlock, "snapshot to keep", args.SnapshotNumToKeep)
		if strings.Contains(string(args.DBVariant), "flat") {
			slog.Warn("Snapshots are currently not supported with flat database variants; consider disabling snapshots or using a different variant")
		}
	}

	if args.LogDBSize {
		slog.Warn("DB size log enabled. This will trigger a flush with every progress report and reduce performance")
	}

	// Load genesis data from the specified file.
	genesis, err := ReadGenesisFromFile(args.JSONGenesisFile)
	if err != nil {
		return fmt.Errorf("failed to read genesis file %q: %w", args.JSONGenesisFile, err)
	}
	chainID := genesis.ChainID

	metadataStore, err := NewStaticMetadataStore(chainID, slog.Default())
	if err != nil {
		return fmt.Errorf("failed to create metadata store for chain ID %d: %w", chainID, err)
	}

	// Open State Database in new directory.
	params := StateParameters{
		Directory:   args.StateDBDir,
		WithArchive: args.WithArchive,
		Schema:      args.DBSchema,
		Variant:     args.DBVariant,
	}

	state, err := NewState(params, metadataStore, chainID)
	if err != nil {
		return fmt.Errorf("failed to create state: %w", err)
	}
	chain := &stateChainAdapter{
		chainID:         chainID,
		state:           state,
		schema:          args.DBSchema,
		snapshotHandler: snapshotHandler,
	}
	// Because snapshots invalidate the state, we need to close it here.
	defer func() {
		slog.Info("Closing state database", "directory", args.StateDBDir)
		err = errors.Join(err, chain.state.Close())
	}()

	if args.StartBlock == 0 {
		slog.Info("Starting replay from genesis")
		// Apply genesis data to the state database.
		if err := state.ApplyGenesis(genesis); err != nil {
			return fmt.Errorf("failed to apply genesis data: %w", err)
		}
	} else {
		slog.Info("Starting replay from block", "block_number", args.StartBlock)
	}
	stateRoot, err := state.GetStateRoot().Await().Get()
	if err != nil {
		return fmt.Errorf("failed to get state root: %w", err)
	}
	slog.Info("Loaded state", "chain_id", chainID, "root_hash", stateRoot)

	// Open the block database.
	slog.Info("Opening block database", "directory", args.BlockDBDir)
	var database blockdb.BlockDB
	if args.OverwriteStateRoot {
		slog.Info("State root overwriting enabled")
		database, err = blockdb.OpenRocksDBForWriting(args.BlockDBDir)
	} else {
		database, err = blockdb.OpenRocksDBForReading(args.BlockDBDir)
	}
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	// Prepare the progress logger.
	progress := startProgressLogger(state, args.StateDBDir, args.LogDBSize)
	defer func() {
		slog.Info(progress.GetSummary())
	}()

	// Pick the replay method.
	run := runReplayLoop
	if args.UsePipeline {
		slog.Info("Using replay pipeline")
		run = runReplayPipeline
	} else {
		slog.Info("Using simple replay loop")
	}

	// ---- Start Replay ----
	blocks := database.GetRange(chainID, args.StartBlock, args.EndBlock)

	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot: New(args.OverwriteStateRoot, args.ConfirmAllPrompts),
		skipStateRootCheck: args.NoStateRootCheck,
		stateRootNotSet:    false,
	}

	return run(
		ctx, blocks, chain, database, replayLoopContext, func(block *types.Block) {
			info, err := progress.LogProgress(block)
			if err != nil {
				slog.Error("Failed to log progress", "error", err)
				return
			}
			if len(info) > 0 {
				slog.Info(info)
			}
		},
	)
}

// --- progress logger ---

// progressLogger is a UX helper utility for the replay command producing
// the main progress log output.
type progressLogger struct {
	state                  *State
	stateDBDirectory       string
	start                  time.Time
	lastUpdate             time.Time
	lastReportedBlockTime  time.Time
	lastProcessedBlockTime time.Time
	txCounter              uint64
	gasCounter             uint64
	lastTxCounter          uint64
	lastGasCounter         uint64
	firstBlockTime         time.Time
	logDBSize              bool
}

func startProgressLogger(state *State, stateDBDirectory string, logDBSize bool) *progressLogger {
	now := time.Now()
	return &progressLogger{
		state:            state,
		stateDBDirectory: stateDBDirectory,
		start:            now,
		lastUpdate:       now,
		logDBSize:        logDBSize,
	}
}

func (p *progressLogger) LogProgress(block *types.Block) (string, error) {
	// Keep track of metrics for logging purposes.
	p.lastProcessedBlockTime = time.Unix(int64(block.Time()), 0)
	p.txCounter += uint64(len(block.Transactions()))
	p.gasCounter += block.GasUsed()

	number := block.NumberU64()
	if number == 0 {
		p.firstBlockTime = p.lastProcessedBlockTime
		p.lastReportedBlockTime = p.lastProcessedBlockTime
		return "", nil
	}

	// Periodically log the progress of the replay.
	if number%10_000 != 0 {
		return "", nil
	}

	currentBlockTime := time.Unix(int64(block.Time()), 0)
	deltaBlockTime := currentBlockTime.Sub(p.lastReportedBlockTime)
	p.lastReportedBlockTime = currentBlockTime
	deltaTx := p.txCounter - p.lastTxCounter
	deltaGas := p.gasCounter - p.lastGasCounter
	p.lastTxCounter = p.txCounter
	p.lastGasCounter = p.gasCounter

	now := time.Now()
	deltaTime := now.Sub(p.lastUpdate)
	p.lastUpdate = now

	runtime := time.Since(p.start)

	// Optionally log the size of the state database.
	var sizeStr string
	if p.logDBSize {
		err := p.state.db.Flush()
		if err != nil {
			return "", fmt.Errorf("failed to flush state database: %w", err)
		}
		liveSize, err := utils.DirSize(filepath.Join(p.stateDBDirectory, "live"))
		if err != nil {
			return "", fmt.Errorf("failed to compute live database size: %w", err)
		}

		sizeStr = fmt.Sprintf(", live DB size: %.3f GiB", float64(liveSize)/1024/1024/1024)

		archiveDir := filepath.Join(p.stateDBDirectory, "archive")
		archiveMissing, err := utils.IsEmptyOrMissingDir(archiveDir)
		if err != nil {
			return "", fmt.Errorf("failed to check existence of archive database directory: %w", err)
		}
		if !archiveMissing {
			archiveSize, err := utils.DirSize(archiveDir)
			if err != nil {
				return "", fmt.Errorf("failed to compute archive database size: %w", err)
			}
			sizeStr += fmt.Sprintf(", archive DB size: %.3f GiB", float64(archiveSize)/1024/1024/1024)
		} else {
			sizeStr += ", archive DB size: n/a"
		}
	}

	return fmt.Sprintf(
		"Processing block %d from %v @ t=%2d:%02d:%02d, %.2f txs/s, %.2f MGas/s, %.2fx realtime%s",
		number,
		currentBlockTime.Format(time.DateTime),
		int(runtime.Hours()),
		int(runtime.Minutes())%60,
		int(runtime.Seconds())%60,
		float64(deltaTx)/deltaTime.Seconds(),
		float64(deltaGas)/deltaTime.Seconds()/1000/1000,
		deltaBlockTime.Seconds()/deltaTime.Seconds(),
		sizeStr,
	), nil
}

func (p *progressLogger) GetSummary() string {
	duration := time.Since(p.start)
	deltaBlockTime := p.lastProcessedBlockTime.Sub(p.firstBlockTime)
	return fmt.Sprintf(
		"Replay finished in %v, processed %d txs (%.2f Tx/s), used %.3f TGas (%.2f MGas/s), %.2fx realtime",
		duration,
		p.txCounter,
		float64(p.txCounter)/duration.Seconds(),
		float64(p.gasCounter)/1e12,
		float64(p.gasCounter)/duration.Seconds()/1e6,
		deltaBlockTime.Seconds()/duration.Seconds(),
	)
}

// --- block replay logic ---

// runReplayLoop processes the blocks from the given iterator, applying them
// to the chain and checking the results against the expected values in the
// blocks. This is the main business logic of the replay command.
func runReplayLoop(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	chain Chain,
	blockDB blockdb.BlockDB,
	replayLoopContext ReplayLoopContext,
	onBlockDone func(block *types.Block),
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

		// Run the transactions in the block against the state database.
		receipts, stateRoot, err := chain.ApplyBlock(gethBlock)
		if err != nil {
			return fmt.Errorf("failed to apply block %d: %w", block.Number, err)
		}

		// Check the receipts against the expected values in the block.
		if err := checkBlockResults(chain, block, receipts, stateRoot, blockDB, &replayLoopContext); err != nil {
			return err
		}

		// Report the progress of the replay.
		if onBlockDone != nil {
			onBlockDone(gethBlock)
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
	onBlockDone func(block *types.Block),
) error {
	const bufferSize = 1024

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
		decoded   *decodedBlock
		receipts  types.Receipts
		stateRoot future.Future[result.Result[common.Hash]]
	}

	// Channels between stages
	decodedBlocks := make(chan *decodedBlock, bufferSize)
	results := make(chan *processResult, bufferSize)
	done := make(chan struct{})
	abort := make(chan struct{})

	// Utility to collect errors.
	issue := atomic.Pointer[error]{}
	reportIssue := func(err error) {
		issue.Store(&err)
		close(abort)
	}

	// Stage 1: Decode blocks
	go func() {
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
	}()

	// Stage 2: Apply blocks
	go func() {
		defer close(results)
		for decoded := range decodedBlocks {
			gethBlock := decoded.geth
			receipts, stateRootFuture, err := chain.ApplyBlock(gethBlock)
			if err != nil {
				reportIssue(fmt.Errorf("failed to apply block %d: %w", gethBlock.NumberU64(), err))
				return
			}
			result := &processResult{
				decoded:   decoded,
				receipts:  receipts,
				stateRoot: stateRootFuture,
			}
			select {
			case results <- result:
				continue
			case <-abort:
				return
			}
		}
	}()

	// Stage 3: Check results
	go func() {
		defer close(done)
		for result := range results {
			block := result.decoded.proto

			err := checkBlockResults(chain, block, result.receipts, result.stateRoot, blockDB, &replayLoopContext)
			if err != nil {
				reportIssue(err)
				return
			}

			// Report the progress of the replay.
			if onBlockDone != nil {
				onBlockDone(result.decoded.geth)
			}
		}
	}()
	<-done

	err := issue.Load()
	if err == nil {
		return nil
	}
	return *err
}

// checkBlockResults checks the results of applying a block against the
// expected values in the block, including receipt fields and the resulting
// state root. It is factored out to allow its use in both the simple replay
// loop and the pipeline version.
func checkBlockResults(
	chain Chain,
	block *blockdb.Block,
	receipts types.Receipts,
	stateRootFuture future.Future[result.Result[common.Hash]],
	blockDB blockdb.BlockDB,
	replayLoopContext *ReplayLoopContext,
) error {
	zone := tracy.ZoneBegin("CheckResults")
	overwriteStateRoot := &replayLoopContext.overwriteStateRoot
	noStateRootCheck := replayLoopContext.skipStateRootCheck

	for i, receipt := range receipts {
		want := block.Receipts[i]
		if receipt.Status != want.GetStatus() {
			return fmt.Errorf("receipt status mismatch for block %d, tx %d: expected %d, got %d",
				block.Number, i, want.GetStatus(), receipt.Status)
		}
		if receipt.CumulativeGasUsed != want.CumulativeGasUsed {
			return fmt.Errorf("receipt cumulative gas used mismatch for block %d, tx %d: expected %d, got %d",
				block.Number, i, want.CumulativeGasUsed, receipt.CumulativeGasUsed)
		}
		// TODO: check all receipt fields if needed.
	}

	// TODO:
	// - check logs

	// Check resulting state root.
	computedStateRoot, err := stateRootFuture.Await().Get()
	if err != nil {
		return fmt.Errorf("failed to get state root after applying block %d: %w", block.Number, err)
	}
	expectedStateRoot := getExpectedStateRoot(chain, block)

	if overwriteStateRoot.IsEnabled() {
		if !overwriteStateRoot.IsConfirmed() && expectedStateRoot != (common.Hash{}) && expectedStateRoot != computedStateRoot {
			slog.Warn("Block has existing state root", "block_number", block.Number, "existing", expectedStateRoot, "new", computedStateRoot)
			fmt.Printf("Are you sure you want to overwrite the existing state root (y/n)? ")
			var response string
			if _, err := fmt.Scanln(&response); err != nil {
				return fmt.Errorf("failed to read user input: %w", err)
			}
			if strings.ToLower(strings.TrimSpace(response)) != "y" {
				slog.Info("State roots overriding disabled from this point onward")
				overwriteStateRoot.Disable() //disabled by the user
			} else {
				slog.Info("Overriding state roots from this point onward")
				overwriteStateRoot.Confirm() //confirmed by the user
			}
		}

		// Double check in case user disabled the overwrite
		if overwriteStateRoot.IsEnabled() {
			updateStateRoot(chain, block, computedStateRoot)
			err = blockDB.Update(chain.ChainID(), block)
			if err != nil {
				return fmt.Errorf("failed to update block %d in database: %w", block.Number, err)
			}
		}
	}

	if !noStateRootCheck && !overwriteStateRoot.IsEnabled() {
		if expectedStateRoot == (common.Hash{}) {
			if !replayLoopContext.stateRootNotSet {
				slog.Warn("No state root set in the block DB. No checks will be performed", "block_number", block.Number)
				replayLoopContext.stateRootNotSet = true
			}
		} else if computedStateRoot != expectedStateRoot {
			return fmt.Errorf("state root mismatch after applying block %d: expected %x, got %x",
				block.Number, expectedStateRoot, computedStateRoot)
		}
	}

	zone.End()
	return nil
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
}

// stateChainAdapter is an adapter that allows the State to be used as a Chain.
type stateChainAdapter struct {
	chainID         uint64
	state           *State
	stateRwMutex    sync.Mutex
	schema          carmen.Schema
	snapshotHandler *SnapshotHandler
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

func (a *stateChainAdapter) ApplyBlock(block *types.Block) (
	types.Receipts,
	future.Future[result.Result[common.Hash]],
	error,
) {
	a.stateRwMutex.Lock()
	defer a.stateRwMutex.Unlock()

	zoneBlock := tracy.ZoneBegin("ProcessBlock")
	defer zoneBlock.End()

	// Block 0 is skipped since it is equivalent with the genesis data
	// import. The archive does not accept two blocks with the same number.
	if block.NumberU64() == 0 {
		return nil, a.state.GetStateRoot(), nil
	}

	// Apply the block to the state database.
	receipts, err := a.state.ApplyBlock(block)
	if err != nil {
		stateRoot := future.Future[result.Result[common.Hash]]{}
		return nil, stateRoot, fmt.Errorf("failed to apply block %d: %w", block.NumberU64(), err)
	}

	stateRoot := a.state.GetStateRoot()
	if a.snapshotHandler.ShouldCreateSnapshot(block.NumberU64()) {
		stateRoot = future.Immediate(stateRoot.Await())
		a.state, err = a.snapshotHandler.Snapshot(block.NumberU64(), a.state)
		if err != nil {
			return nil, stateRoot, fmt.Errorf("failed to create snapshot at block %d: %w", block.NumberU64(), err)
		}
	}
	// Return the receipts and the resulting state root.
	return receipts, stateRoot, nil
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

// SnapshotHandler is a utility to handle intermediate state database snapshots, allowing
// to create snapshots in a specific block interval.
type SnapshotHandler struct {
	blockInterval     uint64
	startBlock        uint64
	endBlock          uint64
	pastSnapshotList  []*uint64
	pastSnapshotIndex uint64
}

func NewSnapshotHandler(blockInterval uint64, startBlock uint64, endBlock uint64, snapshotToKeep uint64) *SnapshotHandler {
	return &SnapshotHandler{
		blockInterval:     blockInterval,
		startBlock:        startBlock,
		endBlock:          endBlock,
		pastSnapshotList:  make([]*uint64, snapshotToKeep),
		pastSnapshotIndex: 0,
	}
}

// ShouldCreateSnapshot returns true if a snapshot should be created at the given block number.
// A snapshot should be created if the block interval is set and the current block number is between
// the start block height and end block height and a multiple of the specified interval.
func (s *SnapshotHandler) ShouldCreateSnapshot(currentBlock uint64) bool {
	return s.blockInterval > 0 && currentBlock >= s.startBlock && currentBlock <= s.endBlock && currentBlock%s.blockInterval == 0
}

// Snapshot creates a snapshot of the state database at the given block number.
// It closes and reopens the state to ensure all data is flushed to disk,
// copies the state database directory to a new location, and removes a previous snapshot
// if needed to keep the specified number of snapshots.
func (s *SnapshotHandler) Snapshot(currentBlock uint64, state *State) (*State, error) {
	stateDBDir := state.stateParameter.Directory
	if !s.ShouldCreateSnapshot(currentBlock) {
		return state, nil
	}
	slog.Info("Creating state database snapshot", "block_number", currentBlock)

	// Remove the oldest snapshot if it exists
	oldestSnapshotDir := s.GetOldestSnapshotDir(stateDBDir)
	if s.pastSnapshotList[s.pastSnapshotIndex] != nil && oldestSnapshotDir != "" {
		slog.Info("Removing previous state database snapshot", "directory", oldestSnapshotDir)
		err := os.RemoveAll(oldestSnapshotDir)
		if err != nil {
			return nil, fmt.Errorf("failed to remove previous state database snapshot: %w", err)
		}
	}
	s.pastSnapshotList[s.pastSnapshotIndex] = &currentBlock
	s.pastSnapshotIndex = (s.pastSnapshotIndex + 1) % uint64(len(s.pastSnapshotList))

	// Create the snapshot by copying the state database directory
	snapshotDir := s.snapshotDir(stateDBDir, currentBlock)
	if err := os.RemoveAll(snapshotDir); err != nil { // remove existing snapshot if any
		return nil, fmt.Errorf("failed to remove existing snapshot directory %q: %w", snapshotDir, err)
	}
	// Close and reopen the state to ensure all data is flushed to disk
	err := state.Close()
	if err != nil {
		return nil, fmt.Errorf("failed to close state database before snapshot: %w", err)
	}
	err = os.CopyFS(snapshotDir, os.DirFS(stateDBDir))
	if err != nil {
		return nil, fmt.Errorf("failed to copy state database for snapshot: %w", err)
	}
	// Open the state again
	state, err = NewState(state.stateParameter, state.metadataStore, state.chainID)
	if err != nil {
		return nil, fmt.Errorf("failed to reopen state database after snapshot: %w", err)
	}

	slog.Info("Snapshot created successfully", "snapshot_directory", snapshotDir)
	return state, nil
}

// GetOldestSnapshotDir returns the path to the oldest snapshot created.
// If there are no snapshots, an empty string is returned.
func (s *SnapshotHandler) GetOldestSnapshotDir(stateDBDir string) string {
	for cnt := uint64(0); cnt < uint64(len(s.pastSnapshotList)); cnt++ {
		i := (s.pastSnapshotIndex + cnt) % uint64(len(s.pastSnapshotList))
		if s.pastSnapshotList[i] != nil {
			return s.snapshotDir(stateDBDir, *s.pastSnapshotList[i])
		}
	}
	return ""
}

// GetSnapshotDirs returns a list of paths to the current existing snapshots.
func (s *SnapshotHandler) GetSnapshotDirs(stateDBDir string) []string {
	list := make([]string, 0, len(s.pastSnapshotList))
	for _, pos := range s.pastSnapshotList {
		if pos != nil {
			list = append(list, s.snapshotDir(stateDBDir, *pos))
		}
	}
	return list
}

func (s *SnapshotHandler) snapshotDir(stateDBDir string, blockNum uint64) string {
	return fmt.Sprintf("%s_snapshot_%d", stateDBDir, blockNum)
}

// ReplayLoopContext is a utility struct to hold flags to pass to the `replayLoop` functions.
type ReplayLoopContext struct {
	overwriteStateRoot FlagWithConfirmation
	skipStateRootCheck bool
	stateRootNotSet    bool
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
