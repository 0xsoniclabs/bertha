package app

import (
	"context"
	"errors"
	"fmt"
	"io"
	"iter"
	"log/slog"
	"maps"
	"math/big"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	// _ "[github.com/ianlancetaylor/cgosymbolizer](http://github.com/ianlancetaylor/cgosymbolizer)" // Enable to resolve symbols across cgo calls (this breaks Go symbols)
	"github.com/0xsoniclabs/blockdb"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	_ "github.com/0xsoniclabs/carmen/go/state/gostate"
	"github.com/0xsoniclabs/tracy"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/urfave/cli/v3"
)

//go:generate mockgen -source=replay.go -destination=replay_mock.go -package=app

var (
	jsonGenesisFlag = &cli.StringFlag{
		Name:    "json-genesis",
		Aliases: []string{"g"},
		Usage:   "JSON encoded genesis data to use for replaying the blockchain",
	}

	stateDbDirectoryFlag = &cli.StringFlag{
		Name:    "state-db-dir",
		Aliases: []string{"sdb"},
		Usage:   "Path to the state database directory (default: OS-defined temporary directory)",
		Value:   "",
	}

	withArchiveFlag = &cli.BoolFlag{
		Name:    "with-archive",
		Aliases: []string{"a"},
		Usage:   "Use the archive mode for the state database",
		Value:   false,
	}

	keepDbFlag = &cli.BoolFlag{
		Name:  "keep-db",
		Usage: "Keep the state database after running the replay",
	}

	initDbFlag = &cli.StringFlag{
		Name:  "init-db-dir",
		Usage: "Path to a state database directory to use to init the state database. The database will be copied to a temporary folder or the directory specified by '--state-db-dir' before replaying.",
		Value: "",
	}

	dbSchema = &cli.IntFlag{
		Name:    "db-schema",
		Aliases: []string{"schema"},
		Usage:   "Block database schema version to use",
		Value:   5,
	}

	dbVariant = &cli.StringFlag{
		Name:    "db-variant",
		Aliases: []string{"variant"},
		Usage:   "Block database variant to use (" + strings.Join(getListOfVariants(), ", ") + ")",
		Value:   "go-file",
	}

	usePipelineFlag = &cli.BoolFlag{
		Name:  "use-pipeline",
		Usage: "Enable the replay pipeline (default: true)",
		Value: true,
	}

	snapshotInterval = &cli.Uint64Flag{
		Name:    "snapshot-interval",
		Aliases: []string{"si"},
		Usage:   "Interval of blocks at which to perform database snapshots (0 = disabled)",
		Value:   0,
	}

	overwriteStateRoot = &cli.BoolFlag{
		Name:  "overwrite-state-roots",
		Usage: "Overwrite the state roots in the block database with the ones computed from the state",
		Value: false,
	}

	noStateRootCheck = &cli.BoolFlag{
		Name:    "no-state-root-check",
		Aliases: []string{"no-src"},
		Usage:   "Skip checking the state roots with the ones stored in the block database",
		Value:   false,
	}

	confirmAllPromptsFlag = &cli.BoolFlag{
		Name:  "y",
		Usage: "Automatically confirm all prompts",
		Value: false,
	}
)

// getListOfVariants returns a sorted list of all registered database variants.
func getListOfVariants() []string {
	variants := map[string]struct{}{}
	for config := range carmen.GetAllRegisteredStateFactories() {
		variants[string(config.Variant)] = struct{}{}
	}
	return slices.Sorted(maps.Keys(variants))
}

func getReplayCommand() *cli.Command {
	return &cli.Command{
		Name:   "replay",
		Usage:  "replay the full block chain from the block database",
		Action: runReplay,
		Flags: []cli.Flag{
			jsonGenesisFlag,
			blockDatabaseDirectoryFlag,
			stateDbDirectoryFlag,
			withArchiveFlag,
			keepDbFlag,
			initDbFlag,
			dbSchema,
			dbVariant,
			startBlockFlag,
			endBlockFlag,
			usePipelineFlag,
			snapshotInterval,
			overwriteStateRoot,
			noStateRootCheck,
			confirmAllPromptsFlag,
		},
	}
}

