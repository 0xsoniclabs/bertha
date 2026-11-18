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

package verify

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"math"
	"math/big"
	"slices"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/replay/sonic"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/bundle"
	"github.com/ethereum/go-ethereum/core/types"
)

// CheckBlobTxs checks that neither the stored blocks nor the stored failed
// bundles of the given chain contain a blob transaction, including the
// transactions of (nested) bundles. Every blob transaction found is logged.
// Bundles in blocks are opened from the first stored rules update with the
// Brio upgrade on.
func CheckBlobTxs(
	ctx context.Context,
	databaseDir string,
	chainID uint64,
	logger utils.Logger,
	progressIndicatorFactory utils.ProgressIndicatorFactory,
) (err error) {
	logger.Info("Opening block database", "directory", databaseDir)
	database, err := blockdb.OpenRocksDBForReading(databaseDir)
	if err != nil {
		return fmt.Errorf("failed to open database: %w", err)
	}
	defer func() {
		err = errors.Join(err, database.Close())
	}()

	signer := types.LatestSignerForChainID(new(big.Int).SetUint64(chainID))
	numBlobTxs := 0
	check := func(tx *types.Transaction, location string, brio bool) error {
		paths, err := findBlobTxs(signer, tx, brio)
		for _, path := range paths {
			logger.Error("Found blob transaction", "location", location+path)
		}
		numBlobTxs += len(paths)
		if err != nil {
			return fmt.Errorf("%s: %w", location, err)
		}
		return nil
	}

	failedBundlesData, err := database.GetFailedBundles(chainID)
	if err != nil {
		return fmt.Errorf("failed to get failed bundles: %w", err)
	}
	var failedBundles sonic.FailedBundles
	if failedBundlesData != nil {
		if err := json.Unmarshal(failedBundlesData, &failedBundles); err != nil {
			return fmt.Errorf("failed to parse stored failed bundles: %w", err)
		}
	}
	numFailedBundles := 0
	for _, bundles := range failedBundles {
		numFailedBundles += len(bundles)
	}
	if numFailedBundles > 0 {
		logger.Info("Checking failed bundles for blob transactions",
			"chain_id", chainID,
			"num_failed_bundles", numFailedBundles,
		)
		progressIndicator := progressIndicatorFactory.New(int64(numFailedBundles), "Checking failed bundles")
		for _, number := range slices.Sorted(maps.Keys(failedBundles)) {
			for _, failedBundle := range failedBundles[number] {
				if err := check(failedBundle.Tx, fmt.Sprintf("failed bundle at block %d, position %d", number, failedBundle.Pos), true); err != nil {
					return err
				}
				_ = progressIndicator.Add(1) // update errors are ignored
			}
		}
	} else {
		logger.Warn("No failed bundles found", "chain_id", chainID)
	}

	rulesUpdateHeightsData, err := database.GetRulesUpdateHeights(chainID)
	if err != nil {
		return fmt.Errorf("failed to get rules update heights: %w", err)
	}
	var rulesUpdateHeights []sonic.RulesUpdateHeight
	if rulesUpdateHeightsData != nil {
		if err := json.Unmarshal(rulesUpdateHeightsData, &rulesUpdateHeights); err != nil {
			return fmt.Errorf("failed to parse stored rules update heights: %w", err)
		}
	}
	brioBlock := uint64(math.MaxUint64)
	for _, update := range rulesUpdateHeights {
		if update.Rules.Upgrades.Brio {
			brioBlock = min(brioBlock, update.Block)
		}
	}
	if brioBlock == math.MaxUint64 {
		logger.Warn("No Brio upgrade found, bundles in blocks are not opened", "chain_id", chainID)
	} else {
		logger.Info("Opening bundles in blocks from the Brio upgrade on", "chain_id", chainID, "brio_block", brioBlock)
	}

	startBlock, endBlock, hasBlocks, err := getStoredBlockRange(database, chainID, 0, math.MaxUint64)
	if err != nil {
		return err
	}
	if hasBlocks {
		logger.Info("Checking blocks for blob transactions",
			"chain_id", chainID,
			"start_block", startBlock,
			"end_block", endBlock,
		)
		progressIndicator := progressIndicatorFactory.New(int64(endBlock-startBlock+1), "Checking blocks")
		for block, err := range database.GetRange(chainID, startBlock, endBlock) {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			if err != nil {
				return fmt.Errorf("failed to get block: %w", err)
			}
			if block == nil {
				return fmt.Errorf("encountered nil block")
			}
			for i, blockTx := range block.Transactions {
				tx, err := convert.ToGethTransaction(blockTx)
				if err != nil {
					return fmt.Errorf("failed to convert transaction %d of block %d: %w", i, block.Number, err)
				}
				if err := check(tx, fmt.Sprintf("block %d, tx %d", block.Number, i), block.Number >= brioBlock); err != nil {
					return err
				}
			}
			_ = progressIndicator.Add(1) // update errors are ignored
		}
	} else {
		logger.Warn("No blocks found", "chain_id", chainID)
	}

	if numBlobTxs > 0 {
		return fmt.Errorf("found %d blob transactions", numBlobTxs)
	}
	logger.Info("Found no blob transactions")
	return nil
}

// findBlobTxs returns the paths to the blob transactions among the given
// transaction and the transactions of the bundles it contains, recursively.
// The path of the given transaction itself is empty, the transactions of a
// bundle are visited in the order of their hashes. Like Sonic, it treats the
// transaction as a bundle envelope only if the Brio upgrade is active.
func findBlobTxs(signer types.Signer, tx *types.Transaction, brio bool) ([]string, error) {
	var paths []string
	if tx.Type() == types.BlobTxType {
		paths = append(paths, "")
	}
	if !brio || !bundle.IsEnvelope(tx) {
		return paths, nil
	}
	txBundle, err := bundle.OpenEnvelope(signer, tx)
	if err != nil {
		return paths, fmt.Errorf("failed to open bundle: %w", err)
	}
	byHash := func(a, b *types.Transaction) int { return a.Hash().Cmp(b.Hash()) }
	for _, bundledTx := range slices.SortedFunc(maps.Values(txBundle.Transactions), byHash) {
		found, err := findBlobTxs(signer, bundledTx, true)
		for _, path := range found {
			paths = append(paths, fmt.Sprintf(" > bundled tx %s%s", bundledTx.Hash(), path))
		}
		if err != nil {
			return paths, fmt.Errorf("bundled tx %s: %w", bundledTx.Hash(), err)
		}
	}
	return paths, nil
}
