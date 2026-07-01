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
	"context"
	"fmt"
	"log/slog"
	"math"
	"sync"
	"time"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/sonic/evmcore"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/ethereum/go-ethereum/core/types"
)

const (
	poolCapacity = 1024

	// maxConcurrentVerifications caps the number of verifyBlock goroutines
	// running in parallel. When the limit is reached, no more goroutines are
	// spawned until one of the in-flight verifications finishes.
	maxConcurrentVerifications = 8
)

// blockWithHashHistory holds a block and the block hash history for that block.
type blockWithHashHistory struct {
	block       *blockdb.Block
	hashHistory *blockHashHistory
}

// archiveVerifier re-executes old blocks against archive states to verify that
// the archive produces correct results. A dispatcher goroutine spawns a new
// verification goroutine on every tick, up to maxConcurrentVerifications
// in-flight verifications, allowing multiple verifications to run in parallel
// so that slow verifications do not reduce the effective rate. When the
// concurrency limit is reached, ticks are dropped.
type archiveVerifier struct {
	pool *utils.RandomRetentionPool[blockWithHashHistory]

	runWithState func(func(*State) error) error
	metadata     MetadataStore
	interpreter  tosca.Interpreter
	chainID      uint64
	ctx          context.Context
	cancelParent context.CancelCauseFunc
	done         chan struct{}
	wg           sync.WaitGroup
	firstErr     error
	errOnce      sync.Once
	interval     time.Duration
}

func newArchiveVerifier(
	ctx context.Context,
	cancelParent context.CancelCauseFunc,
	runWithState func(func(*State) error) error,
	metadata MetadataStore,
	interpreter tosca.Interpreter,
	chainID uint64,
	archiveRate float64,
) (*archiveVerifier, error) {
	if archiveRate == 0 {
		return nil, nil // Archive verification is disabled.
	}
	if math.IsNaN(archiveRate) || math.IsInf(archiveRate, 0) || archiveRate < 0 {
		return nil, fmt.Errorf("archive rate must be a finite number greater than or equal to zero, got %g", archiveRate)
	}
	interval := time.Duration(float64(time.Second) / archiveRate)
	if interval <= 0 {
		return nil, fmt.Errorf("archive rate %g is too high; resulting interval must be at least 1ns", archiveRate)
	}
	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	if err != nil {
		return nil, fmt.Errorf("failed to create archive verifier pool: %w", err)
	}
	v := &archiveVerifier{
		pool:         pool,
		runWithState: runWithState,
		metadata:     metadata,
		interpreter:  interpreter,
		chainID:      chainID,
		ctx:          ctx,
		cancelParent: cancelParent,
		done:         make(chan struct{}),
		interval:     interval,
	}
	v.wg.Go(v.dispatcher)
	return v, nil
}

// submit adds a block to the archive verifier's pool for later verification.
func (v *archiveVerifier) submit(block *blockdb.Block, blockHashHistory *blockHashHistory) {
	// Block 0 is skipped since it is equivalent with the genesis data import.
	if block.Number > 0 {
		v.pool.Add(blockWithHashHistory{block, blockHashHistory})
	}
}

func (v *archiveVerifier) dispatcher() {
	ticker := time.NewTicker(v.interval)
	defer ticker.Stop()

	// sem bounds the number of concurrent verifyBlock goroutines. When it's
	// full the tick is dropped.
	sem := make(chan struct{}, maxConcurrentVerifications)

	for {
		select {
		case <-ticker.C:
			// If the maximal concurrency is not reached, spawn a new verifyBlock goroutine.
			// Otherwise, drop the tick.
			select {
			case sem <- struct{}{}:
				v.wg.Go(func() {
					defer func() { <-sem }()
					v.verifyBlock()
				})
			default:
			}
		case <-v.done:
			return
		case <-v.ctx.Done():
			return
		}
	}
}

func (v *archiveVerifier) verifyBlock() {
	handleError := func(err error) {
		v.errOnce.Do(func() {
			slog.Error("Archive verification failed", "error", err)
			v.firstErr = err
			v.cancelParent(err)
		})
	}

	var archiveHeight uint64
	var archiveEmpty bool
	err := v.runWithState(func(s *State) error {
		var err error
		archiveHeight, archiveEmpty, err = s.GetArchiveBlockHeight()
		return err
	})
	if err != nil {
		handleError(fmt.Errorf("failed to get archive height: %w", err))
		return
	}
	if archiveEmpty {
		return
	}

	const attempts = 10
	var item blockWithHashHistory
	var found bool
	for i := 0; i < attempts; i++ {
		entry, ok := v.pool.GetRandom()
		if !ok {
			return // no blocks in pool
		}

		if entry.block.Number == 0 || entry.block.Number-1 > archiveHeight {
			continue // block is not in archive yet
		}

		item = entry
		found = true
		break
	}
	if !found {
		return
	}

	block := item.block

	gethBlock, err := convert.ConvertToGethBlock(block)
	if err != nil {
		handleError(fmt.Errorf("failed to convert block %d: %w", block.Number, err))
		return
	}

	chainConfig, upgrades := getChainConfigAndUpgrades(gethBlock, v.chainID, v.metadata)

	processor := evmcore.NewStateProcessorForReplay(
		chainConfig,
		item.hashHistory,
		upgrades,
	)

	corrections := v.metadata.GetCorrectionsAtBlock(block.Number)

	var receipts types.Receipts
	err = v.runWithState(func(s *State) error {
		var applyErr error
		receipts, applyErr = s.ApplyBlock(gethBlock, v.interpreter, processor, upgrades, corrections, chainConfig, nil, true)
		return applyErr
	})
	if err != nil {
		handleError(fmt.Errorf("failed to apply block %d: %w", block.Number, err))
		return
	}

	if err := checkReceipts(item.block, receipts); err != nil {
		handleError(fmt.Errorf("receipt check failed for block %d: %w", block.Number, err))
		return
	}
}

// close signals no more blocks will be submitted, waits for all in-flight
// verifications to finish, and returns the first error encountered.
func (v *archiveVerifier) close() error {
	close(v.done)
	v.wg.Wait()
	return v.firstErr
}
