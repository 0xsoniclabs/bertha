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
	"encoding/json"
	"fmt"

	"github.com/ethereum/go-ethereum/common/hexutil"
	"github.com/ethereum/go-ethereum/core/types"
)

// FailedBundles maps block numbers to the failed bundles of the respective
// blocks, sorted by position.
type FailedBundles map[uint64][]FailedBundle

// FailedBundle is the envelope of a bundle whose single tx in its root failed.
// Its txs and receipts are missing from the block, but its state changes are
// part of the chain state.
type FailedBundle struct {
	// Pos is the number of block transactions executed before the bundle.
	Pos uint64
	Tx  *types.Transaction
}

// UnmarshalJSON decodes a JSON list of {"block", "pos", "tx"} entries, where tx
// is the hex-encoded envelope transaction. The entries must be sorted by block
// and position.
func (f *FailedBundles) UnmarshalJSON(data []byte) error {
	var entries []struct {
		Block *uint64       `json:"block"`
		Pos   *uint64       `json:"pos"`
		Tx    hexutil.Bytes `json:"tx"`
	}
	if err := json.Unmarshal(data, &entries); err != nil {
		return err
	}
	failedBundles := FailedBundles{}
	for i, entry := range entries {
		if entry.Block == nil {
			return fmt.Errorf("failed bundle %d has no block", i)
		}
		if entry.Pos == nil {
			return fmt.Errorf("failed bundle %d has no position", i)
		}
		block, pos := *entry.Block, *entry.Pos
		tx := new(types.Transaction)
		if err := tx.UnmarshalBinary(entry.Tx); err != nil {
			return fmt.Errorf("invalid transaction of failed bundle %d at block %d, position %d: %w", i, block, pos, err)
		}
		if i > 0 {
			prevBlock, prevPos := *entries[i-1].Block, *entries[i-1].Pos
			if prevBlock > block || prevBlock == block && prevPos > pos {
				return fmt.Errorf("failed bundle %d at block %d, position %d is not sorted by block and position", i, block, pos)
			}
		}
		failedBundles[block] = append(failedBundles[block], FailedBundle{Pos: pos, Tx: tx})
	}
	*f = failedBundles
	return nil
}
