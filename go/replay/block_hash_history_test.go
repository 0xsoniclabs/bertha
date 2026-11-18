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
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/stretchr/testify/require"
)

func TestBlockHashHistory_Clone_ReturnsNilForNilReceiver(t *testing.T) {
	var h *blockHashHistory
	require.Nil(t, h.Clone())
}

func TestBlockHashHistory_Clone_ProducesIndependentCopy(t *testing.T) {
	original := &blockHashHistory{}
	for i := uint64(0); i < 256; i++ {
		original.SetBlockHash(i, common.BytesToHash([]byte{byte(i)}))
	}

	clone := original.Clone()
	require.Equal(t, *original, *clone)

	// Mutating the clone must not affect the original.
	clone.SetBlockHash(0, common.Hash{0xAA})
	require.Equal(t, common.BytesToHash([]byte{0}), original.GetBlockHash(0))

	// Mutating the original must not affect the clone.
	original.SetBlockHash(1, common.Hash{0xBB})
	require.Equal(t, common.BytesToHash([]byte{1}), clone.GetBlockHash(1))
}

func TestBlockHashHistory_CanSetAndRetrieveHistoricHashes(t *testing.T) {
	history := &blockHashHistory{}
	for _, offset := range []uint64{0, 12, 1234} {
		for i := uint64(0); i < 256; i++ {
			history.SetBlockHash(i+offset, common.BytesToHash([]byte{byte(i + offset)}))
		}
		for i := uint64(0); i < 256; i++ {
			expected := common.BytesToHash([]byte{byte(i + offset)})
			actual := history.GetBlockHash(i + offset)
			require.Equal(t, expected, actual)
		}
	}
}

func TestBlockHashHistory_ProducesHeaderWithCorrectHashes(t *testing.T) {
	history := &blockHashHistory{}
	history.SetBlockHash(12, common.Hash{1, 2, 3})
	history.SetBlockHash(11, common.Hash{4, 5, 6})
	history.SetBlockHash(10, common.Hash{7, 8, 9})

	tests := map[string]struct {
		number     uint64
		hash       common.Hash
		parentHash common.Hash
	}{
		"block 12": {number: 12, hash: common.Hash{1, 2, 3}, parentHash: common.Hash{4, 5, 6}},
		"block 11": {number: 11, hash: common.Hash{4, 5, 6}, parentHash: common.Hash{7, 8, 9}},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			header := history.Header(common.Hash{}, tt.number)
			require.Equal(t, tt.number, header.Number.Uint64())
			require.Equal(t, tt.hash, header.Hash)
			require.Equal(t, tt.parentHash, header.ParentHash)
		})
	}
}
