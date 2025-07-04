package app

import (
	"context"
	"errors"
	"fmt"
	"iter"
	"log/slog"
	"math"
	"math/big"
	"os"
	"time"

	"github.com/0xsoniclabs/blockdb"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	_ "github.com/0xsoniclabs/carmen/go/state/gostate"
	"github.com/0xsoniclabs/sonic/evmcore"
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
)

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
		},
	}
}

func runReplay(ctx context.Context, c *cli.Command) (err error) {

	genesisFileName := c.String(jsonGenesisFlag.Name)
	stateDbDirectory := c.String(stateDbDirectoryFlag.Name)
	blockDbDirectory := c.String(blockDatabaseDirectoryFlag.Name)
	withArchive := c.Bool(withArchiveFlag.Name)
	keepDb := c.Bool(keepDbFlag.Name)

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
		Directory: stateDbDirectory,
		Archive:   carmen.NoArchive,
	}
	if withArchive {
		params.Archive = carmen.S5Archive
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

	// Load corrections for the Sonic mainnet.
	corrections, err := GetSonicMainnetCorrections()
	if err != nil {
		return fmt.Errorf("failed to load corrections: %w", err)
	}

	// ---- Start Replay ----
	start := time.Now()
	lastUpdate := time.Now()
	lastReportedBlockTime := time.Unix(0, 0)
	lastProcessedBlockTime := time.Unix(0, 0)
	txCounter := uint64(0)
	gasCounter := uint64(0)
	lastTxCounter := uint64(0)
	lastGasCounter := uint64(0)

	firstBlockTime := time.Time{}

	defer func() {
		duration := time.Since(start)
		deltaBlockTime := lastProcessedBlockTime.Sub(firstBlockTime)
		slog.Info(fmt.Sprintf(
			"Replay finished in %v, processed %d txs (%.2f Tx/s), used %.3f TGas (%.2f MGas/s), %.2fx realtime",
			duration,
			txCounter,
			float64(txCounter)/duration.Seconds(),
			float64(gasCounter)/1e12,
			float64(gasCounter)/duration.Seconds()/1e6,
			deltaBlockTime.Seconds()/duration.Seconds(),
		))
	}()

	chain := &stateChainAdapter{
		chainId:          chainId,
		state:            state,
		blockHashHistory: &blockHashHistory{},
	}

	blocks := database.GetRange(chainId, 0, math.MaxUint64)
	return runReplayLoop(
		ctx, blocks, chain, corrections, func(block *types.Block) {
			// Keep track of metrics for logging purposes.
			lastProcessedBlockTime = time.Unix(int64(block.Time()), 0)
			txCounter += uint64(len(block.Transactions()))
			gasCounter += block.GasUsed()

			number := block.NumberU64()
			if number == 0 {
				firstBlockTime = lastProcessedBlockTime
				lastReportedBlockTime = lastProcessedBlockTime
				return
			}

			// Periodically log the progress of the replay.
			if number%10_000 != 0 {
				return
			}

			currentBlockTime := time.Unix(int64(block.Time()), 0)
			deltaBlockTime := currentBlockTime.Sub(lastReportedBlockTime)
			lastReportedBlockTime = currentBlockTime
			deltaTx := txCounter - lastTxCounter
			deltaGas := gasCounter - lastGasCounter
			lastTxCounter = txCounter
			lastGasCounter = gasCounter
			now := time.Now()
			deltaTime := now.Sub(lastUpdate)
			lastUpdate = now

			runtime := time.Since(start)

			fmt.Printf(
				"Processing block %d from %v @ t=%2d:%02d:%02d, %.2f txs/s, %.2f MGas/s, %.2fx realtime\n",
				number,
				currentBlockTime.Format(time.DateTime),
				int(runtime.Hours()),
				int(runtime.Minutes())%60,
				int(runtime.Seconds())%60,
				float64(deltaTx)/deltaTime.Seconds(),
				float64(deltaGas)/deltaTime.Seconds()/1000/1000,
				deltaBlockTime.Seconds()/deltaTime.Seconds(),
			)
		},
	)
}

func runReplayLoop(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	chain Chain,
	corrections Corrections,
	onBlockDone func(block *types.Block),
) error {
	for block, err := range blocks {
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
		receipts, stateRoot, err := chain.ApplyBlock(gethBlock, corrections)
		if err != nil {
			return fmt.Errorf("failed to apply block %d: %w", block.Number, err)
		}

		// Check the receipts against the expected values in the block.
		for i, receipt := range receipts {
			want := block.Receipts[i]
			if receipt.Status != want.Status {
				return fmt.Errorf("receipt status mismatch for block %d, tx %d: expected %d, got %d",
					block.Number, i, want.Status, receipt.Status)
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
		if common.Hash(block.StateRoot) != stateRoot {
			return fmt.Errorf("state root mismatch after applying block %d: expected %x, got %x",
				block.Number, block.StateRoot, stateRoot)
		}

		// Report the progress of the replay.
		if onBlockDone != nil {
			onBlockDone(gethBlock)
		}
	}
	return nil
}

// Chain is an interface for an evolving block chain.
type Chain interface {
	ApplyBlock(*types.Block, Corrections) (types.Receipts, common.Hash, error)
}

// stateChainAdapter is an adapter that allows the State to be used as a Chain.
type stateChainAdapter struct {
	chainId          uint64
	state            *State
	blockHashHistory *blockHashHistory
}

func (a *stateChainAdapter) ApplyBlock(
	block *types.Block,
	corrections Corrections,
) (
	types.Receipts,
	common.Hash,
	error,
) {
	// Track historic block hashes for the BLOCKHASH opcode.
	a.blockHashHistory.SetBlockHash(block.NumberU64()-1, block.ParentHash())

	// Block 0 is skipped since it is equivalent with the genesis data
	// import. The archive does not accept two blocks with the same number.
	if block.NumberU64() == 0 {
		return nil, a.state.GetStateRoot(), nil
	}

	// Apply the block to the state database.
	receipts, err := a.state.ApplyBlock(
		a.chainId,
		block,
		a.blockHashHistory,
		corrections,
	)
	if err != nil {
		return nil, common.Hash{}, fmt.Errorf("failed to apply block %d: %w", block.NumberU64(), err)
	}

	// Return the receipts and the resulting state root.
	return receipts, a.state.GetStateRoot(), nil
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

// --- bloch hash history adapter ---

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
