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
	"encoding/binary"
	"fmt"
	"iter"
	"math"
	"math/big"
	"os"
	"path/filepath"
	"testing"
	"testing/synctest"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	"github.com/0xsoniclabs/sonic/evmcore/core_types"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/drivercall"
	"github.com/0xsoniclabs/sonic/opera/contracts/driver/driverpos"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
	"google.golang.org/protobuf/proto"
)

func TestReplay_SmallValidDb_DoesNotReportIssues(t *testing.T) {
	require := require.New(t)

	chainID := uint64(123)

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
	defer options.Destroy()
	db, err := grocksdb.OpenDb(options, path)
	require.NoError(err, "failed to create database")

	writeOptions := grocksdb.NewDefaultWriteOptions()
	defer writeOptions.Destroy()
	version := make([]byte, 8)
	binary.BigEndian.PutUint64(version, blockdb.CurrentVersion)
	require.NoError(db.Put(writeOptions, blockdb.MakeVersionKey(), version), "failed to write database version")

	for _, block := range utils.CreateValidBlocks(t, 10_100) {
		key := blockdb.MakeBlockKey(chainID, uint64(block.Number))

		value, err := proto.Marshal(block)
		require.NoError(err, "failed to marshal block")
		require.NoError(db.Put(writeOptions, key, value))
	}

	db.Close()

	require.NoError(
		Replay(t.Context(), ReplayArgs{
			BlockDBDir:      path,
			JSONGenesisFile: genesis,
			Interpreter:     "sfvm",
			DBSchema:        5,
			DBVariant:       "go-file",
		}),
	)

	require.NoError(
		Replay(t.Context(), ReplayArgs{
			BlockDBDir:      path,
			JSONGenesisFile: genesis,
			WithArchive:     true,
			Interpreter:     "sfvm",
			DBSchema:        5,
			DBVariant:       "go-file",
		}),
	)
}

func TestReplay_FailsIfStartBlockIsProvidedWithoutStateDbDir(t *testing.T) {
	require := require.New(t)

	dir := t.TempDir()
	genesis := filepath.Join(dir, "genesis.json")
	require.NoError(os.WriteFile(genesis, []byte(`{"Rules": {"NetworkID": 123}}`), 0644))

	dbPath := filepath.Join(dir, "block-db")
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	defer options.Destroy()
	db, err := grocksdb.OpenDb(options, dbPath)
	require.NoError(err)

	writeOptions := grocksdb.NewDefaultWriteOptions()
	defer writeOptions.Destroy()
	version := make([]byte, 8)
	binary.BigEndian.PutUint64(version, blockdb.CurrentVersion)
	require.NoError(db.Put(writeOptions, blockdb.MakeVersionKey(), version))

	db.Close()

	err = Replay(t.Context(), ReplayArgs{
		BlockDBDir:      dbPath,
		JSONGenesisFile: genesis,
		StartBlock:      1000,
		Interpreter:     "sfvm",
	})
	require.ErrorContains(
		err,
		"existing state or initial database directory must be specified when starting from a non-genesis block",
	)
}