func runReplay(ctx context.Context, c *cli.Command) (err error) {

	genesisFileName := c.String(jsonGenesisFlag.Name)
	stateDbDirectory := c.String(stateDbDirectoryFlag.Name)
	blockDbDirectory := c.String(blockDatabaseDirectoryFlag.Name)
	withArchive := c.Bool(withArchiveFlag.Name)
	keepDb := c.Bool(keepDbFlag.Name)
	initDbDir := c.String(initDbFlag.Name)
	startBlock := c.Uint64(startBlockFlag.Name)
	endBlock := c.Uint64(endBlockFlag.Name)
	usePipeline := c.Bool(usePipelineFlag.Name)
	snapshotInterval := c.Uint64(snapshotInterval.Name)
	overwriteStateRoot := c.Bool(overwriteStateRoot.Name)
	confirmAllPrompts := c.Bool(confirmAllPromptsFlag.Name)
	noStateRootCheck := c.Bool(noStateRootCheck.Name)

	schema := carmen.Schema(c.Int(dbSchema.Name))
	variant := carmen.Variant(c.String(dbVariant.Name))

	snapshotHandler := NewSnapshotHandler(snapshotInterval)

	slog.Info("Loading genesis file", "file", genesisFileName)
	// Create a temporary directory for the state database
	if stateDbDirectory == "" {
		if startBlock > 0 && initDbDir == "" {
			return fmt.Errorf("existing state or initial database directory must be specified when starting from a non-genesis block")
		}
		stateDbDirectory = os.TempDir()
		stateDbDirectory, err = os.MkdirTemp(stateDbDirectory, "replay_chain_state_")
	}
	if err != nil {
		return fmt.Errorf("failed to create temporary state database directory: %w", err)
	}
	slog.Info("Creating state database", "directory", stateDbDirectory)
	if initDbDir != "" {
		slog.Info("Copying initial state database", "source_directory", initDbDir, "destination_directory", stateDbDirectory)
		if isEmpty, err := IsEmptyOrMissingDir(stateDbDirectory); err != nil {
			return fmt.Errorf("failed to check if state database directory %q is empty: %w", stateDbDirectory, err)
		} else if !isEmpty {
			return fmt.Errorf("state database directory %q is not empty. Please specify an empty directory to be initialized or use a temporary directory", stateDbDirectory)
		}
		err = os.CopyFS(stateDbDirectory, os.DirFS(initDbDir))
		if err != nil {
			return fmt.Errorf("failed to copy initial state database %q in destination directory %q: %w", initDbDir, stateDbDirectory, err)
		}
	}

	if !keepDb {
		slog.Warn("State database will be deleted after replay (use --keep-db to keep it)")
		defer func() {
			slog.Info("Removing state database directory", "directory", stateDbDirectory)
			err = errors.Join(err, os.RemoveAll(stateDbDirectory))
			if snapshotHandler.lastSnapshot != nil {
				backupDir := fmt.Sprintf("%s_snapshot_%d", stateDbDirectory, snapshotHandler.lastSnapshot)
				if err == nil || errors.Is(err, context.Canceled) {
					slog.Info("Removing latest snapshot directory", "directory", backupDir)
					err = errors.Join(err, os.RemoveAll(backupDir))
				} else {
					slog.Info(fmt.Sprintf("Replay terminated with error. The latest snapshot will be kept for inspection using '%s' flag", initDbFlag.Name), "directory", backupDir)
				}
			}
		}()

	}

	if snapshotInterval > 0 {
		matches, err := filepath.Glob(stateDbDirectory + "_snapshot_*")
		if err != nil {
			return fmt.Errorf("failed to check existing snapshots in state database directory %q: %w", stateDbDirectory, err)
		}
		if len(matches) > 0 {
			slog.Warn("Existing snapshots found for state database directory", "directory", stateDbDirectory, "snapshots_found", len(matches))
			if !confirmAllPrompts {
				fmt.Printf("Do you want to delete the existing snapshots and continue (y/n)? ")
				var response string
				fmt.Scanln(&response)
				if strings.ToLower(strings.TrimSpace(response)) != "y" {
					slog.Error("Execution aborted by the user")
					return nil
				}
			}
		}

		slog.Info("Intermediate state database snapshots enabled", "interval_blocks", snapshotInterval)
		if strings.Contains(string(variant), "flat") {
			slog.Warn("Snapshots are currently not supported with flat database variants; consider disabling snapshots or using a different variant")
		}
	}

	// Load genesis data from the specified file.
	genesis, err := ReadGenesisFromFile(genesisFileName)
	if err != nil {
		return fmt.Errorf("failed to read genesis file %q: %w", genesisFileName, err)
	}
	chainId := genesis.ChainId

	// Open State Database in new directory.
	params := StateParameters{
		Directory:   stateDbDirectory,
		WithArchive: withArchive,
		Schema:      schema,
		Variant:     variant,
	}

	state, err := NewState(params)
	if err != nil {
		return fmt.Errorf("failed to create state: %w", err)
	}
	chain := &stateChainAdapter{
		chainId:         chainId,
		state:           state,
		schema:          schema,
		snapshotHandler: snapshotHandler,
	}
	// Because snapshots invalidate the state, we need to close it here.
	defer func() {
		slog.Info("Closing state database", "directory", stateDbDirectory)
		err = errors.Join(err, chain.state.Close())
	}()

	if startBlock == 0 {
		slog.Info("Starting replay from genesis")
		// Apply genesis data to the state database.
		if err := state.ApplyGenesis(genesis); err != nil {
			return fmt.Errorf("failed to apply genesis data: %w", err)
		}
	} else {
		slog.Info("Starting replay from block", "block_number", startBlock)
	}
	stateRoot, err := state.GetStateRoot().Await().Get()
	if err != nil {
		return fmt.Errorf("failed to get state root: %w", err)
	}
	slog.Info("Loaded state", "chain_id", chainId, "root_hash", stateRoot)

	// Open the block database.
	slog.Info("Opening block database", "directory", blockDbDirectory)
	var database blockdb.BlockDB
	if overwriteStateRoot {
		slog.Info("State root overwriting enabled")
		database, err = blockdb.OpenRocksDBForWriting(blockDbDirectory)
	} else {
		database, err = blockdb.OpenRocksDBForReading(blockDbDirectory)
	}
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	// Load metadata for the chain.
	metadata, err := GetMetadataForChain(chainId)
	if err != nil {
		return fmt.Errorf("failed to get metadata for chain ID %d: %w", chainId, err)
	}

	// Prepare the progress logger.
	progress := startProgressLogger()
	defer func() {
		slog.Info(progress.GetSummary())
	}()

	// Pick the replay method.
	run := runReplayLoop
	if usePipeline {
		slog.Info("Using replay pipeline")
		run = runReplayPipeline
	} else {
		slog.Info("Using simple replay loop")
	}

	// ---- Start Replay ----
	blocks := database.GetRange(chainId, startBlock, endBlock)

	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot: New(overwriteStateRoot, confirmAllPrompts),
		skipStateRootCheck: noStateRootCheck,
		stateRootNotSet:    false,
	}

	return run(
		ctx, blocks, chain, metadata, database, replayLoopContext, func(block *types.Block) {
			if info := progress.LogProgress(block); len(info) > 0 {
				slog.Info(info)
			}
		},
	)
}

