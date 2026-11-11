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
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
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
			ctrl := gomock.NewController(t)
			executor := NewMockArchiveExecutor(ctrl)
			if tc.verifier {
				// The dispatcher goroutine may or may not have called
				// GetArchiveBlockHeight before close() is called; the exact
				// count depends on scheduling, so we only allow it here.
				executor.EXPECT().GetArchiveBlockHeight().Return(uint64(0), true, nil).AnyTimes()
			}

			ctx, cancel := context.WithCancelCause(t.Context())
			defer cancel(nil)

			verifier, err := newArchiveVerifier(ctx, cancel, executor, tc.rate, false, slog.Default())

			if tc.err {
				require.ErrorContains(t, err, "archive rate")
				require.Nil(t, verifier)
			} else {
				require.NoError(t, err)
				if tc.verifier {
					require.NotNil(t, verifier)
					require.NoError(t, verifier.close())
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
	v.submit(&blockdb.Block{Number: 0}, &blockHashHistory{})

	_, ok := pool.GetRandom()
	require.False(t, ok)
}

func TestArchiveVerifier_Submit_AddsNonZeroBlocks(t *testing.T) {
	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)

	v := &archiveVerifier{pool: pool}
	v.submit(&blockdb.Block{Number: 1}, &blockHashHistory{})

	item, ok := pool.GetRandom()
	require.True(t, ok)
	require.Equal(t, uint64(1), item.block.Number)
}

func TestArchiveVerifier_dispatcher_LimitsConcurrency(t *testing.T) {
	var startedTasks atomic.Int64
	finish := make(chan struct{})
	ctrl := gomock.NewController(t)
	executor := NewMockArchiveExecutor(ctrl)
	// The dispatcher spawns at least maxConcurrentVerifications tasks that
	// block on <-finish, and after finish is closed spawns more tasks. The
	// exact total count depends on scheduling.
	executor.EXPECT().GetArchiveBlockHeight().DoAndReturn(func() (uint64, bool, error) {
		startedTasks.Add(1)
		<-finish
		return 0, true, nil
	}).MinTimes(maxConcurrentVerifications)

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
		executor:     executor,
		ctx:          ctx,
		cancelParent: cancel,
		done:         make(chan struct{}),
		interval:     time.Millisecond,
		logger:       slog.Default(),
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

func TestArchiveVerifier_dispatcher_LogsDroppedTicksRateLimited(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctrl := gomock.NewController(t)

		// Count how many drop reports the dispatcher has emitted.
		var warnCount atomic.Int64
		logger := utils.NewMockLogger(ctrl)
		logger.EXPECT().Warn(
			"Archive verification cannot keep up with archive rate",
			"dropped_ticks", gomock.Any(),
		).Do(func(string, ...any) {
			warnCount.Add(1)
		}).AnyTimes()

		// blocking controls whether GetArchiveBlockHeight blocks (causing the
		// concurrency semaphore to fill up and subsequent ticks to be dropped)
		// or returns immediately (allowing the pool to drain and no drops to
		// occur).
		var blocking atomic.Bool
		blocking.Store(true)
		unblock := make(chan struct{})

		executor := NewMockArchiveExecutor(ctrl)
		executor.EXPECT().GetArchiveBlockHeight().DoAndReturn(func() (uint64, bool, error) {
			if blocking.Load() {
				<-unblock
			}
			return 0, true, nil
		}).AnyTimes()

		pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
		require.NoError(t, err)
		for i := range maxConcurrentVerifications * 2 {
			pool.Add(blockWithHashHistory{block: &blockdb.Block{Number: uint64(i)}})
		}

		ctx, cancel := context.WithCancelCause(t.Context())
		defer cancel(nil)

		// The tick interval is chosen so that (a) many ticks fit into a single
		// dropLogInterval and (b) it does not evenly divide dropLogInterval, so
		// main ticks and drop-report ticks never fire at the exact same
		// instant. If they did, select ordering could push a drop into the
		// next reporting window and make the test flaky.
		v := &archiveVerifier{
			pool:         pool,
			executor:     executor,
			logger:       logger,
			ctx:          ctx,
			cancelParent: cancel,
			done:         make(chan struct{}),
			interval:     700 * time.Millisecond,
		}
		v.wg.Go(v.dispatcher)

		// Advance past the first dropLogInterval boundary. The semaphore fills
		// after maxConcurrentVerifications ticks and every subsequent tick is
		// dropped, so exactly one report is expected.
		time.Sleep(dropLogInterval)
		synctest.Wait()
		require.Equal(t, int64(1), warnCount.Load())

		// Another dropLogInterval passes while drops keep happening; a second
		// report is expected.
		time.Sleep(dropLogInterval)
		synctest.Wait()
		require.Equal(t, int64(2), warnCount.Load())

		// Stop the drops: let verifyBlock goroutines complete quickly so no
		// ticks are dropped in the next window. No new report should appear.
		blocking.Store(false)
		close(unblock)
		time.Sleep(dropLogInterval)
		synctest.Wait()
		require.Equal(t, int64(2), warnCount.Load())

		require.NoError(t, v.close())
	})
}