func TestReplay_StateDbAndSnapshotCleanupBehavior(t *testing.T) {
	chainID := uint64(123)

	cases := map[string]struct {
		keepDB                bool
		invalidReplay         bool
		cancelContext         bool
		expectStateDBDeleted  bool
		expectSnapshotDeleted bool
	}{
		"SuccessfulReplayWithNoKeepDBThenDbAndSnapshotsDeleted": {
			keepDB:                false,
			expectStateDBDeleted:  true,
			expectSnapshotDeleted: true,
		},
		"SuccessfulReplayWithKeepDBThenDbAndSnapshotsPreserved": {
			keepDB:                true,
			expectStateDBDeleted:  false,
			expectSnapshotDeleted: false,
		},
		"CanceledWithNoKeepDBThenDbAndSnapshotsDeleted": {
			keepDB:                false,
			cancelContext:         true,
			expectStateDBDeleted:  true,
			expectSnapshotDeleted: true,
		},
		"CanceledWithKeepDBThenDbPreserved": {
			keepDB:                true,
			cancelContext:         true,
			expectStateDBDeleted:  false,
			expectSnapshotDeleted: true, // snapshots are also preserved, but since the replay is canceled before the first snapshot, there is no snapshot at the end of the test
		},
		"FailedReplayWithNoKeepDBThenDbDeletedButSnapshotsPreserved": {
			keepDB:                false,
			invalidReplay:         true,
			expectStateDBDeleted:  true,
			expectSnapshotDeleted: false,
		},
		"FailedReplayWithKeepDBThenDbAndSnapshotsPreserved": {
			keepDB:                true,
			invalidReplay:         true,
			expectStateDBDeleted:  false,
			expectSnapshotDeleted: false,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()

			// Create block DB with enough blocks for snapshots
			numBlocks := 20
			blocks := utils.CreateValidBlocks(t, numBlocks)
			if tc.invalidReplay {
				// Tamper with a block after a snapshot to trigger a state root mismatch
				blocks[15].StateRoot = common.Hash{0xFF}.Bytes()
			}

			path := filepath.Join(dir, "block-db")
			options := grocksdb.NewDefaultOptions()
			options.SetCreateIfMissing(true)
			defer options.Destroy()
			db, err := grocksdb.OpenDb(options, path)
			require.NoError(t, err)

			writeOptions := grocksdb.NewDefaultWriteOptions()
			defer writeOptions.Destroy()
			version := make([]byte, 8)
			binary.BigEndian.PutUint64(version, blockdb.CurrentVersion)
			require.NoError(t, db.Put(writeOptions, blockdb.MakeVersionKey(), version))

			for _, block := range blocks {
				key := blockdb.MakeBlockKey(chainID, uint64(block.Number))
				value, err := proto.Marshal(block)
				require.NoError(t, err)
				require.NoError(t, db.Put(writeOptions, key, value))
			}
			db.Close()

			genesisPath := filepath.Join(dir, "genesis.json")
			require.NoError(t, os.WriteFile(genesisPath, []byte(fmt.Sprintf(`{"Rules": {"NetworkID": %d}}`, chainID)), 0644))
			stateDBDir := filepath.Join(dir, "state-db")

			ctx := t.Context()
			if tc.cancelContext {
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			}

			err = Replay(ctx, ReplayArgs{
				BlockDBDir:        path,
				JSONGenesisFile:   genesisPath,
				StateDBDir:        stateDBDir,
				KeepDB:            tc.keepDB,
				Interpreter:       "sfvm",
				DBSchema:          5,
				DBVariant:         "go-file",
				EndBlock:          uint64(numBlocks),
				SnapshotInterval:  10,
				SnapshotNumToKeep: 1,
				SnapshotEndBlock:  math.MaxUint64,
				NoReceiptsCheck:   true,
			})

			if tc.cancelContext {
				require.ErrorIs(t, err, context.Canceled)
			} else if tc.invalidReplay {
				require.ErrorContains(t, err, "state root mismatch")
			} else {
				require.NoError(t, err)
			}

			_, statErr := os.Stat(stateDBDir)
			if tc.expectStateDBDeleted {
				require.True(t, os.IsNotExist(statErr), "state DB dir should be removed")
			} else {
				require.NoError(t, statErr, "state DB dir should still exist")
			}

			// Check snapshot directories (pattern: stateDBDir_snapshot_*)
			matches, err := filepath.Glob(stateDBDir + "_snapshot_*")
			require.NoError(t, err)
			if tc.expectSnapshotDeleted {
				require.Empty(t, matches, "snapshot dirs should be removed")
			} else {
				require.NotEmpty(t, matches, "snapshot dirs should still exist")
			}
		})
	}
}

