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

package app

import (
	"bytes"
	"context"
	"encoding/binary"
	"fmt"
	"iter"
	"log/slog"
	"os"
	"path/filepath"
	"testing"
	"testing/synctest"

	"github.com/0xsoniclabs/blockdb"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	"github.com/0xsoniclabs/carmen/go/state"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
	"google.golang.org/protobuf/proto"
)

func TestReplay_SmallValidDb_DoesNotReportIssues(t *testing.T) {
	require := require.New(t)

	chainId := uint64(123)

	dir := t.TempDir()
	genesis := filepath.Join(dir, "genesis.json")
	require.NoError(os.WriteFile(genesis, []byte(`{
		"Rules": {
			"NetworkID": 123
		}
	}`), 0644))

	path := filepath.Join(dir, "small-db")
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	db, err := grocksdb.OpenDb(options, path)
	require.NoError(err, "failed to create database")

	writeOptions := grocksdb.NewDefaultWriteOptions()
	for _, block := range createValidBlocks(t, 10_100) {
		key := make([]byte, 16)
		binary.BigEndian.PutUint64(key[:8], chainId)
		binary.BigEndian.PutUint64(key[8:], uint64(block.Number))

		value, err := proto.Marshal(block)
		require.NoError(err, "failed to marshal block")
		require.NoError(db.Put(writeOptions, key, value))
	}
	writeOptions.Destroy()

	db.Close()

	require.NoError(
		getReplayCommand().Run(t.Context(), []string{
			"test",
			"--db", path,
			"--json-genesis", genesis,
		}),
	)

	require.NoError(
		getReplayCommand().Run(t.Context(), []string{
			"test",
			"--db", path,
			"--json-genesis", genesis,
			"--with-archive",
		}),
	)
}

func TestReplay_FailsIfStartBlockIsProvidedWithoutStateDbDir(t *testing.T) {
	require := require.New(t)
	err := getReplayCommand().Run(t.Context(), []string{
		"test",
		"--start-block", "1000",
	})
	require.ErrorContains(
		err,
		"existing state or initial database directory must be specified when starting from a non-genesis block",
	)
}

func TestProgressLogger_ProducesLogMessagesEvery10kSteps(t *testing.T) {
	require := require.New(t)

	logger := startProgressLogger(nil, "", false)

	block0, err := ConvertToGethBlock(&blockdb.Block{
		Number:    0,
		Timestamp: 1000,
	})
	require.NoError(err)
	block10k, err := ConvertToGethBlock(&blockdb.Block{
		Number:    10_000,
		Timestamp: 2000,
	})
	require.NoError(err)
	block15k, err := ConvertToGethBlock(&blockdb.Block{
		Number:    15_000,
		Timestamp: 2000,
	})
	require.NoError(err)
	block20k, err := ConvertToGethBlock(&blockdb.Block{
		Number:    20_000,
		Timestamp: 3500,
	})
	require.NoError(err)

	require.Empty(logger.LogProgress(block0))
	res, err := logger.LogProgress(block10k)
	require.NoError(err)
	require.Regexp(
		`Processing block 10000 from 1970-01-01 [0-9]{2}:33:20 @ t= 0:00:[0-9]{2}, 0.00 txs/s, 0.00 MGas/s, [0-9]+.[0-9]{2}x realtime`,
		res,
	)
	require.Empty(logger.LogProgress(block15k))
	res, err = logger.LogProgress(block20k)
	require.NoError(err)
	require.Regexp(
		`Processing block 20000 from 1970-01-01 [0-9]{2}:58:20 @ t= 0:00:[0-9]{2}, 0.00 txs/s, 0.00 MGas/s, [0-9]+.[0-9]{2}x realtime`,
		res,
	)
}