// --- progress logger ---

// progressLogger is a UX helper utility for the replay command producing
// the main progress log output.
type progressLogger struct {
	start                  time.Time
	lastUpdate             time.Time
	lastReportedBlockTime  time.Time
	lastProcessedBlockTime time.Time
	txCounter              uint64
	gasCounter             uint64
	lastTxCounter          uint64
	lastGasCounter         uint64
	firstBlockTime         time.Time
}

func startProgressLogger() *progressLogger {
	now := time.Now()
	return &progressLogger{
		start:      now,
		lastUpdate: now,
	}
}

func (p *progressLogger) LogProgress(block *types.Block) string {
	// Keep track of metrics for logging purposes.
	p.lastProcessedBlockTime = time.Unix(int64(block.Time()), 0)
	p.txCounter += uint64(len(block.Transactions()))
	p.gasCounter += block.GasUsed()

	number := block.NumberU64()
	if number == 0 {
		p.firstBlockTime = p.lastProcessedBlockTime
		p.lastReportedBlockTime = p.lastProcessedBlockTime
		return ""
	}

	// Periodically log the progress of the replay.
	if number%10_000 != 0 {
		return ""
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

	return fmt.Sprintf(
		"Processing block %d from %v @ t=%2d:%02d:%02d, %.2f txs/s, %.2f MGas/s, %.2fx realtime",
		number,
		currentBlockTime.Format(time.DateTime),
		int(runtime.Hours()),
		int(runtime.Minutes())%60,
		int(runtime.Seconds())%60,
		float64(deltaTx)/deltaTime.Seconds(),
		float64(deltaGas)/deltaTime.Seconds()/1000/1000,
		deltaBlockTime.Seconds()/deltaTime.Seconds(),
	)
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
	metadata Metadata,
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

		gethBlock, err := ConvertToGethBlock(block)
		if err != nil {
			return fmt.Errorf("failed to convert block %d: %w", block.Number, err)
		}

		// Run the transactions in the block against the state database.
		receipts, stateRoot, err := chain.ApplyBlock(gethBlock, metadata)
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
	metadata Metadata,
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
		signer := types.LatestSignerForChainID(new(big.Int).SetUint64(chain.ChainId()))
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
			gethBlock, err := ConvertToGethBlock(block)
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
			receipts, stateRootFuture, err := chain.ApplyBlock(gethBlock, metadata)
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
			fmt.Scanln(&response)
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
			err = blockDB.Update(chain.ChainId(), block)
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
	ChainId() uint64
	IsMptConformant() bool
	IsVerkleConformant() bool
	ApplyBlock(*types.Block, Metadata) (
		types.Receipts,
		future.Future[result.Result[common.Hash]],
		error,
	)
}

// stateChainAdapter is an adapter that allows the State to be used as a Chain.
type stateChainAdapter struct {
	chainId         uint64
	state           *State
	stateRwMutex    sync.Mutex
	schema          carmen.Schema
	snapshotHandler *SnapshotHandler
}

func (a *stateChainAdapter) ChainId() uint64 {
	return a.chainId
}

func (a *stateChainAdapter) IsMptConformant() bool {
	return a.schema == 5
}

func (a *stateChainAdapter) IsVerkleConformant() bool {
	return a.schema == 6
}

func (a *stateChainAdapter) ApplyBlock(
	block *types.Block,
	metadata Metadata,
) (
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
	receipts, err := a.state.ApplyBlock(
		a.chainId,
		block,
		metadata,
	)
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

// SnapshotHandler is a utility to handle intermediate state database snapshots.
// It allows to create snapshots at specified block intervals, making sure that only one snapshot is kept at a time.
type SnapshotHandler struct {
	blockInterval uint64
	lastSnapshot  *uint64
}

func NewSnapshotHandler(blockInterval uint64) *SnapshotHandler {
	return &SnapshotHandler{
		blockInterval: blockInterval,
		lastSnapshot:  nil,
	}
}

// ShouldCreateSnapshot returns true if a snapshot should be created at the given block number.
// A snapshot should be created if the block interval is set and the current block number is bigger than 0 and is a multiple of the interval.
func (s *SnapshotHandler) ShouldCreateSnapshot(currentBlock uint64) bool {
	return s.blockInterval > 0 && currentBlock > 0 && currentBlock%s.blockInterval == 0
}

// Snapshot creates a snapshot of the state database at the given block number.
// It closes and reopens the state to ensure all data is flushed to disk,
// copies the state database directory to a new location, and removes the previous snapshot if it exists.
func (s *SnapshotHandler) Snapshot(currentBlock uint64, state *State) (*State, error) {
	stateDBDir := state.stateParameter.Directory
	if !s.ShouldCreateSnapshot(currentBlock) {
		return state, nil
	}

	slog.Info("Creating state database snapshot", "block_number", currentBlock)
	backupDir := fmt.Sprintf("%s_snapshot_%d", stateDBDir, currentBlock)
	os.RemoveAll(backupDir) // remove existing snapshot if any
	// Close and reopen the state to ensure all data is flushed to disk
	err := state.Close()
	if err != nil {
		return nil, fmt.Errorf("failed to close state database before snapshot: %w", err)
	}
	// Create the snapshot by copying the state database directory
	err = os.CopyFS(backupDir, os.DirFS(stateDBDir))
	if err != nil {
		return nil, fmt.Errorf("failed to copy state database for snapshot: %w", err)
	}
	// Open the state again
	state, err = NewState((*state).stateParameter)
	if err != nil {
		return nil, fmt.Errorf("failed to reopen state database after snapshot: %w", err)
	}
	if s.lastSnapshot != nil {
		slog.Info("Removing previous state database snapshot", "directory", *s.lastSnapshot)
		prevBackupDir := fmt.Sprintf("%s_snapshot_%d", stateDBDir, *s.lastSnapshot)
		err = os.RemoveAll(prevBackupDir)
		if err != nil {
			return nil, fmt.Errorf("failed to remove previous state database snapshot: %w", err)
		}
	}
	s.lastSnapshot = &currentBlock
	return state, nil
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

// IsEmptyOrMissingDir returns true if a directory is missing or empty.
func IsEmptyOrMissingDir(path string) (bool, error) {
	_, err := os.Stat(path)
	if os.IsNotExist(err) {
		return true, nil // non-existent
	}
	if err != nil {
		return false, err
	}
	f, err := os.Open(path)
	if err != nil {
		return false, err
	}
	defer f.Close()

	_, err = f.Readdir(1) // try to read a single entry
	if err == io.EOF {
		return true, nil // empty
	}
	return false, err // either not empty or some other error
}