func TestReplay_BlockDBAccessMode(t *testing.T) {
	chainID := uint64(123)

	cases := map[string]struct {
		overwriteStateRoot      bool
		writeRulesUpdateHeights bool
		requiresWriteAccess     bool
	}{
		"NoOverwriteStateRootAndNoWriteRulesUpdateHeightsRequiresOnlyReadAccess": {
			overwriteStateRoot:      false,
			writeRulesUpdateHeights: false,
			requiresWriteAccess:     false,
		},
		"OverwriteStateRootRequiresWriteAccess": {
			overwriteStateRoot:      true,
			writeRulesUpdateHeights: false,
			requiresWriteAccess:     true,
		},
		"WriteRulesUpdateHeightsRequiresWriteAccess": {
			overwriteStateRoot:      false,
			writeRulesUpdateHeights: true,
			requiresWriteAccess:     true,
		},
		"OverwriteStateRootAndWriteRulesUpdateHeightsRequiresWriteAccess": {
			overwriteStateRoot:      true,
			writeRulesUpdateHeights: true,
			requiresWriteAccess:     true,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()

			numBlocks := 20
			blockDBPath := filepath.Join(dir, "block-db")
			options := grocksdb.NewDefaultOptions()
			options.SetCreateIfMissing(true)
			defer options.Destroy()
			db, err := grocksdb.OpenDb(options, blockDBPath)
			require.NoError(t, err)

			writeOptions := grocksdb.NewDefaultWriteOptions()
			defer writeOptions.Destroy()
			version := make([]byte, 8)
			binary.BigEndian.PutUint64(version, blockdb.CurrentVersion)
			require.NoError(t, db.Put(writeOptions, blockdb.MakeVersionKey(), version))

			for _, block := range utils.CreateValidBlocks(t, numBlocks) {
				key := blockdb.MakeBlockKey(chainID, uint64(block.Number))
				value, err := proto.Marshal(block)
				require.NoError(t, err)
				require.NoError(t, db.Put(writeOptions, key, value))
			}
			db.Close()

			genesis := filepath.Join(dir, "genesis.json")
			require.NoError(t, os.WriteFile(genesis, []byte(fmt.Sprintf(`{"Rules": {"NetworkID": %d}}`, chainID)), 0644))

			for _, readOnly := range []bool{false, true} {
				if readOnly {
					require.NoError(t, os.Chmod(blockDBPath, 0555))
					t.Cleanup(func() {
						// Restore write permissions for cleanup
						_ = os.Chmod(blockDBPath, 0755)
					})
				}

				err := Replay(t.Context(), ReplayArgs{
					BlockDBDir:              blockDBPath,
					JSONGenesisFile:         genesis,
					Interpreter:             "sfvm",
					DBSchema:                5,
					DBVariant:               "go-file",
					OverwriteStateRoot:      tc.overwriteStateRoot,
					WriteRulesUpdateHeights: tc.writeRulesUpdateHeights,
					EndBlock:                uint64(numBlocks),
				})

				if tc.requiresWriteAccess && readOnly {
					require.Error(t, err)
				} else {
					require.NoError(t, err)
				}
			}
		})
	}
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
			"FailsOnDifferentReceipts":                      failsOnDifferentReceipts,
			"FailsOnIncorrectStateRootHash":                 failsOnIncorrectStateRootHash,
			"FailsOnParentHashMismatch":                     failsOnParentHashMismatch,
			"OverwriteStateRootHash":                        overwriteStateRootHash,
			"SkipStateRootCheckIfNoStateRootCheckFlagIsSet": skipStateRootCheckIfNoStateRootCheckFlagIsSet,
			"SkipReceiptsCheckIfNoReceiptsCheckFlagIsSet":   skipReceiptsCheckIfNoReceiptsCheckFlagIsSet,
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
	database blockdb.BlockDB,
	replayLoopContext ReplayLoopContext,
	onBlockProcessed func(*types.Block) error,
) error

func canProcessEmptyBlocks(t *testing.T, run replayer) {
	chainID := uint64(12)
	state, err := NewState(
		StateParameters{Directory: t.TempDir(), Schema: 5},
	)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	stateRoot, err := state.GetStateRoot().Await().Get()
	require.NoError(t, err)

	// A block history where nothing ever is happening.
	block1 := &blockdb.Block{Number: 0, StateRoot: stateRoot[:]}
	block1geth, err := convert.ConvertToGethBlock(block1)
	require.NoError(t, err)
	block2 := &blockdb.Block{Number: 1, StateRoot: stateRoot[:], ParentHash: block1geth.Hash().Bytes()}
	block2geth, err := convert.ConvertToGethBlock(block2)
	require.NoError(t, err)
	block3 := &blockdb.Block{Number: 2, StateRoot: stateRoot[:], ParentHash: block2geth.Hash().Bytes()}
	blocks := []*blockdb.Block{block1, block2, block3}

	chain := &stateChainAdapter{
		chainID:          chainID,
		metadataStore:    &BlockDBMetadataStore{},
		blockHashHistory: &blockHashHistory{},
		state:            state,
		snapshotHandler:  NewSnapshotHandler(0, 0, math.MaxUint64, 1),
	}

	iter := utils.NewIter(blocks)
	counter := 0
	require.NoError(t, run(t.Context(), iter, chain, nil, ReplayLoopContext{}, func(block *types.Block) error {
		counter++
		return nil
	}))
	require.Equal(t, len(blocks), counter)
}

