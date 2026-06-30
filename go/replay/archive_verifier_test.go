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
	"math"
	"sync/atomic"
	"testing"
	"time"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/tosca/go/tosca"
	"github.com/stretchr/testify/require"
)

func TestArchiveVerifier_NewArchiveVerifier_ChecksArchiveRate(t *testing.T) {
	tests := map[string]struct {
		rate     float64
		err      bool
		verifier bool
	}{
		"NegativeRate": {rate: -1, err: true},
		"NaNRate":      {rate: math.NaN(), err: true},
		"InfRate":      {rate: math.Inf(1), err: true},
		"TooHighRate":  {rate: math.MaxFloat64, err: true},
		"ZeroRate":     {rate: 0, err: false},
		"ValidRate":    {rate: 1000, err: false, verifier: true},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithCancelCause(t.Context())
			defer cancel(nil)

			verifier, err := newArchiveVerifier(ctx, cancel, func(func(*State) error) error {
				return nil
			}, &BlockDBMetadataStore{}, nil, 123, tc.rate)

			if tc.err {
				require.ErrorContains(t, err, "archive rate")
				require.Nil(t, verifier)
			} else {
				require.NoError(t, err)
				if tc.verifier {
					require.NotNil(t, verifier)
				} else {
					require.Nil(t, verifier)
				}
			}
		})
	}
}

func TestArchiveVerifier_Submit_SkipsBlockZero(t *testing.T) {
	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)

	v := &archiveVerifier{pool: pool}
	v.submit(&blockdb.Block{Number: 0}, blockHashHistory{})

	_, ok := pool.GetRandom()
	require.False(t, ok)
}

func TestArchiveVerifier_Submit_AddsNonZeroBlocks(t *testing.T) {
	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)

	v := &archiveVerifier{pool: pool}
	v.submit(&blockdb.Block{Number: 1}, blockHashHistory{})

	item, ok := pool.GetRandom()
	require.True(t, ok)
	require.Equal(t, uint64(1), item.block.Number)
}

func TestArchiveVerifier_dispatcher_LimitsConcurrency(t *testing.T) {
	var startedTasks atomic.Int64
	finish := make(chan struct{})
	runWithState := func(func(*State) error) error {
		startedTasks.Add(1)
		<-finish
		return nil
	}

	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)

	// Add more blocks than the concurrency limit to the pool.
	for i := 1; i <= maxConcurrentVerifications*2; i++ {
		pool.Add(blockWithHashHistory{block: &blockdb.Block{Number: uint64(i)}})
	}

	ctx, cancel := context.WithCancelCause(t.Context())
	defer cancel(nil)

	v := &archiveVerifier{
		pool:         pool,
		runWithState: runWithState,
		metadata:     &BlockDBMetadataStore{},
		chainID:      123,
		ctx:          ctx,
		cancelParent: cancel,
		done:         make(chan struct{}),
		interval:     time.Millisecond,
	}
	v.wg.Go(v.dispatcher)

	// Give the dispatcher time to spawn goroutines.
	time.Sleep(time.Millisecond * 100)

	// Since the verifyBlock goroutines block on the finish channel, the number
	// of started tasks should be equal to the concurrency limit.
	require.Equal(t, int64(maxConcurrentVerifications), startedTasks.Load())

	// Unblock the verifyBlock goroutines.
	close(finish)

	// Give the dispatcher time to spawn more goroutines.
	time.Sleep(time.Millisecond * 100)
	require.Greater(t, startedTasks.Load(), int64(maxConcurrentVerifications))

	// Close the verifier to stop the dispatcher.
	require.NoError(t, v.close())
}