func TestArchiveVerifier_verifyBlock(t *testing.T) {
	// The archive holds blocks up to 1, so block 2 is the first it can re-execute.
	const matchingGasUsed = 21_000
	const differingGasUsed = 999
	tests := map[string]struct {
		archiveEmpty    bool
		blockNumber     uint64
		storedGasUsed   uint64
		noReceiptsCheck bool
		wantApply       bool
		wantErr         string
	}{
		"matching receipts are accepted":                         {blockNumber: 2, storedGasUsed: matchingGasUsed, wantApply: true},
		"differing receipts are rejected":                        {blockNumber: 2, storedGasUsed: differingGasUsed, wantApply: true, wantErr: "receipt check failed for block 2"},
		"differing receipts are accepted without receipts check": {blockNumber: 2, storedGasUsed: differingGasUsed, noReceiptsCheck: true, wantApply: true},
		"block above archive height is skipped":                  {blockNumber: 3},
		"block is skipped while archive is empty":                {archiveEmpty: true, blockNumber: 2},
	}

	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
			require.NoError(t, err)
			pool.Add(blockWithHashHistory{block: &blockdb.Block{
				Number:   tc.blockNumber,
				Receipts: []*blockdb.TransactionReceipt{{CumulativeGasUsed: tc.storedGasUsed}},
			}})

			executor := NewMockArchiveExecutor(gomock.NewController(t))
			executor.EXPECT().GetArchiveBlockHeight().Return(uint64(1), tc.archiveEmpty, nil)
			if tc.wantApply {
				executor.EXPECT().ApplyBlockOnArchive(gomock.Any(), gomock.Any()).Return(types.Receipts{{CumulativeGasUsed: matchingGasUsed}}, nil)
			}

			ctx, cancel := context.WithCancelCause(t.Context())
			defer cancel(nil)
			verifier := &archiveVerifier{
				pool:            pool,
				executor:        executor,
				noReceiptsCheck: tc.noReceiptsCheck,
				ctx:             ctx,
				cancelParent:    cancel,
				done:            make(chan struct{}),
				logger:          slog.Default(),
			}

			verifier.verifyBlock()

			err = verifier.close()
			if tc.wantErr != "" {
				require.ErrorContains(t, err, tc.wantErr)
			} else {
				require.NoError(t, err)
			}
		})
	}
}

func TestArchiveVerifier_verifyBlock_AppliesBlockWithItsHashHistory(t *testing.T) {
	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)
	hashHistory := &blockHashHistory{}
	hashHistory.SetBlockHash(1, common.Hash{1})
	pool.Add(blockWithHashHistory{block: &blockdb.Block{Number: 2}, hashHistory: hashHistory})

	ctrl := gomock.NewController(t)
	executor := NewMockArchiveExecutor(ctrl)
	executor.EXPECT().GetArchiveBlockHeight().Return(uint64(1), false, nil)
	executor.EXPECT().ApplyBlockOnArchive(gomock.Any(), hashHistory)

	ctx, cancel := context.WithCancelCause(t.Context())
	defer cancel(nil)
	verifier := &archiveVerifier{
		pool:         pool,
		executor:     executor,
		ctx:          ctx,
		cancelParent: cancel,
		done:         make(chan struct{}),
		logger:       slog.Default(),
	}

	verifier.verifyBlock()

	require.NoError(t, verifier.close())
}