func canProcessNonEmptyBlocks(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()
	chain.EXPECT().GetBlockHashHistory().Return(&blockHashHistory{}).AnyTimes()

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
		ethBlock, err := convert.ConvertToGethBlock(block)
		require.NoError(t, err, "failed to convert block %d", block.Number)

		call := chain.EXPECT().
			ApplyBlock(gomock.Any()).
			DoAndReturn(func(b *types.Block) (
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

	iter := utils.NewIter(blocks)
	require.NoError(t, run(t.Context(), iter, chain, nil, ReplayLoopContext{}, nil))
}

func failsOnFailedBlockRetrieval(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	injectedError := fmt.Errorf("injected error")
	blocks := func(yield func(*blockdb.Block, error) bool) {
		yield(nil, injectedError)
	}
	require.ErrorIs(t, run(t.Context(), blocks, chain, nil, ReplayLoopContext{}, nil), injectedError)
}

func failsOnCancelledContext(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	ctxt, cancel := context.WithCancel(t.Context())
	cancel()

	blocks := utils.NewIter([]*blockdb.Block{
		{Number: 0, StateRoot: types.EmptyRootHash[:]},
	})

	require.ErrorIs(t, run(ctxt, blocks, chain, nil, ReplayLoopContext{}, nil), context.Canceled)
}

func failsOnBlockConversionError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{
		Transactions: []*blockdb.Transaction{{
			TransactionType: 99_999, // invalid transaction type
		}},
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, chain, nil, ReplayLoopContext{}, nil),
		"failed to convert block 0",
	)
}

func failsOnBlockApplicationError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	injectedError := fmt.Errorf("injected error")
	chain.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(nil, future.Immediate(result.Ok(common.Hash{})), injectedError)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{}})
	require.ErrorIs(t,
		run(ctxt, blocks, chain, nil, ReplayLoopContext{}, nil),
		injectedError,
	)
}

func failsOnCommitmentComputationError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	injectedError := fmt.Errorf("injected error")
	chain.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(nil, future.Immediate(result.Err[common.Hash](injectedError)), nil)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{}})
	require.ErrorIs(t,
		run(ctxt, blocks, chain, nil, ReplayLoopContext{}, nil),
		injectedError,
	)
}

func failsOnDifferentReceipts(t *testing.T, run replayer) {
	cases := map[string]struct {
		computedReceipts types.Receipts
		blockReceipts    []*blockdb.TransactionReceipt
		expectedError    string
	}{
		"count mismatch": {
			computedReceipts: types.Receipts{{Status: types.ReceiptStatusFailed}},
			blockReceipts:    []*blockdb.TransactionReceipt{},
			expectedError:    "number of receipts mismatch",
		},
		"status mismatch": {
			computedReceipts: types.Receipts{{Status: types.ReceiptStatusFailed}},
			blockReceipts: []*blockdb.TransactionReceipt{
				{PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful}},
			},
			expectedError: "receipt status mismatch",
		},
		"cumulative gas mismatch": {
			computedReceipts: types.Receipts{
				{Status: types.ReceiptStatusSuccessful, CumulativeGasUsed: 100},
			},
			blockReceipts: []*blockdb.TransactionReceipt{
				{PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful}},
			},
			expectedError: "receipt cumulative gas used mismatch",
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			chain := NewMockChain(ctrl)
			chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

			chain.EXPECT().
				ApplyBlock(gomock.Any()).
				Return(
					tc.computedReceipts,
					future.Immediate(result.Ok(common.Hash{})),
					nil,
				)

			ctxt := t.Context()
			blocks := utils.NewIter([]*blockdb.Block{{Receipts: tc.blockReceipts}})
			require.ErrorContains(t,
				run(ctxt, blocks, chain, nil, ReplayLoopContext{}, nil),
				tc.expectedError,
			)
		})
	}
}