func TestProgressLogger_PrintsDirSizeIfEnabled(t *testing.T) {
	require := require.New(t)
	ctrl := gomock.NewController(t)
	dbMock := state.NewMockStateDB(ctrl)
	dbMock.EXPECT().Flush().Return(nil).Times(2)
	state := &State{
		blockHashHistory: nil,
		db:               dbMock,
		stateParameter:   StateParameters{},
	}

	dir := t.TempDir()
	liveDir := filepath.Join(dir, "live")
	os.Mkdir(liveDir, 0700)

	filePath := filepath.Join(liveDir, "file1.txt")
	data := make([]byte, 124*1024*1024)
	err := os.WriteFile(filePath, data, 0644)
	require.NoError(err)

	block, err := ConvertToGethBlock(&blockdb.Block{
		Number:       10000,
		Timestamp:    1000,
		Transactions: []*blockdb.Transaction{},
	})
	require.NoError(err)

	logger := startProgressLogger(state, dir, true)
	res, err := logger.LogProgress(block)
	require.NoError(err)

	require.Regexp(
		`Processing block 10000 from 1970-01-01 [0-9]{2}:[0-9]{2}:[0-9]{2} @ t= 0:00:00, 0.00 txs/s, 0.00 MGas/s, [0-9]+.[0-9]{2}x realtime, live DB size: 0.121 GiB, archive DB size: n/a`,
		res,
	)

	archiveDir := filepath.Join(dir, "archive")
	os.Mkdir(archiveDir, 0700)
	filePath = filepath.Join(archiveDir, "file2.txt")
	data = make([]byte, 156*1024*1024)
	err = os.WriteFile(filePath, data, 0644)
	require.NoError(err)

	logger = startProgressLogger(state, dir, true)
	res, err = logger.LogProgress(block)
	require.NoError(err)

	require.Regexp(
		`Processing block 10000 from 1970-01-01 [0-9]{2}:[0-9]{2}:[0-9]{2} @ t= 0:00:00, 0.00 txs/s, 0.00 MGas/s, [0-9]+.[0-9]{2}x realtime, live DB size: 0.121 GiB, archive DB size: 0.152 GiB`,
		res,
	)
}

func TestProgressLogger_ProducesASummary(t *testing.T) {
	require := require.New(t)

	logger := startProgressLogger(nil, "", false)

	block, err := ConvertToGethBlock(&blockdb.Block{
		Number:    0,
		Timestamp: 1000,
		Transactions: []*blockdb.Transaction{
			{TransactionType: types.LegacyTxType, Nonce: 0},
			{TransactionType: types.LegacyTxType, Nonce: 1},
		},
	})
	require.NoError(err)

	require.Empty(logger.LogProgress(block))
	require.Regexp(
		`Replay finished in .*, processed 2 txs \([0-9]+.[0-9]{2} Tx/s\), used 0.000 TGas \([0-9]+.[0-9]{2} MGas/s\), [0-9]+.[0-9]{2}x realtime`,
		logger.GetSummary(),
	)
}

func TestReplayLoop(t *testing.T) {
	runTests := func(t *testing.T, run replayer) {
		tests := map[string]func(*testing.T, replayer){
			"CanProcessEmptyBlocks":                         canProcessEmptyBlocks,
			"CanProcessNonEmptyBlocks":                      canProcessNonEmptyBlocks,
			"FailsOnFailedBlockRetrieval":                   failsOnFailedBlockRetrieval,
			"FailsOnCancelledContext":                       failsOnCancelledContext,
			"FailsOnBlockConversionError":                   failsOnBlockConversionError,
			"FailsOnBlockApplicationError":                  failsOnBlockApplicationError,
			"FailsOnCommitmentComputationError":             failsOnCommitmentComputationError,
			"FailsOnWrongReceiptStatus":                     failsOnWrongReceiptStatus,
			"FailsOnWrongReceiptCumulatedGasUsed":           failsOnWrongReceiptCumulatedGasUsed,
			"FailsOnIncorrectStateRootHash":                 failsOnIncorrectStateRootHash,
			"OverwriteStateRootHash":                        overwriteStateRootHash,
			"SkipStateRootCheckIfNoStateRootCheckFlagIsSet": skipStateRootCheckIfNoStateRootCheckFlagIsSet,
		}
		for name, test := range tests {
			t.Run(name, func(t *testing.T) {
				test(t, run)
			})
		}
	}

	t.Run("Loop", func(t *testing.T) {
		runTests(t, runReplayLoop)
	})

	t.Run("Pipeline", func(t *testing.T) {
		runTests(t, runReplayPipeline)
	})
}

