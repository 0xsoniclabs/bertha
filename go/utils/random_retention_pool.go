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
	"fmt"
	"math/rand/v2"
	"sync"
)

// RandomRetentionPool is a pool with a fixed maximal capacity of items. Once the
// pool reaches its capacity, new entries will replace existing ones at random.
// This results in a probabilistic sampling of items where more recently added
// items are more likely to be retained.
type RandomRetentionPool[T any] struct {
	items []T
	mutex sync.Mutex
}

// NewRandomRetentionPool creates a new RandomRetentionPool with the specified
// capacity.
func NewRandomRetentionPool[T any](capacity int) (*RandomRetentionPool[T], error) {
	if capacity <= 0 {
		return nil, fmt.Errorf("pool capacity must be larger than 0")
	}
	return &RandomRetentionPool[T]{
		items: make([]T, 0, capacity),
	}, nil
}

// Add adds an item to the pool or replaces an existing one if the pool is
// at capacity.
func (p *RandomRetentionPool[T]) Add(item T) {
	p.mutex.Lock()
	defer p.mutex.Unlock()
	if len(p.items) < cap(p.items) {
		p.items = append(p.items, item)
		return
	}
	i := rand.IntN(len(p.items))
	p.items[i] = item
}

// GetRandom returns a random entry from the pool.
func (p *RandomRetentionPool[T]) GetRandom() (T, bool) {
	p.mutex.Lock()
	defer p.mutex.Unlock()
	if len(p.items) == 0 {
		var zero T
		return zero, false
	}
	i := rand.IntN(len(p.items))
	item := p.items[i]
	return item, true
}
