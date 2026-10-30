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

// Package verify allows to verify the integrity of the block data in the block database.
package verify

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"iter"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/ethereum/go-ethereum/common"
)

type VerifyArgs struct {
	DatabaseDir string
	ChainID     uint64
	StartBlock  uint64
	EndBlock    uint64
}

func Verify(
	ctx context.Context,
	args VerifyArgs,
	logger utils.Logger,
	progressIndicatorFactory utils.ProgressIndicatorFactory,
) (err error) {
	if args.StartBlock > args.EndBlock {
		return fmt.Errorf("start block %d is greater than end block %d", args.StartBlock, args.EndBlock)
	}

	logger.Info("Opening block database", "directory", args.DatabaseDir)
	database, err := blockdb.OpenRocksDBForReading(args.DatabaseDir)
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	startBlock, endBlock, hasBlocks, err := getStoredBlockRange(database, args.ChainID, args.StartBlock, args.EndBlock)
	if err != nil {
		return err
	}
	if !hasBlocks {
		logger.Warn("No blocks found",
			"chain_id", args.ChainID,
			"start_block", args.StartBlock,
			"end_block", args.EndBlock,
		)
		return nil
	}

	logger.Info("Verifying blocks",
		"chain_id", args.ChainID,
		"start_block", startBlock,
		"end_block", endBlock,
	)

	failedBundlesData, err := database.GetFailedBundles(args.ChainID)
	if err != nil {
		return fmt.Errorf("failed to get failed bundles: %w", err)
	}
	var failedBundles []struct {
		Block *uint64 `json:"block"`
	}
	if failedBundlesData != nil {
		if err := json.Unmarshal(failedBundlesData, &failedBundles); err != nil {
			return fmt.Errorf("failed to parse stored failed bundles: %w", err)
		}
	}
	blocksWithFailedBundles := map[uint64]bool{}
	for i, failedBundle := range failedBundles {
		if failedBundle.Block == nil {
			return fmt.Errorf("failed to parse stored failed bundles: entry %d has no block", i)
		}
		blocksWithFailedBundles[*failedBundle.Block] = true
	}

	numBlocks := int64(endBlock - startBlock + 1)
	progressIndicator := progressIndicatorFactory.New(numBlocks, "Verifying blocks")

	return verifyBlocks(
		ctx,
		database.GetRangeRev(
			args.ChainID,
			startBlock,
			endBlock,
		),
		blocksWithFailedBundles,
		func(uint64) {
			_ = progressIndicator.Add(1) // update errors are ignored
		},
		logger,
	)
}

// getStoredBlockRange returns the numbers of the lowest and the highest block stored for the given
// chain within the given block range. The third result is false if the range contains no block.
func getStoredBlockRange(
	database blockdb.BlockDB,
	chainID uint64,
	startBlock uint64,
	endBlock uint64,
) (uint64, uint64, bool, error) {
	lowest, hasLowest, err := firstBlockNumber(database.GetRange(chainID, startBlock, endBlock))
	if err != nil {
		return 0, 0, false, fmt.Errorf("failed to read lowest block: %w", err)
	}
	highest, hasHighest, err := firstBlockNumber(database.GetRangeRev(chainID, startBlock, endBlock))
	if err != nil {
		return 0, 0, false, fmt.Errorf("failed to read highest block: %w", err)
	}
	if !hasLowest || !hasHighest {
		return 0, 0, false, nil
	}
	return lowest, highest, true, nil
}

// firstBlockNumber returns the number of the first block of the given sequence. The second result
// is false if the sequence contains no block.
func firstBlockNumber(blocks iter.Seq2[*blockdb.Block, error]) (uint64, bool, error) {
	for block, err := range blocks {
		if err != nil {
			return 0, false, err
		}
		if block == nil {
			return 0, false, fmt.Errorf("encountered nil block")
		}
		return block.Number, true, nil
	}
	return 0, false, nil
}

// verifyBlocks checks that the hash of each block matches the parent hash of
// its successor, given before it. The gas used by failed bundles is missing from
// the receipts, from which the block hash is computed, so hash mismatches of
// blocks with failed bundles are tolerated with a warning.
func verifyBlocks(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	blocksWithFailedBundles map[uint64]bool,
	onVerifiedBlock func(number uint64),
	logger utils.Logger,
) error {
	first := true
	warned := false
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
			err := verifyBlock(blockHash, block)
			tolerated := errors.Is(err, errBlockHashMismatch) && blocksWithFailedBundles[block.Number]
			if tolerated && !warned {
				warned = true
				logger.Warn("Tolerated hash mismatch of block with failed bundles; further tolerated hash mismatches are not reported", "block", block.Number)
			}
			if err != nil && !tolerated {
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
	gethBlock, err := convert.ConvertToGethBlock(block)
	if err != nil {
		return fmt.Errorf("failed to convert block to Ethereum format: %w", err)
	}

	got := gethBlock.Hash()
	if got != hash {
		return fmt.Errorf("%w: expected %x, got %x", errBlockHashMismatch, hash, got)
	}

	return nil
}

var errBlockHashMismatch = errors.New("block hash mismatch")