func TestArchiveVerifier_verifyBlock(t *testing.T) {
	tests := map[string]struct {
		numBlocks   int
		setupPool   func([]*blockdb.Block, *utils.RandomRetentionPool[blockWithHashHistory])
		verifyCalls int
		wantErr     string
	}{
		"VerifiesBlocksSuccessfully": {
			numBlocks: 100,
			setupPool: func(blocks []*blockdb.Block, pool *utils.RandomRetentionPool[blockWithHashHistory]) {
				for _, block := range blocks[1:] {
					pool.Add(blockWithHashHistory{block: block, hashHistory: blockHashHistory{}})
				}
			},
			verifyCalls: 100,
		},
		"DetectsCorruption": {
			numBlocks: 10,
			setupPool: func(blocks []*blockdb.Block, pool *utils.RandomRetentionPool[blockWithHashHistory]) {
				pool.Add(blockWithHashHistory{
					block: &blockdb.Block{
						Number: blocks[5].Number,
						// corrupted receipts
						Receipts: []*blockdb.TransactionReceipt{{
							CumulativeGasUsed: 999,
						}},
					},
					hashHistory: blockHashHistory{},
				})
			},
			verifyCalls: 1,
			wantErr:     "receipt check failed",
		},
		"SkipsBlocksAboveArchiveHeight": {
			numBlocks: 5,
			setupPool: func(_ []*blockdb.Block, pool *utils.RandomRetentionPool[blockWithHashHistory]) {
				pool.Add(blockWithHashHistory{block: &blockdb.Block{
					Number: 9999,
					// corrupted receipts, but should be skipped since it's above the archive height
					Receipts: []*blockdb.TransactionReceipt{{
						CumulativeGasUsed: 999,
					}},
				}})
			},
			verifyCalls: 1,
		},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			interpreter, err := tosca.NewInterpreter("sfvm")
			require.NoError(t, err)

			state, err := NewState(StateParameters{
				Directory:   t.TempDir(),
				WithArchive: true,
				Schema:      5,
				Variant:     "go-file",
			})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			chain := &stateChainAdapter{
				chainID:          123,
				metadataStore:    &BlockDBMetadataStore{},
				blockHashHistory: &blockHashHistory{},
				interpreter:      interpreter,
				state:            state,
				schema:           5,
				snapshotHandler:  NewSnapshotHandler(0, 0, math.MaxUint64, 1),
			}

			protoBlocks := utils.CreateValidBlocks(t, tc.numBlocks)
			for _, block := range protoBlocks {
				gethBlock, err := convert.ConvertToGethBlock(block)
				require.NoError(t, err)
				_, _, err = chain.ApplyBlock(gethBlock)
				require.NoError(t, err)
			}
			// Flush so the archive height reflects the applied blocks.
			require.NoError(t, state.db.Flush())

			ctx, cancel := context.WithCancelCause(t.Context())
			defer cancel(nil)

			pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
			require.NoError(t, err)
			tc.setupPool(protoBlocks, pool)

			verifier := &archiveVerifier{
				pool:         pool,
				runWithState: func(f func(*State) error) error { return f(state) },
				metadata:     &BlockDBMetadataStore{},
				interpreter:  interpreter,
				chainID:      123,
				ctx:          ctx,
				cancelParent: cancel,
				done:         make(chan struct{}),
			}

			for range tc.verifyCalls {
				verifier.verifyBlock()
			}

			err = verifier.close()
			if tc.wantErr != "" {
				require.ErrorContains(t, err, tc.wantErr)
			} else {
				require.NoError(t, err)
			}
		})
	}
}

func TestArchiveVerifier_verifyBlock_CancelsContextOnFailure(t *testing.T) {
	ctx, cancel := context.WithCancelCause(t.Context())
	defer cancel(nil)

	interpreter, err := tosca.NewInterpreter("sfvm")
	require.NoError(t, err)

	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)

	injectedErr := fmt.Errorf("injected archive error")
	verifier := &archiveVerifier{
		pool: pool,
		// Use a runWithState that always fails to trigger verifier error.
		runWithState: func(func(*State) error) error { return injectedErr },
		metadata:     &BlockDBMetadataStore{},
		interpreter:  interpreter,
		chainID:      123,
		ctx:          ctx,
		cancelParent: cancel,
		done:         make(chan struct{}),
	}

	// Submit blocks so the pool is non-empty.
	protoBlocks := utils.CreateValidBlocks(t, 10)
	for _, block := range protoBlocks[1:] {
		verifier.submit(block, blockHashHistory{})
	}

	// Call verifyBlock directly to deterministically trigger cancellation.
	verifier.verifyBlock()

	require.ErrorIs(t, ctx.Err(), context.Canceled)
	require.ErrorIs(t, context.Cause(ctx), injectedErr)

	verifierErr := verifier.close()
	require.ErrorIs(t, verifierErr, injectedErr)
}