func TestArchiveVerifier_verifyBlock_CancelsContextOnFailure(t *testing.T) {
	ctx, cancel := context.WithCancelCause(t.Context())
	defer cancel(nil)

	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)

	injectedErr := fmt.Errorf("injected archive error")
	ctrl := gomock.NewController(t)
	executor := NewMockArchiveExecutor(ctrl)
	// Use an archive that always fails to trigger verifier error.
	executor.EXPECT().GetArchiveBlockHeight().Return(uint64(0), false, injectedErr)
	verifier := &archiveVerifier{
		pool:         pool,
		executor:     executor,
		ctx:          ctx,
		cancelParent: cancel,
		done:         make(chan struct{}),
		logger:       slog.Default(),
	}

	// Submit blocks so the pool is non-empty.
	protoBlocks := utils.CreateValidBlocks(t, 10)
	for _, block := range protoBlocks[1:] {
		verifier.submit(block, &blockHashHistory{})
	}

	// Call verifyBlock directly to deterministically trigger cancellation.
	verifier.verifyBlock()

	require.ErrorIs(t, ctx.Err(), context.Canceled)
	require.ErrorIs(t, context.Cause(ctx), injectedErr)

	verifierErr := verifier.close()
	require.ErrorIs(t, verifierErr, injectedErr)
}

func TestArchiveVerifier_close_WaitsForInFlightVerifications(t *testing.T) {
	var startedTasks atomic.Int64
	finish := make(chan struct{})
	ctrl := gomock.NewController(t)
	executor := NewMockArchiveExecutor(ctrl)
	// The dispatcher fills the semaphore with maxConcurrentVerifications
	// blocked tasks before close() is called; after close() the dispatcher
	// stops spawning, so exactly that many calls happen.
	executor.EXPECT().GetArchiveBlockHeight().DoAndReturn(func() (uint64, bool, error) {
		startedTasks.Add(1)
		<-finish
		return 0, true, nil
	}).Times(maxConcurrentVerifications)

	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)
	// verifyBlock will return early after getting the archive height because
	// the archive is reported as empty.
	for i := range maxConcurrentVerifications {
		pool.Add(blockWithHashHistory{block: &blockdb.Block{Number: uint64(i)}})
	}

	ctx, cancel := context.WithCancelCause(t.Context())
	defer cancel(nil)

	v := &archiveVerifier{
		pool:         pool,
		executor:     executor,
		ctx:          ctx,
		cancelParent: cancel,
		done:         make(chan struct{}),
		interval:     time.Millisecond,
		logger:       slog.Default(),
	}
	v.wg.Go(v.dispatcher)

	// Wait until all concurrent verifyBlock slots are occupied.
	require.Eventually(t, func() bool {
		return startedTasks.Load() == int64(maxConcurrentVerifications)
	}, time.Second, time.Millisecond)

	// close() must wait for the in-flight verifyBlock goroutines to finish.
	closed := make(chan error, 1)
	go func() { closed <- v.close() }()

	select {
	case err := <-closed:
		t.Fatalf("close() returned before in-flight verifications finished: %v", err)
	case <-time.After(50 * time.Millisecond):
		// close() is correctly blocked on the running verifyBlock goroutines.
	}

	// Unblock the verifyBlock goroutines and expect close() to complete with no error.
	close(finish)

	select {
	case err := <-closed:
		require.NoError(t, err)
	case <-time.After(time.Second):
		t.Fatal("close() did not complete after unblocking verifications")
	}
}

func TestArchiveVerifier_close_StopsDispatcher(t *testing.T) {
	var startedTasks atomic.Int64
	ctrl := gomock.NewController(t)
	executor := NewMockArchiveExecutor(ctrl)
	// Report an empty archive so verifyBlock returns after the height check
	// without touching the pool or applying blocks. The test waits until at
	// least 3 tasks have started before closing.
	executor.EXPECT().GetArchiveBlockHeight().DoAndReturn(func() (uint64, bool, error) {
		startedTasks.Add(1)
		return 0, true, nil
	}).MinTimes(3)

	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)
	for i := 2; i <= 10; i++ {
		pool.Add(blockWithHashHistory{block: &blockdb.Block{Number: uint64(i)}})
	}

	ctx, cancel := context.WithCancelCause(t.Context())
	defer cancel(nil)

	v := &archiveVerifier{
		pool:         pool,
		executor:     executor,
		ctx:          ctx,
		cancelParent: cancel,
		done:         make(chan struct{}),
		interval:     time.Millisecond,
		logger:       slog.Default(),
	}
	v.wg.Go(v.dispatcher)

	// Wait until several verifyBlock tasks have been spawned by the dispatcher.
	require.Eventually(t, func() bool {
		return startedTasks.Load() >= 3
	}, time.Second, time.Millisecond)

	// Close the verifier; the dispatcher must stop spawning new verifyBlock
	// goroutines.
	require.NoError(t, v.close())

	countAfterClose := startedTasks.Load()
	time.Sleep(50 * time.Millisecond)
	require.Equal(t, countAfterClose, startedTasks.Load())
}
