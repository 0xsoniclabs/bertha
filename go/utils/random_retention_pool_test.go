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

func TestRandomRetentionPool_NewRandomRetentionPool_CreatesPoolWithSpecifiedCapacity(t *testing.T) {
	capacity := 5
	pool, err := NewRandomRetentionPool[int](capacity)
	require.NoError(t, err)
	require.Equal(t, capacity, cap(pool.items))
}

func TestRandomRetentionPool_NewRandomRetentionPool_ReturnsErrorForNonPositiveCapacity(t *testing.T) {
	_, err := NewRandomRetentionPool[int](0)
	require.Error(t, err)

	_, err = NewRandomRetentionPool[int](-1)
	require.Error(t, err)
}

func TestRandomRetentionPool_Add_AddsItemsUntilCapacity(t *testing.T) {
	capacity := 3
	pool := RandomRetentionPool[int]{items: make([]int, 0, capacity)}

	for i := 0; i < capacity; i++ {
		pool.Add(i)
	}

	require.Equal(t, capacity, len(pool.items))
	for i := 0; i < capacity; i++ {
		require.Contains(t, pool.items, i)
	}
}

func TestRandomRetentionPool_Add_ReplacesItemsWhenAtCapacity(t *testing.T) {
	capacity := 3
	pool := RandomRetentionPool[int]{items: make([]int, 0, capacity)}

	for i := 0; i < capacity; i++ {
		pool.Add(i)
	}

	// Submit additional items to trigger replacement
	for i := capacity; i < capacity+5; i++ {
		pool.Add(i)
	}

	require.Equal(t, capacity, len(pool.items)) // The pool should still have the same capacity
	require.Contains(t, pool.items, capacity+4) // The last submitted item should be in the pool
}

func TestRandomRetentionPool_GetRandom_ReturnsItemFromPool(t *testing.T) {
	capacity := 3
	pool := RandomRetentionPool[int]{items: []int{0, 1, 2}}

	for i := 0; i < capacity; i++ {
		pool.Add(i)
	}

	item, ok := pool.GetRandom()
	require.True(t, ok)
	require.Contains(t, pool.items, item)
}

func TestRandomRetentionPool_GetRandom_ReturnsFalseWhenEmpty(t *testing.T) {
	capacity := 3
	pool := RandomRetentionPool[int]{items: make([]int, 0, capacity)}

	item, ok := pool.GetRandom()
	require.False(t, ok)
	require.Equal(t, 0, item) // zero value for int
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