func failsOnParentHashMismatch(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	hashOfParentBlock := common.Hash{0xAB}
	history := blockHashHistory{}
	history.SetBlockHash(0, hashOfParentBlock)
	chain.EXPECT().GetBlockHashHistory().Return(&history)

	chain.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(
			nil,
			future.Immediate(result.Ok(common.Hash{0x1})),
			nil,
		)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{
		Number:     uint64(1),
		ParentHash: common.Hash{0xCD}.Bytes(), // does not match hashOfParentBlock
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, chain, nil, ReplayLoopContext{}, nil),
		"parent hash mismatch",
	)
}

func failsOnIncorrectStateRootHash(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()

	chain.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(
			nil,
			future.Immediate(result.Ok(common.Hash{0x1})),
			nil,
		)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{
		StateRoot: common.Hash{0x2}.Bytes(),
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, chain, nil, ReplayLoopContext{}, nil),
		"state root mismatch",
	)
}

func skipStateRootCheckIfNoStateRootCheckFlagIsSet(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()
	chain.EXPECT().GetBlockHashHistory().Return(&blockHashHistory{})

	chain.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(
			nil,
			future.Immediate(result.Ok(common.Hash{0x1})),
			nil,
		)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{
		Number:    1,
		StateRoot: common.Hash{0x2}.Bytes(), // different state root
	}})
	require.NoError(t,
		run(ctxt, blocks, chain, nil, ReplayLoopContext{
			skipStateRootCheck: true,
		}, nil),
		"state root mismatch",
	)
}

func skipReceiptsCheckIfNoReceiptsCheckFlagIsSet(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()
	chain.EXPECT().GetBlockHashHistory().Return(&blockHashHistory{})

	chain.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(
			types.Receipts{{Status: types.ReceiptStatusFailed}},
			future.Immediate(result.Ok(common.Hash{})),
			nil,
		)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{
		Number: 1,
		Receipts: []*blockdb.TransactionReceipt{{
			PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: types.ReceiptStatusSuccessful}, // different receipt
		}},
	}})
	err := run(ctxt, blocks, chain, nil, ReplayLoopContext{skipReceiptsCheck: true}, nil)
	require.NoError(t, err)
}

func overwriteStateRootHash(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	chain := NewMockChain(ctrl)
	chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	chain.EXPECT().IsMptConformant().Return(true).AnyTimes()
	chain.EXPECT().GetBlockHashHistory().Return(&blockHashHistory{})

	chain.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(
			nil,
			future.Immediate(result.Ok(common.Hash{0x1})),
			nil,
		)

	blockDB := blockdb.NewMockBlockDB(ctrl)
	blockDB.EXPECT().Update(gomock.Any(),
		&blockdb.Block{
			Number:    1,
			StateRoot: common.Hash{0x1}.Bytes(),
		},
	).Times(1)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{
		Number:    1,
		StateRoot: common.Hash{0x2}.Bytes(),
	}})
	require.NoError(t,
		run(ctxt, blocks, chain, blockDB, ReplayLoopContext{
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
		chain.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
		chain.EXPECT().IsMptConformant().Return(true).AnyTimes()
		chain.EXPECT().GetBlockHashHistory().Return(&blockHashHistory{}).AnyTimes()

		promise, firstHash := future.Create[result.Result[common.Hash]]()

		// The first block gets the promise we will only fulfill once all other
		// stages are blocked.
		chain.EXPECT().
			ApplyBlock(gomock.Any()).
			Return(nil, firstHash, nil)

		// All other blocks are processed immediately, to fill up the output
		// channels of the first two stages.
		chain.EXPECT().
			ApplyBlock(gomock.Any()).
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
			err := runReplayPipeline(t.Context(), utils.NewIter(blocks), chain, nil, ReplayLoopContext{}, nil)
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
	chainID := uint64(12)

	state, err := NewState(
		StateParameters{Directory: t.TempDir(), Schema: 5},
	)
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	chain := &stateChainAdapter{
		chainID:          chainID,
		metadataStore:    &BlockDBMetadataStore{},
		blockHashHistory: &blockHashHistory{},
		state:            state,
	}

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number: 1,
		Transactions: []*blockdb.Transaction{{
			TransactionType: types.LegacyTxType,
			Nonce:           123, // skipped, since nonces are 0 in the DB
		}},
	})
	require.NoError(t, err)

	_, _, err = chain.ApplyBlock(block)
	require.Error(t, err)
	require.ErrorContains(t, err, "failed to apply block")
}

