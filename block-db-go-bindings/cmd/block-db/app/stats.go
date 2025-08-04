package app

import (
	"context"
	"errors"
	"fmt"
	"iter"

	"github.com/0xsoniclabs/blockdb"
	"github.com/schollz/progressbar/v3"
	"github.com/urfave/cli/v3"
)

func getStatsCommand() *cli.Command {
	return &cli.Command{
		Name:   "stats",
		Usage:  "Derive statistics from the block database",
		Action: runStats,
		Flags: []cli.Flag{
			blockDatabaseDirectoryFlag,
			chainIdFlag,
			startBlockFlag,
			endBlockFlag,
		},
	}
}

func runStats(ctx context.Context, c *cli.Command) (err error) {

	dir := c.String(blockDatabaseDirectoryFlag.Name)
	chainId := c.Uint64(chainIdFlag.Name)
	startBlock := c.Uint64(startBlockFlag.Name)
	endBlock := c.Uint64(endBlockFlag.Name)

	fmt.Printf("Opening block database in %q ...\n", dir)
	database, err := blockdb.OpenDB(dir)
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	fmt.Printf("Deriving statistics for chain ID %d from block %d to block %d ...\n", chainId, startBlock, endBlock)

	numBlocks := int64(endBlock - startBlock)
	bar := progressbar.Default(numBlocks, "Deriving statistics")

	stats, err := deriveStats(ctx, database.GetRangeRev(
		chainId,
		startBlock,
		endBlock,
	), func(uint64) {
		_ = bar.Add(1) // Progress bar update errors are ignored
	})
	if err != nil {
		return fmt.Errorf("failed to derive statistics: %w", err)
	}
	fmt.Printf("Statistics derived successfully:\n")
	for txType, count := range stats.TransactionTypes {
		fmt.Printf("Transaction Type %d: %d occurrences\n", txType, count)
	}
	return nil
}

func deriveStats(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	onProcessedBlock func(number uint64),
) (stats, error) {
	stats := stats{
		TransactionTypes: make(map[uint64]int),
	}
	for block, err := range blocks {
		if ctx.Err() != nil {
			return stats, ctx.Err()
		}
		if err != nil {
			return stats, fmt.Errorf("failed to get block: %w", err)
		}
		if block == nil {
			return stats, fmt.Errorf("encountered nil block")
		}

		for _, tx := range block.Transactions {
			stats.TransactionTypes[tx.TransactionType]++
		}

		if onProcessedBlock != nil {
			onProcessedBlock(block.Number)
		}
	}
	return stats, nil
}

type stats struct {
	TransactionTypes map[uint64]int
}