type replayer func(
	ctx context.Context,
	blocks iter.Seq2[*blockdb.Block, error],
	chain Chain,
	metadata Metadata,
	database blockdb.BlockDB,
	replayLoopContext ReplayLoopContext,
	onBlockProcessed func(*types.Block),
) error

func canProcessEmptyBlocks(t *testing.T, run replayer) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	stateRoot, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)

	// A block history where nothing ever is happening.
	blocks := []*blockdb.Block{
		{Number: 0, StateRoot: stateRoot[:]},
		{Number: 1, StateRoot: stateRoot[:]},
		{Number: 2, StateRoot: stateRoot[:]},
	}

	chain := &stateChainAdapter{
		chainId:         12,
		state:           state,
		snapshotHandler: NewSnapshotHandler(0),
	}

	iter := newIter(blocks)
	counter := 0
	require.NoError(t, run(t.Context(), iter, chain, Metadata{}, nil, ReplayLoopContext{}, func(block *types.Block) {
		counter++
	}))
	require.Equal(t, len(blocks), counter)
}

func canProcessNonEmptyBlocks(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	// A block history with a few transactions.
	blocks := []*blockdb.Block{
		{
			Number:    0,
			StateRoot: []byte{0x1},
			Transactions: []*blockdb.Transaction{
				{TransactionType: types.LegacyTxType, Nonce: 0},
				{TransactionType: types.LegacyTxType, Nonce: 1},
			},
		},
		{
			Number:    1,
			StateRoot: []byte{0x2},
			Transactions: []*blockdb.Transaction{
				{TransactionType: types.LegacyTxType, Nonce: 3},
			},
		},
		{
			Number:    2,
			StateRoot: []byte{0x3},
			Transactions: []*blockdb.Transaction{
				{TransactionType: types.LegacyTxType, Nonce: 4},
				{TransactionType: types.LegacyTxType, Nonce: 5},
				{TransactionType: types.LegacyTxType, Nonce: 6},
				{TransactionType: types.LegacyTxType, Nonce: 7},
			},
		},
	}

	// Check that the blocks are processed in order and correctly forwarded.
	var last *gomock.Call
	for _, block := range blocks {
		ethBlock, err := ConvertToGethBlock(block)
		require.NoError(t, err, "failed to convert block %d", block.Number)

		call := chain.EXPECT().
			ApplyBlock(gomock.Any(), gomock.Any()).
			DoAndReturn(func(b *types.Block, _ Metadata) (
				[]*types.Receipt, future.Future[result.Result[common.Hash]], error,
			) {
				require.Equal(t, ethBlock.NumberU64(), b.NumberU64())
				// No need to check the full block conversion, since this is
				// covered by the Converter's unit tests. However, we check
				// enough to make sure that the correct block is passed.
				require.Equal(t, len(ethBlock.Transactions()), len(b.Transactions()))
				for i, tx := range ethBlock.Transactions() {
					require.Equal(t, tx.Nonce(), b.Transactions()[i].Nonce())
				}
				return nil, future.Immediate(result.Ok(common.BytesToHash(block.StateRoot))), nil
			})

		if last != nil {
			call.After(last)
		}
		last = call
	}

	iter := newIter(blocks)
	require.NoError(t, run(t.Context(), iter, chain, Metadata{}, nil, ReplayLoopContext{}, nil))
}

func failsOnFailedBlockRetrieval(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()

	injectedError := fmt.Errorf("injected error")
	blocks := func(yield func(*blockdb.Block, error) bool) {
		yield(nil, injectedError)
	}
	require.ErrorIs(t, run(t.Context(), blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil), injectedError)
}

func failsOnCancelledContext(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()

	ctxt, cancel := context.WithCancel(t.Context())
	cancel()

	blocks := newIter([]*blockdb.Block{
		{Number: 0, StateRoot: types.EmptyRootHash[:]},
	})

	require.ErrorIs(t, run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil), context.Canceled)
}

