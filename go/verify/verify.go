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

	numBlocks := int64(endBlock - startBlock + 1)
	progressIndicator := progressIndicatorFactory.New(numBlocks, "Verifying blocks")

	return verifyBlocks(
		ctx,
		database.GetRangeRev(
			args.ChainID,
			startBlock,
			endBlock,
		),
		func(uint64) {
			_ = progressIndicator.Add(1) // update errors are ignored
		},
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
	gethBlock, err := convert.ConvertToGethBlock(block)
	if err != nil {
		return fmt.Errorf("failed to convert block to Ethereum format: %w", err)
	}

	got := gethBlock.Hash()
	if got != hash {
		return fmt.Errorf("block hash mismatch: expected %x, got %x", hash, got)
	}

	return nil
}
