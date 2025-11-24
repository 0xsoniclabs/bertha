package app

import (
	"context"
	"errors"
	"fmt"
	"iter"
	"math"

	"github.com/0xsoniclabs/blockdb"
	"github.com/ethereum/go-ethereum/common"
	"github.com/schollz/progressbar/v3"
	"github.com/urfave/cli/v3"
)

var (
	chainIdFlag = &cli.Uint64Flag{
		Name:    "chain-id",
		Aliases: []string{"c"},
		Usage:   "Chain ID to verify",
		Value:   146, // Default to Sonic mainnet chain ID
	}

	startBlockFlag = &cli.Uint64Flag{
		Name:    "start-block",
		Aliases: []string{"s"},
		Usage:   "Starting block number to verify",
		Value:   0,
	}

	endBlockFlag = &cli.Uint64Flag{
		Name:    "end-block",
		Aliases: []string{"e"},
		Usage:   "Ending block number to verify (inclusive)",
		Value:   math.MaxUint64, // Default to the maximum block number
	}
)

func getVerifyCommand() *cli.Command {
	return &cli.Command{
		Name:   "verify",
		Usage:  "Verify the block database",
		Action: runVerify,
		Flags: []cli.Flag{
			blockDatabaseDirectoryFlag,
			chainIdFlag,
			startBlockFlag,
			endBlockFlag,
		},
	}
}

func runVerify(ctx context.Context, c *cli.Command) (err error) {

	dir := c.String(blockDatabaseDirectoryFlag.Name)
	chainId := c.Uint64(chainIdFlag.Name)
	startBlock := c.Uint64(startBlockFlag.Name)
	endBlock := c.Uint64(endBlockFlag.Name)

	fmt.Printf("Opening block database in %q ...\n", dir)
	database, err := blockdb.OpenRocksDBForReading(dir)
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	fmt.Printf("Verifying blocks for chain ID %d from block %d to block %d ...\n", chainId, startBlock, endBlock)

	numBlocks := int64(endBlock - startBlock)
	bar := progressbar.Default(numBlocks, "Verifying blocks")

	return verifyBlocks(ctx, database.GetRangeRev(
		chainId,
		startBlock,
		endBlock,
	), func(uint64) {
		_ = bar.Add(1) // Progress bar update errors are ignored
	})
}

func verifyBlocks(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	onVerifiedBlock func(number uint64),
) error {
	first := true
	blockHash := common.Hash{}
	for block, err := range blocks {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		if err != nil {
			return fmt.Errorf("failed to get block: %w", err)
		}
		if block == nil {
			return fmt.Errorf("encountered nil block")
		}

		if !first {
			if err := verifyBlock(blockHash, block); err != nil {
				return fmt.Errorf("block verification failed for block %d: %w", block.Number, err)
			}
		}
		if onVerifiedBlock != nil {
			onVerifiedBlock(block.Number)
		}
		first = false
		copy(blockHash[:], block.ParentHash)
	}
	return nil
}

func verifyBlock(
	hash common.Hash,
	block *blockdb.Block,
) error {
	gethBlock, err := ConvertToGethBlock(block)
	if err != nil {
		return fmt.Errorf("failed to convert block to Ethereum format: %w", err)
	}

	got := gethBlock.Hash()
	if got != hash {
		return fmt.Errorf("block hash mismatch: expected %x, got %x", hash, got)
	}

	return nil
}