func failsOnBlockConversionError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		Transactions: []*blockdb.Transaction{{
			TransactionType: 99_999, // invalid transaction type
		}},
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil),
		"failed to convert block 0",
	)
}

func failsOnBlockApplicationError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()

	injectedError := fmt.Errorf("injected error")
	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(nil, future.Immediate(result.Ok(common.Hash{})), injectedError)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{}})
	require.ErrorIs(t,
		run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil),
		injectedError,
	)
}

func failsOnCommitmentComputationError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()

	injectedError := fmt.Errorf("injected error")
	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(nil, future.Immediate(result.Err[common.Hash](injectedError)), nil)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{}})
	require.ErrorIs(t,
		run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil),
		injectedError,
	)
}

func failsOnWrongReceiptStatus(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			types.Receipts{{Status: types.ReceiptStatusFailed}},
			future.Immediate(result.Ok(common.Hash{})),
			nil,
		)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		Receipts: []*blockdb.TransactionReceipt{{
			PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
		}},
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil),
		"receipt status mismatch",
	)
}

func failsOnWrongReceiptCumulatedGasUsed(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			types.Receipts{{
				Status:            types.ReceiptStatusSuccessful,
				CumulativeGasUsed: 100,
			}},
			future.Immediate(result.Ok(common.Hash{})),
			nil,
		)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		Receipts: []*blockdb.TransactionReceipt{{
			PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful},
		}},
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil),
		"receipt cumulative gas used mismatch",
	)
}

func failsOnIncorrectStateRootHash(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			nil,
			future.Immediate(result.Ok(common.Hash{0x1})),
			nil,
		)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		StateRoot: common.Hash{0x2}.Bytes(),
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{}, nil),
		"state root mismatch",
	)
}

func skipStateRootCheckIfNoStateRootCheckFlagIsSet(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			nil,
			future.Immediate(result.Ok(common.Hash{0x1})),
			nil,
		)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		StateRoot: common.Hash{0x2}.Bytes(), // different state root
	}})
	require.NoError(t,
		run(ctxt, blocks, chain, Metadata{}, nil, ReplayLoopContext{
			skipStateRootCheck: true,
		}, nil),
		"state root mismatch",
	)
}

func overwriteStateRootHash(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			nil,
			future.Immediate(result.Ok(common.Hash{0x1})),
			nil,
		)

	blockDB := blockdb.NewMockDBInterface(ctrl)
	blockDB.EXPECT().Update(gomock.Any(),
		&blockdb.Block{
			Number:    0,
			StateRoot: common.Hash{0x1}.Bytes(),
		},
	).Times(1)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		StateRoot: common.Hash{0x2}.Bytes(),
	}})
	require.NoError(t,
		run(ctxt, blocks, chain, Metadata{}, blockDB, ReplayLoopContext{
			overwriteStateRoot: New(true, true),
		}, nil),
		"state root mismatch",
	)
}

func TestRunReplayPipeline_IssueInThirdStageAbortsOtherStages(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		// In this test case, we delay the evaluation of the state root to stall
		// the pipeline in the third stage. This results in stages 1 and 2 being
		// stuck trying to send to full channels. We then inject an error in stage
		// 3 and verify that the other stages are aborted correctly.
		ctrl := gomock.NewController(t)
		chain := NewMockChain(ctrl)
		chain.EXPECT().ChainId().Return(uint64(12)).AnyTimes()
		chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

		promise, firstHash := future.Create[result.Result[common.Hash]]()

		// The first block gets the promise we will only fulfill once all other
		// stages are blocked.
		chain.EXPECT().
			ApplyBlock(gomock.Any(), gomock.Any()).
			Return(nil, firstHash, nil)

		// All other blocks are processed immediately, to fill up the output
		// channels of the first two stages.
		chain.EXPECT().
			ApplyBlock(gomock.Any(), gomock.Any()).
			Return(nil, future.Immediate(result.Ok(common.Hash{0x1})), nil).
			AnyTimes()

		blocks := []*blockdb.Block{}
		for range 10_000 {
			blocks = append(blocks, &blockdb.Block{
				StateRoot: common.Hash{0x0}.Bytes(),
			})
		}

		issue := fmt.Errorf("injected error")

		// Start running the replay pipeline.
		go func() {
			err := runReplayPipeline(t.Context(), newIter(blocks), chain, Metadata{}, nil, ReplayLoopContext{}, nil)
			require.ErrorIs(t, err, issue)
		}()

		// Wait until the first two stages are blocked trying to send to
		// full channels.
		synctest.Wait()

		// Inject an error in the third stage by completing the promise
		// with an error. This error should cause all stages to abort, and the
		// pipeline to terminate. The abort of the stages is verified by the
		// synctest bubble environment not allowing any go-routines to be left
		// running.
		promise.Fulfill(result.Err[common.Hash](issue))
	})
}

