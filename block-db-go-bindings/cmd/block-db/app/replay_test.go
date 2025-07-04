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

func TestReplay_InSmallValidDb_DoesNotReportIssues(t *testing.T) {
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
		chainId:          12,
		state:            state,
		blockHashHistory: &blockHashHistory{},
	}

	iter := newIter(blocks)
	counter := 0
	require.NoError(t, runReplayLoop(t.Context(), iter, chain, nil, func(block *types.Block) {
		counter++
	}))
	require.Equal(t, len(blocks), counter)
}

func TestRunReplayLoop_FailsOnFailedBlockRetrieval(t *testing.T) {
	injectedError := fmt.Errorf("injected error")
	blocks := func(yield func(*blockdb.Block, error) bool) {
		yield(nil, injectedError)
	}
	require.ErrorIs(t, runReplayLoop(t.Context(), blocks, nil, nil, nil), injectedError)
}

func TestRunReplayLoop_FailsOnCancelledContext(t *testing.T) {
	ctxt, cancel := context.WithCancel(t.Context())
	cancel()

	blocks := newIter([]*blockdb.Block{
		{Number: 0, StateRoot: types.EmptyRootHash[:]},
	})

	require.ErrorIs(t, runReplayLoop(ctxt, blocks, nil, nil, nil), context.Canceled)
}

func TestRunReplayLoop_FailsOnBlockConversionError(t *testing.T) {
	ctxt := t.Context()
	blocks := newIter([]*blockdb.Block{{
		Transactions: []*blockdb.Transaction{{
			TransactionType: 99_999, // invalid transaction type
		}},
	}})
	require.ErrorContains(t,
		runReplayLoop(ctxt, blocks, nil, nil, nil),
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
		runReplayLoop(ctxt, blocks, chain, nil, nil),
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
		runReplayLoop(ctxt, blocks, chain, nil, nil),
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
		runReplayLoop(ctxt, blocks, chain, nil, nil),
		"receipt cumulative gas used mismatch",
	)
}

func TestRunReplayLoop_FailsOnWrongIncorrectStateRootHash(t *testing.T) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)

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
		runReplayLoop(ctxt, blocks, chain, nil, nil),
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
		chainId:          12,
		state:            state,
		blockHashHistory: &blockHashHistory{},
	}

	block, err := ConvertToGethBlock(&blockdb.Block{
		Number: 1,
		Transactions: []*blockdb.Transaction{{
			TransactionType: types.LegacyTxType,
			Nonce:           123, // skipped, since nonces are 0 in the DB
		}},
	})
	require.NoError(t, err)

	_, _, err = chain.ApplyBlock(block, nil)
	require.Error(t, err)
	require.ErrorContains(t, err, "failed to apply block")
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

func TestHistoryAdapter_ProducesHeaderWithCorrectHashes(t *testing.T) {
	history := &blockHashHistory{}

	block := uint64(12)
	current := common.Hash{1, 2, 3}
	parent := common.Hash{4, 5, 6}
	grandParent := common.Hash{7, 8, 9}

	history.SetBlockHash(block, current)
	history.SetBlockHash(block-1, parent)
	history.SetBlockHash(block-2, grandParent)

	adapter := historyAdapter{history: history}

	header := adapter.GetHeader(common.Hash{}, block)
	require.Equal(t, block, header.Number.Uint64())
	require.Equal(t, current, header.Hash)
	require.Equal(t, parent, header.ParentHash)

	header = adapter.GetHeader(common.Hash{}, block-1)
	require.Equal(t, block-1, header.Number.Uint64())
	require.Equal(t, parent, header.Hash)
	require.Equal(t, grandParent, header.ParentHash)
}
