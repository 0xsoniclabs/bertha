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

package verify

import (
	"context"
	"errors"
	"fmt"
	"iter"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/ethereum/go-ethereum/common"
	"github.com/schollz/progressbar/v3"
)

type VerifyArgs struct {
	DatabaseDir string
	ChainID     uint64
	StartBlock  uint64
	EndBlock    uint64
}

func Verify(ctx context.Context, args VerifyArgs) (err error) {
	fmt.Printf("Opening block database in %q ...\n", args.DatabaseDir)
	database, err := blockdb.OpenRocksDBForReading(args.DatabaseDir)
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	fmt.Printf("Verifying blocks for chain ID %d from block %d to block %d ...\n", args.ChainID, args.StartBlock, args.EndBlock)

	numBlocks := int64(args.EndBlock - args.StartBlock)
	bar := progressbar.Default(numBlocks, "Verifying blocks")

	return verifyBlocks(
		ctx,
		database.GetRangeRev(
			args.ChainID,
			args.StartBlock,
			args.EndBlock,
		),
		func(uint64) {
			_ = bar.Add(1) // Progress bar update errors are ignored
		},
	)
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