func TestStateChainAdapter_ApplyBlock_ForwardsExecutionError(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	chain := &stateChainAdapter{
		chainId: 12,
		state:   state,
	}

	block, err := ConvertToGethBlock(&blockdb.Block{
		Number: 1,
		Transactions: []*blockdb.Transaction{{
			TransactionType: types.LegacyTxType,
			Nonce:           123, // skipped, since nonces are 0 in the DB
		}},
	})
	require.NoError(t, err)

	_, _, err = chain.ApplyBlock(block, Metadata{})
	require.Error(t, err)
	require.ErrorContains(t, err, "failed to apply block")
}

func Test_getExpectedStateRoot_ReturnsCorrectStateRoot(t *testing.T) {
	require := require.New(t)

	chainId := uint64(123)

	dir := t.TempDir()
	state, err := NewState(StateParameters{
		Directory: dir,
	})
	require.NoError(err)
	defer func() {
		require.NoError(state.Close())
	}()

	block := &blockdb.Block{
		Number:          0,
		StateRoot:       common.HexToHash("0xdeadbeef").Bytes(),
		VerkleStateRoot: common.HexToHash("0xabad1dea").Bytes(),
	}

	expectedStateRoot := getExpectedStateRoot(&stateChainAdapter{
		chainId: chainId,
		state:   state,
		schema:  5,
	}, block)
	require.Equal(common.HexToHash("0xdeadbeef"), expectedStateRoot)

	expectedStateRoot = getExpectedStateRoot(&stateChainAdapter{
		chainId: chainId,
		state:   state,
		schema:  6,
	}, block)
	require.Equal(common.HexToHash("0xabad1dea"), expectedStateRoot)
}

func Test_updateStateRoot_UpdatesCorrectStateRoot(t *testing.T) {
	require := require.New(t)

	chainId := uint64(123)

	dir := t.TempDir()
	state, err := NewState(StateParameters{
		Directory: dir,
	})
	require.NoError(err)
	defer func() {
		require.NoError(state.Close())
	}()

	block := &blockdb.Block{
		Number:    0,
		StateRoot: common.HexToHash("0xdeadbeef").Bytes(),
	}

	updateStateRoot(&stateChainAdapter{
		chainId: chainId,
		state:   state,
		schema:  5,
	}, block, common.HexToHash("0xfeedface"))

	require.Equal(common.HexToHash("0xfeedface").Bytes(), block.StateRoot)

	block = &blockdb.Block{
		Number:          0,
		VerkleStateRoot: common.HexToHash("0xabad1dea").Bytes(),
	}

	updateStateRoot(&stateChainAdapter{
		chainId: chainId,
		state:   state,
		schema:  6,
	}, block, common.HexToHash("0xfacefeed"))

	require.Equal(common.HexToHash("0xfacefeed").Bytes(), block.VerkleStateRoot)
}

