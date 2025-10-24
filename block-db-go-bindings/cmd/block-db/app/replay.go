package app

import (
	"context"
	"errors"
	"fmt"
	"iter"
	"log/slog"
	"maps"
	"os"
	"slices"
	"strings"
	"time"

	// _ "[github.com/ianlancetaylor/cgosymbolizer](http://github.com/ianlancetaylor/cgosymbolizer)" // Enable to resolve symbols across cgo calls (this breaks Go symbols)
	"github.com/0xsoniclabs/blockdb"
	"github.com/0xsoniclabs/blockdb/tracy"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	_ "github.com/0xsoniclabs/carmen/go/state/gostate"
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
			dbSchema,
			dbVariant,
			endBlockFlag,
		},
	}
}

func runReplay(ctx context.Context, c *cli.Command) (err error) {

	genesisFileName := c.String(jsonGenesisFlag.Name)
	stateDbDirectory := c.String(stateDbDirectoryFlag.Name)
	blockDbDirectory := c.String(blockDatabaseDirectoryFlag.Name)
	withArchive := c.Bool(withArchiveFlag.Name)
	keepDb := c.Bool(keepDbFlag.Name)
	endBlock := c.Uint64(endBlockFlag.Name)

	schema := carmen.Schema(c.Int(dbSchema.Name))
	variant := carmen.Variant(c.String(dbVariant.Name))

	slog.Info("Loading genesis file", "file", genesisFileName)

	// Create a temporary directory for the state database
	if stateDbDirectory == "" {
		stateDbDirectory = os.TempDir()
	}
	stateDbDirectory, err = os.MkdirTemp(stateDbDirectory, "replay_chain_state_")
	if err != nil {
		return fmt.Errorf("failed to create temporary state database directory: %w", err)
	}
	slog.Info("Creating state database", "directory", stateDbDirectory)
	if !keepDb {
		slog.Warn("State database will be deleted after replay (use --keep-db to keep it)")
		defer func() {
			slog.Info("Removing state database directory", "directory", stateDbDirectory)
			err = errors.Join(err, os.RemoveAll(stateDbDirectory))
		}()
	}

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
	defer func() {
		slog.Info("Closing state database", "directory", stateDbDirectory)
		err = errors.Join(err, state.Close())
	}()

	// Load genesis data from the specified file.
	genesis, err := ReadGenesisFromFile(genesisFileName)
	if err != nil {
		return fmt.Errorf("failed to read genesis file %q: %w", genesisFileName, err)
	}
	chainId := genesis.ChainId

	// Apply genesis data to the state database.
	if err := state.ApplyGenesis(genesis); err != nil {
		return fmt.Errorf("failed to apply genesis data: %w", err)
	}
	stateRoot := state.GetStateRoot()
	slog.Info("Loaded genesis", "chain_id", chainId, "root_hash", stateRoot)

	// Open the block database.
	slog.Info("Opening block database", "directory", blockDbDirectory)
	database, err := blockdb.OpenDB(blockDbDirectory)
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

	// ---- Start Replay ----

	blocks := database.GetRange(chainId, 0, endBlock)
	chain := &stateChainAdapter{
		chainId:         chainId,
		state:           state,
		isMptConformant: schema == 5,
	}
	return runReplayLoop(
		ctx, blocks, chain, metadata, func(block *types.Block) {
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
		zone := tracy.ZoneBegin("VerifyReceipts", 0x0000FF)
		for i, receipt := range receipts {
			want := block.Receipts[i]
			if receipt.Status != want.GetStatus() {
				zone.End()
				return fmt.Errorf("receipt status mismatch for block %d, tx %d: expected %d, got %d",
					block.Number, i, want.GetStatus(), receipt.Status)
			}
			if receipt.CumulativeGasUsed != want.CumulativeGasUsed {
				zone.End()
				return fmt.Errorf("receipt cumulative gas used mismatch for block %d, tx %d: expected %d, got %d",
					block.Number, i, want.CumulativeGasUsed, receipt.CumulativeGasUsed)
			}
			// TODO: check all receipt fields if needed.
		}
		zone.End()

		// TODO:
		// - check logs

		// Check resulting state root.
		zone = tracy.ZoneBegin("VerifyStateRoot", 0xFF00FF)
		if chain.IsMptConformant() && common.BytesToHash(block.StateRoot) != stateRoot {
			zone.End()
			return fmt.Errorf("state root mismatch after applying block %d: expected %x, got %x",
				block.Number, block.StateRoot, stateRoot)
		}
		zone.End()

		// Report the progress of the replay.
		if onBlockDone != nil {
			onBlockDone(gethBlock)
		}
	}
	return nil
}

// Chain is an interface for an evolving block chain.
type Chain interface {
	IsMptConformant() bool
	ApplyBlock(*types.Block, Metadata) (types.Receipts, common.Hash, error)
}

// stateChainAdapter is an adapter that allows the State to be used as a Chain.
type stateChainAdapter struct {
	chainId         uint64
	state           *State
	isMptConformant bool
}

func (a *stateChainAdapter) IsMptConformant() bool {
	return a.isMptConformant
}

func (a *stateChainAdapter) ApplyBlock(
	block *types.Block,
	metadata Metadata,
) (
	types.Receipts,
	common.Hash,
	error,
) {
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
		return nil, common.Hash{}, fmt.Errorf("failed to apply block %d: %w", block.NumberU64(), err)
	}

	// Return the receipts and the resulting state root.
	return receipts, a.state.GetStateRoot(), nil
}