func TestStateChainAdapter_ApplyBlock_AppliesUpgrades(t *testing.T) {
	// To see an effect of upgrades, this test uses two different rule sets
	// such that gas costs for a simple transaction with excess gas differ.
	// If the single proposer block formation is enabled, no fees are charged
	// for transactions using too high gas limits. If it is disabled, a 10%
	// excess gas charge is applied.
	noExcessGasCharges := opera.GetAllegroUpgrades()
	noExcessGasCharges.SingleProposerBlockFormation = true

	withExcessGasCharges := opera.GetAllegroUpgrades()
	withExcessGasCharges.SingleProposerBlockFormation = false

	metadataStore := &BlockDBMetadataStore{
		metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{
				{Block: 5, Rules: opera.Rules{Upgrades: noExcessGasCharges}},
				{Block: 10, Rules: opera.Rules{Upgrades: withExcessGasCharges}},
				{Block: 15, Rules: opera.Rules{Upgrades: noExcessGasCharges}},
			},
		},
	}

	chainID := uint64(2)
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() {
		require.NoError(t, state.Close())
	}()

	chain := &stateChainAdapter{
		chainID:          chainID,
		metadataStore:    metadataStore,
		blockHashHistory: &blockHashHistory{},
		state:            state,
		snapshotHandler:  &SnapshotHandler{},
	}

	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	signer := types.LatestSignerForChainID(big.NewInt(int64(chainID)))

	for blockNr := range 20 {
		block, err := convert.ConvertToGethBlock(&blockdb.Block{
			Number:   uint64(blockNr + 1), // skip block 0 (genesis)
			GasLimit: 100_000,
			Transactions: []*blockdb.Transaction{
				convert.ToBerthaTransaction(types.MustSignNewTx(
					key,
					signer,
					&types.LegacyTx{
						Nonce: uint64(blockNr),
						To:    &common.Address{0},
						Gas:   50_000, // extra gas beyond 21_000 to check rule effect
					},
				)),
			},
		})
		require.NoError(t, err)

		receipts, _, err := chain.ApplyBlock(block)
		require.NoError(t, err)
		require.Len(t, receipts, 1)
		require.Equal(t, types.ReceiptStatusSuccessful, receipts[0].Status)

		upgrades := metadataStore.GetUpgradesAtBlock(uint64(blockNr + 1))
		if upgrades.SingleProposerBlockFormation {
			require.Equal(t, uint64(21_000), receipts[0].GasUsed)
		} else {
			require.Greater(t, receipts[0].GasUsed, uint64(21_000))
		}
	}
}