func Test_checkBlockResults_OverwritesStateRoot(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	chainId := uint64(12)
	oldStateRoot := common.HexToHash("0xdeadbeef")
	newStateRoot := common.HexToHash("0xfeedface")
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(chainId).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	block := &blockdb.Block{
		Number:    0,
		StateRoot: oldStateRoot.Bytes(),
	}

	blockWithUpdatedStateRoot := &blockdb.Block{
		Number:    0,
		StateRoot: newStateRoot.Bytes(),
	}

	blockDb := blockdb.NewMockDBInterface(ctrl)
	blockDb.EXPECT().
		Update(chainId, blockWithUpdatedStateRoot).
		Return(nil)

	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot: New(true, true),
		stateRootNotSet:    false,
	}

	err := checkBlockResults(
		chain,
		block,
		types.Receipts{},
		future.Immediate(result.Ok(newStateRoot)),
		blockDb,
		&replayLoopContext,
	)
	require.NoError(t, err)
}

func Test_checkBlockResults_LogsMessageIfStateRootNotSet(t *testing.T) {
	ctrl := gomock.NewController(t)
	defer ctrl.Finish()

	chainId := uint64(12)
	stateRoot := common.HexToHash("0xfeedface")
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainId().Return(chainId).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	block := &blockdb.Block{
		Number: 0,
	}

	blockDb := blockdb.NewMockDBInterface(ctrl)
	replayLoopContext := ReplayLoopContext{
		overwriteStateRoot: New(false, false),
		stateRootNotSet:    false,
	}

	// Capture log output
	var logBuffer bytes.Buffer
	slog.SetDefault(slog.New(slog.NewTextHandler(&logBuffer, nil)))

	err := checkBlockResults(
		chain,
		block,
		types.Receipts{},
		future.Immediate(result.Ok(stateRoot)),
		blockDb,
		&replayLoopContext,
	)
	require.NoError(t, err)
	require.Contains(t, logBuffer.String(), "No state root set in the block DB. No checks will be performed")

	// Clear log buffer
	logBuffer.Reset()

	replayLoopContext = ReplayLoopContext{
		overwriteStateRoot: New(false, false),
		stateRootNotSet:    true,
	}

	err = checkBlockResults(
		chain,
		block,
		types.Receipts{},
		future.Immediate(result.Ok(stateRoot)),
		blockDb,
		&replayLoopContext,
	)
	require.NoError(t, err)
	require.Empty(t, logBuffer.String())
}

func Test_SnapshotHandler_ShouldCreateSnapshot(t *testing.T) {
	require := require.New(t)

	handler := &SnapshotHandler{
		blockInterval: 1000,
	}

	require.True(handler.ShouldCreateSnapshot(1000))
	require.True(handler.ShouldCreateSnapshot(2000))
	require.False(handler.ShouldCreateSnapshot(1500))
	require.False(handler.ShouldCreateSnapshot(0))
}

func Test_SnapshotHandler_SnapshotCreatesAndRemovesSnapshots(t *testing.T) {
	require := require.New(t)

	dir := t.TempDir()
	state, err := NewState(StateParameters{
		Directory: dir,
	})
	require.NoError(err)
	defer func() {
		require.NoError(state.Close())
	}()

	handler := &SnapshotHandler{
		blockInterval: 1000,
	}

	// Create first snapshot
	newState, err := handler.Snapshot(1000, state)
	require.NoError(err)
	require.NotNil(newState)

	snapshotDir := fmt.Sprintf("%s_snapshot_1000", dir)
	_, err = os.Stat(snapshotDir)
	require.NoError(err, "snapshot directory should exist")

	// Create second snapshot, first should be removed
	newState, err = handler.Snapshot(2000, newState)
	require.NoError(err)
	require.NotNil(newState)

	snapshotDir2 := fmt.Sprintf("%s_snapshot_2000", dir)
	_, err = os.Stat(snapshotDir2)
	require.NoError(err, "second snapshot directory should exist")

	_, err = os.Stat(snapshotDir)
	require.True(os.IsNotExist(err), "first snapshot directory should be removed")
}

func Test_FlagWithConfirmation(t *testing.T) {
	require := require.New(t)

	flag := New(true, false)
	require.True(flag.IsEnabled())
	require.False(flag.IsConfirmed())

	flag.Confirm()
	require.True(flag.IsConfirmed())

	flag.Disable()
	require.False(flag.IsEnabled())
	require.True(flag.IsConfirmed())
}
