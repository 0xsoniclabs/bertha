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

package utils

import (
	"bytes"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/stretchr/testify/require"
)

// Create a sequence of valid blocks with proper parent-child relationships.
// The first block has no parent, and each subsequent block's parent is the
// previous block.
func CreateValidBlocks(t *testing.T, num int) []*blockdb.Block {
	t.Helper()
	blocks := make([]*blockdb.Block, num)
	lastHash := common.Hash{}
	for i := range num {
		next := &blockdb.Block{
			Number:     uint64(i),
			ParentHash: bytes.Clone(lastHash[:]),
			StateRoot:  types.EmptyRootHash[:],
		}
		blocks[i] = next

		block, err := convert.ConvertToGethBlock(next)
		require.NoError(t, err, "failed to convert block to Geth format")
		lastHash = block.Hash()
	}
	return blocks
}