func TestStateChainAdapter_ApplyBlock_CommitsRulesUpdateWhenEncounteringAnEpochSealingTx(t *testing.T) {
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	address := crypto.PubkeyToAddress(key.PublicKey)
	chainID := uint64(2)
	signer := types.LatestSignerForChainID(big.NewInt(int64(chainID)))

	cases := map[string]struct {
		tx                *blockdb.Transaction
		expectCommitRules bool
	}{
		"EpochSealingTx": {
			// An epoch sealing transaction must be an internal transaction to
			// the driver contract with the correct data.
			// Internal transitions must have YParity=0 and R=0 which produces
			// V=0, R=0, which satisfies internaltx.IsInternal.
			tx: convert.ToBerthaTransaction(types.NewTx(
				&types.LegacyTx{
					Gas:      25000,
					To:       &driver.ContractAddress,
					Data:     drivercall.SealEpoch(nil),
					GasPrice: big.NewInt(0),
					V:        big.NewInt(0),
					R:        big.NewInt(0),
				},
			)),
			expectCommitRules: true,
		},
		"NonEpochSealingTx": {
			tx: convert.ToBerthaTransaction(types.MustSignNewTx(key, signer,
				&types.LegacyTx{
					Gas:      21000,
					To:       &common.Address{1},
					GasPrice: big.NewInt(100000),
				},
			)),
			expectCommitRules: false,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			mockMetadataStore := NewMockMetadataStore(ctrl)

			state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
			require.NoError(t, err)
			defer func() {
				require.NoError(t, state.Close())
			}()

			setBalance(state.db, address, big.NewInt(1e18))

			chain := &stateChainAdapter{
				chainID:          chainID,
				metadataStore:    mockMetadataStore,
				blockHashHistory: &blockHashHistory{},
				state:            state,
				snapshotHandler:  &SnapshotHandler{},
			}

			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				GasLimit:     1e18,
				Number:       5,
				Transactions: []*blockdb.Transaction{tc.tx},
			})
			require.NoError(t, err)

			mockMetadataStore.EXPECT().GetUpgradeHeights()
			mockMetadataStore.EXPECT().GetUpgradesAtBlock(uint64(5))
			mockMetadataStore.EXPECT().GetCorrectionsAtBlock(uint64(5))

			if tc.expectCommitRules {
				mockMetadataStore.EXPECT().CommitRules(uint64(5)).Return(nil)
			}

			_, _, err = chain.ApplyBlock(block)
			require.NoError(t, err)
		})
	}
}

func TestStateChainAdapter_GetChainConfigAndUpgrades_ReadsFromMetadataStoreForNonEthereumChains(t *testing.T) {
	cases := map[string]struct {
		chainID    uint64
		isEthereum bool
	}{
		"Ethereum Mainnet": {
			chainID:    1,
			isEthereum: true,
		},
		"Sepolia": {
			chainID:    11155111,
			isEthereum: true,
		},
		"Holesky": {
			chainID:    17000,
			isEthereum: true,
		},
		"Hoodi": {
			chainID:    560048,
			isEthereum: true,
		},
		"Sonic": {
			chainID:    146,
			isEthereum: false,
		},
		"Unknown Chain": {
			chainID:    9999,
			isEthereum: false,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			mockMetadataStore := NewMockMetadataStore(ctrl)

			block, err := convert.ConvertToGethBlock(&blockdb.Block{Number: 1})
			require.NoError(t, err)

			if !tc.isEthereum {
				mockMetadataStore.EXPECT().GetUpgradeHeights()
				mockMetadataStore.EXPECT().GetUpgradesAtBlock(uint64(1))
			}

			getChainConfigAndUpgrades(block, tc.chainID, mockMetadataStore)
		})
	}
}

func Test_getExpectedStateRoot_ReturnsCorrectStateRoot(t *testing.T) {
	require := require.New(t)

	chainID := uint64(123)

	dir := t.TempDir()
	state, err := NewState(StateParameters{
		Directory: dir,
		Schema:    5,
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
		chainID: chainID,
		state:   state,
		schema:  5,
	}, block)
	require.Equal(common.HexToHash("0xdeadbeef"), expectedStateRoot)

	expectedStateRoot = getExpectedStateRoot(&stateChainAdapter{
		chainID: chainID,
		state:   state,
		schema:  6,
	}, block)
	require.Equal(common.HexToHash("0xabad1dea"), expectedStateRoot)
}

