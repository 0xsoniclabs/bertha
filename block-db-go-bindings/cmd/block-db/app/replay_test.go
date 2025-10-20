package app

import (
	"context"
	"encoding/binary"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/0xsoniclabs/blockdb"
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

func TestProgressLogger_ProducesLogMessagesEvery10kSteps(t *testing.T) {
	require := require.New(t)

	logger := startProgressLogger()

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
	require.Regexp(
		`Processing block 10000 from 1970-01-01 01:33:20 @ t= 0:00:[0-9]{2}, 0.00 txs/s, 0.00 MGas/s, [0-9]+.[0-9]{2}x realtime`,
		logger.LogProgress(block10k),
	)
	require.Empty(logger.LogProgress(block15k))
	require.Regexp(
		`Processing block 20000 from 1970-01-01 01:58:20 @ t= 0:00:[0-9]{2}, 0.00 txs/s, 0.00 MGas/s, [0-9]+.[0-9]{2}x realtime`,
		logger.LogProgress(block20k),
	)
}

func TestProgressLogger_ProducesASummary(t *testing.T) {
	require := require.New(t)

	logger := startProgressLogger()

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

func TestRunReplayLoop_CanProcessEmptyBlocks(t *testing.T) {
	state, err := NewState(StateParameters{
		Directory: t.TempDir(),
	})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	stateRoot := state.GetStateRoot()

	// A block history where nothing ever is happening.
	blocks := []*blockdb.Block{
		{Number: 0, StateRoot: stateRoot[:]},
		{Number: 1, StateRoot: stateRoot[:]},
		{Number: 2, StateRoot: stateRoot[:]},
	}

	chain := &stateChainAdapter{
		chainId: 12,
		state:   state,
	}

	iter := newIter(blocks)
	counter := 0
	require.NoError(t, runReplayLoop(t.Context(), iter, chain, Metadata{}, func(block *types.Block) {
		counter++
	}))
	require.Equal(t, len(blocks), counter)
}

func TestRunReplayLoop_CanProcessNonEmptyBlocks(t *testing.T) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
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
				[]*types.Receipt, common.Hash, error,
			) {
				require.Equal(t, ethBlock.NumberU64(), b.NumberU64())
				// No need to check the full block conversion, since this is
				// covered by the Converter's unit tests. However, we check
				// enough to make sure that the correct block is passed.
				require.Equal(t, len(ethBlock.Transactions()), len(b.Transactions()))
				for i, tx := range ethBlock.Transactions() {
					require.Equal(t, tx.Nonce(), b.Transactions()[i].Nonce())
				}
				return nil, common.BytesToHash(block.StateRoot), nil
			})

		if last != nil {
			call.After(last)
		}
		last = call
	}

	iter := newIter(blocks)
	require.NoError(t, runReplayLoop(t.Context(), iter, chain, Metadata{}, nil))
}

func TestRunReplayLoop_FailsOnFailedBlockRetrieval(t *testing.T) {
	injectedError := fmt.Errorf("injected error")
	blocks := func(yield func(*blockdb.Block, error) bool) {
		yield(nil, injectedError)
	}
	require.ErrorIs(t, runReplayLoop(t.Context(), blocks, nil, Metadata{}, nil), injectedError)
}

func TestRunReplayLoop_FailsOnCancelledContext(t *testing.T) {
	ctxt, cancel := context.WithCancel(t.Context())
	cancel()

	blocks := newIter([]*blockdb.Block{
		{Number: 0, StateRoot: types.EmptyRootHash[:]},
	})

	require.ErrorIs(t, runReplayLoop(ctxt, blocks, nil, Metadata{}, nil), context.Canceled)
}

func TestRunReplayLoop_FailsOnBlockConversionError(t *testing.T) {
	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		Transactions: []*blockdb.Transaction{{
			TransactionType: 99_999, // invalid transaction type
		}},
	}})
	require.ErrorContains(t,
		runReplayLoop(ctxt, blocks, nil, Metadata{}, nil),
		"failed to convert block 0",
	)
}

func TestRunReplayLoop_FailsOnBlockApplicationError(t *testing.T) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)

	injectedError := fmt.Errorf("injected error")
	chain.EXPECT().ApplyBlock(gomock.Any(), gomock.Any()).Return(nil, common.Hash{}, injectedError)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{}})
	require.ErrorIs(t,
		runReplayLoop(ctxt, blocks, chain, Metadata{}, nil),
		injectedError,
	)
}

func TestRunReplayLoop_FailsOnWrongReceiptStatus(t *testing.T) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			types.Receipts{{Status: types.ReceiptStatusFailed}},
			common.Hash{},
			nil,
		)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		Receipts: []*blockdb.TransactionReceipt{{
			Status: types.ReceiptStatusSuccessful,
		}},
	}})
	require.ErrorContains(t,
		runReplayLoop(ctxt, blocks, chain, Metadata{}, nil),
		"receipt status mismatch",
	)
}

func TestRunReplayLoop_FailsOnWrongReceiptCumulatedGasUsed(t *testing.T) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			types.Receipts{{
				Status:            types.ReceiptStatusSuccessful,
				CumulativeGasUsed: 100,
			}},
			common.Hash{},
			nil,
		)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		Receipts: []*blockdb.TransactionReceipt{{
			Status: types.ReceiptStatusSuccessful,
		}},
	}})
	require.ErrorContains(t,
		runReplayLoop(ctxt, blocks, chain, Metadata{}, nil),
		"receipt cumulative gas used mismatch",
	)
}

func TestRunReplayLoop_FailsOnIncorrectStateRootHash(t *testing.T) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	chain.EXPECT().
		ApplyBlock(gomock.Any(), gomock.Any()).
		Return(
			nil,
			common.Hash{0x1},
			nil,
		)

	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		StateRoot: common.Hash{0x2}.Bytes(),
	}})
	require.ErrorContains(t,
		runReplayLoop(ctxt, blocks, chain, Metadata{}, nil),
		"state root mismatch",
	)
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
