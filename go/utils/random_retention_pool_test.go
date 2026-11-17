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

package utils

import (
	"testing"

	"github.com/stretchr/testify/require"
)

func TestRandomRetentionPool_NewRandomRetentionPool_CreatesPoolWithPositiveCapacity(t *testing.T) {
	tests := map[string]struct {
		capacity int
		wantErr  string
	}{
		"positive capacity": {capacity: 5},
		"zero capacity":     {capacity: 0, wantErr: "pool capacity must be larger than 0"},
		"negative capacity": {capacity: -1, wantErr: "pool capacity must be larger than 0"},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			pool, err := NewRandomRetentionPool[int](tt.capacity)
			if tt.wantErr != "" {
				require.EqualError(t, err, tt.wantErr)
				return
			}
			require.NoError(t, err)
			require.Equal(t, tt.capacity, cap(pool.items))
		})
	}
}

func TestRandomRetentionPool_Add_KeepsItemsUpToCapacity(t *testing.T) {
	tests := map[string]struct {
		adds         int
		wantContains []int
	}{
		"up to capacity":  {adds: 3, wantContains: []int{0, 1, 2}},
		"beyond capacity": {adds: 8, wantContains: []int{7}},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			pool := RandomRetentionPool[int]{items: make([]int, 0, 3)}
			for i := range tt.adds {
				pool.Add(i)
			}
			require.Len(t, pool.items, 3)
			require.Subset(t, pool.items, tt.wantContains)
		})
	}
}

func TestRandomRetentionPool_GetRandom_ReturnsItemFromPoolIfNotEmpty(t *testing.T) {
	tests := map[string]struct {
		items  []int
		wantOK bool
	}{
		"empty pool":     {items: []int{}},
		"non-empty pool": {items: []int{0, 1, 2}, wantOK: true},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			pool := RandomRetentionPool[int]{items: tt.items}
			item, ok := pool.GetRandom()
			require.Equal(t, tt.wantOK, ok)
			if tt.wantOK {
				require.Contains(t, tt.items, item)
			} else {
				require.Zero(t, item)
			}
		})
	}
}

func TestRandomRetentionPool_RecentItemsAreMoreLikely(t *testing.T) {
	pool, err := NewRandomRetentionPool[int](10)
	require.NoError(t, err)

	for i := 0; i < 100; i++ {
		pool.Add(i)
	}

	smallerCount := 0
	largerCount := 0
	for i := 0; i < 100; i++ {
		item, ok := pool.GetRandom()
		require.True(t, ok)
		if item < 50 {
			smallerCount++
		} else {
			largerCount++
		}
	}

	require.Greater(t, largerCount, 2*smallerCount) // More recent items should be more likely
}