func Test_updateStateRoot_UpdatesCorrectStateRoot(t *testing.T) {
	require := require.New(t)

	chainID := uint64(123)

	dir := t.TempDir()
	state, err := NewState(StateParameters{
		Directory: dir,
		Schema:    5,
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
		chainID: chainID,
		state:   state,
		schema:  5,
	}, block, common.HexToHash("0xfeedface"))

	require.Equal(common.HexToHash("0xfeedface").Bytes(), block.StateRoot)

	block = &blockdb.Block{
		Number:          0,
		VerkleStateRoot: common.HexToHash("0xabad1dea").Bytes(),
	}

	updateStateRoot(&stateChainAdapter{
		chainID: chainID,
		state:   state,
		schema:  6,
	}, block, common.HexToHash("0xfacefeed"))

	require.Equal(common.HexToHash("0xfacefeed").Bytes(), block.VerkleStateRoot)
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

	blockNum := uint64(12)
	current := common.Hash{1, 2, 3}
	parent := common.Hash{4, 5, 6}
	grandParent := common.Hash{7, 8, 9}

	history.SetBlockHash(blockNum, current)
	history.SetBlockHash(blockNum-1, parent)
	history.SetBlockHash(blockNum-2, grandParent)

	header := history.Header(common.Hash{}, blockNum)
	require.Equal(t, blockNum, header.Number.Uint64())
	require.Equal(t, current, header.Hash)
	require.Equal(t, parent, header.ParentHash)

	header = history.Header(common.Hash{}, blockNum-1)
	require.Equal(t, blockNum-1, header.Number.Uint64())
	require.Equal(t, parent, header.Hash)
	require.Equal(t, grandParent, header.ParentHash)
}

func TestOnNewLog_CallsPatchRulesWithDecodedDiff(t *testing.T) {
	ctrl := gomock.NewController(t)
	mockStore := NewMockMetadataStore(ctrl)

	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	log := makeUpdateNetworkRulesLog(diff)

	mockStore.EXPECT().PatchRules(uint64(42), diff).Return(nil)
	onNewLog(mockStore, 42, log)
}

func TestOnNewLog_IgnoresLogsWithWrongAddress(t *testing.T) {
	ctrl := gomock.NewController(t)
	mockStore := NewMockMetadataStore(ctrl)

	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	log := makeUpdateNetworkRulesLog(diff)
	log.Address = common.Address{0x42} // wrong address

	mockStore.EXPECT().PatchRules(gomock.Any(), gomock.Any()).Return(nil).Times(0)
	onNewLog(mockStore, 1, log)
}

func TestOnNewLog_IgnoresLogsWithWrongTopic(t *testing.T) {
	ctrl := gomock.NewController(t)
	mockStore := NewMockMetadataStore(ctrl)

	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	log := makeUpdateNetworkRulesLog(diff)
	log.Topics = []common.Hash{{0xFF}} // wrong topic

	mockStore.EXPECT().PatchRules(gomock.Any(), gomock.Any()).Return(nil).Times(0)
	onNewLog(mockStore, 1, log)
}

func TestOnNewLog_IgnoresLogsWithNoTopic(t *testing.T) {
	ctrl := gomock.NewController(t)
	mockStore := NewMockMetadataStore(ctrl)

	diff := []byte(`{"Upgrades":{"Allegro":true}}`)
	log := makeUpdateNetworkRulesLog(diff)
	log.Topics = []common.Hash{} // no topic

	mockStore.EXPECT().PatchRules(gomock.Any(), gomock.Any()).Return(nil).Times(0)
	onNewLog(mockStore, 1, log)
}

func TestOnNewLog_IgnoresLogsWithTooShortData(t *testing.T) {
	ctrl := gomock.NewController(t)
	mockStore := NewMockMetadataStore(ctrl)

	mockStore.EXPECT().PatchRules(gomock.Any(), gomock.Any()).Return(nil).Times(0)
	onNewLog(mockStore, 1, &core_types.Log{
		Address: driver.ContractAddress,
		Topics:  []common.Hash{driverpos.Topics.UpdateNetworkRules},
		Data:    make([]byte, 32), // < 64 bytes, should be ignored
	})
}

// makeUpdateNetworkRulesLog returns a *core_types.Log that looks like an
// UpdateNetworkRules event from the driver contract. The diff is ABI-encoded
// as a single dynamic bytes parameter:
//
//	word 0 (bytes  0–31): offset = 32
//	word 1 (bytes 32–63): length of diff
//	bytes 64+:            diff
func makeUpdateNetworkRulesLog(diff []byte) *core_types.Log {
	data := make([]byte, 64+len(diff))
	new(big.Int).SetInt64(32).FillBytes(data[0:32])
	new(big.Int).SetInt64(int64(len(diff))).FillBytes(data[32:64])
	copy(data[64:], diff)
	return &core_types.Log{
		Address: driver.ContractAddress,
		Topics:  []common.Hash{driverpos.Topics.UpdateNetworkRules},
		Data:    data,
	}
}
