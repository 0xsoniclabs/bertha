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

package replay

import (
	"cmp"
	"encoding/json"
	"fmt"
	"os"
	"slices"

	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
)

// FailedBundles map block numbers to the failed bundles of the respective
// blocks, sorted by position.
type FailedBundles map[uint64][]FailedBundle

// FailedBundle is the envelope of a bundle that failed without being reverted
// (Sonic before Canto). Its transactions are missing from the block, but their
// effects are part of the chain state.
type FailedBundle struct {
	// Pos is the number of block transactions executed before the bundle.
	Pos uint64
	Tx  *types.Transaction
}

// ReadFailedBundlesFromFile reads a JSON list of {"block", "pos", "tx"} entries,
// where tx is the hex-encoded envelope transaction.
func ReadFailedBundlesFromFile(filePath string) (FailedBundles, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return nil, fmt.Errorf("failed to read failed bundles file %q: %w", filePath, err)
	}
	var entries []struct {
		Block uint64        `json:"block"`
		Pos   uint64        `json:"pos"`
		Tx    hexutil.Bytes `json:"tx"`
	}
	if err := json.Unmarshal(data, &entries); err != nil {
		return nil, fmt.Errorf("failed to parse failed bundles: %w", err)
	}
	failedBundles := FailedBundles{}
	for _, entry := range entries {
		tx := new(types.Transaction)
		if err := tx.UnmarshalBinary(entry.Tx); err != nil {
			return nil, fmt.Errorf("invalid transaction of failed bundle in block %d: %w", entry.Block, err)
		}
		failedBundles[entry.Block] = append(failedBundles[entry.Block], FailedBundle{Pos: entry.Pos, Tx: tx})
	}
	for _, bundles := range failedBundles {
		slices.SortStableFunc(bundles, func(a, b FailedBundle) int { return cmp.Compare(a.Pos, b.Pos) })
	}
	return failedBundles, nil
}
