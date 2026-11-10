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
	"errors"
	"fmt"
	"iter"
	"math"
	"math/big"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"testing/synctest"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	"github.com/0xsoniclabs/sonic/gossip/blockproc/bundle"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/trie"
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func defaultReplayArgs() ReplayArgs {
	return ReplayArgs{
		Interpreter: "sfvm",
		DBSchema:    5,
		DBVariant:   "go-file",
		EndBlock:    math.MaxUint64,
	}
}

func TestReplay_ReproducesChain(t *testing.T) {
	modes := map[string]func(*ReplayArgs){
		"Loop": func(*ReplayArgs) {},
		"Pipeline": func(args *ReplayArgs) {
			args.UsePipeline = true
		},
		"ArchiveRateEnabled": func(args *ReplayArgs) {
			args.WithArchive = true
			args.ArchiveRate = 1e6
		},
		"ArchiveRateDisabled": func(args *ReplayArgs) {
			args.WithArchive = true
		},
		"Snapshots": func(args *ReplayArgs) {
			args.SnapshotInterval = 7
			args.SnapshotNumToKeep = 2
			args.SnapshotEndBlock = math.MaxUint64
		},
		"GoMemory": func(args *ReplayArgs) {
			args.DBVariant = "go-memory"
		},
		"GoFileFlat": func(args *ReplayArgs) {
			args.DBVariant = "go-file-flat"
		},
	}
	for network, createDb := range blockDbsByNetwork {
		for mode, configure := range modes {
			t.Run(network+"/"+mode, func(t *testing.T) {
				args := defaultReplayArgs()
				args.BlockDBDir, args.JSONGenesisFile = createDb(t)
				args.StateDBDir = filepath.Join(t.TempDir(), "state")
				configure(&args)
				require.NoError(t, Replay(t.Context(), args))
			})
		}
	}
}

func TestReplay_ResumesOnKeptState(t *testing.T) {
	for network, createDb := range blockDbsByNetwork {
		t.Run(network, func(t *testing.T) {
			require := require.New(t)
			args := defaultReplayArgs()
			args.BlockDBDir, args.JSONGenesisFile = createDb(t)
			args.StateDBDir = filepath.Join(t.TempDir(), "state")
			args.KeepDB = true
			args.EndBlock = 25
			require.NoError(Replay(t.Context(), args))

			args.EndBlock = math.MaxUint64
			args.StartBlock = 26
			require.NoError(Replay(t.Context(), args))
		})
	}
}

func TestReplay_Schema4SkipsStateRootCheck(t *testing.T) {
	for network, createDb := range blockDbsByNetwork {
		t.Run(network, func(t *testing.T) {
			args := defaultReplayArgs()
			args.BlockDBDir, args.JSONGenesisFile = createDb(t)
			args.DBSchema = 4
			require.NoError(t, Replay(t.Context(), args))
		})
	}
}

func TestReplay_InvalidFlagCombinationsAreRejected(t *testing.T) {
	cases := map[string]struct {
		args    ReplayArgs
		wantErr string
	}{
		"StartBlockGreaterThanEndBlock": {
			args:    ReplayArgs{StartBlock: 10, EndBlock: 5},
			wantErr: "start block 10 is greater than end block 5",
		},
		"NonGenesisStartWithoutStateOrInitDir": {
			args:    ReplayArgs{StartBlock: 1000, EndBlock: math.MaxUint64},
			wantErr: "when starting from a non-genesis block (block 0), an existing state database or initial database directory must be specified",
		},
		"ArchiveRateWithoutArchive": {
			args:    ReplayArgs{ArchiveRate: 10, EndBlock: math.MaxUint64},
			wantErr: "when using a non-zero archive rate, archive mode must be enabled (pass --with-archive)",
		},
		"SnapshotStartBlockGreaterThanEndBlock": {
			args:    ReplayArgs{EndBlock: math.MaxUint64, SnapshotStartBlock: 100, SnapshotEndBlock: 50},
			wantErr: "snapshot start block 100 is greater than snapshot end block 50",
		},
		"SnapshotIntervalWithZeroNumToKeep": {
			args:    ReplayArgs{EndBlock: math.MaxUint64, SnapshotInterval: 10, SnapshotNumToKeep: 0, SnapshotEndBlock: math.MaxUint64},
			wantErr: "when using a non-zero snapshot interval 10, --snapshot-num-to-keep must be greater than 0",
		},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			err := Replay(t.Context(), tc.args)
			require.ErrorContains(t, err, tc.wantErr)
		})
	}
}

func TestReplay_InvalidArchiveRateIsReportedAsCreationFailure(t *testing.T) {
	args := defaultReplayArgs()
	args.BlockDBDir, args.JSONGenesisFile = createSonicBlockDb(t, nil)
	args.WithArchive = true
	args.ArchiveRate = -1
	require.ErrorContains(t, Replay(t.Context(), args), "failed to create archive verifier")
}

func TestReplay_StateDbAndSnapshotCleanupBehavior(t *testing.T) {
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

			var corrections Corrections
			if tc.invalidReplay {
				// Correct a block after a snapshot to trigger a state root mismatch
				corrections = Corrections{15: {common.HexToAddress("0x01"): {Balance: uint256.NewInt(1)}}}
			}
			stateDBDir := filepath.Join(dir, "state-db")
			args := defaultReplayArgs()
			args.BlockDBDir, args.JSONGenesisFile = createSonicBlockDb(t, corrections)
			args.StateDBDir = stateDBDir
			args.KeepDB = tc.keepDB
			args.EndBlock = 20
			args.SnapshotInterval = 10
			args.SnapshotNumToKeep = 1
			args.SnapshotEndBlock = math.MaxUint64
			args.NoReceiptsCheck = true

			ctx := t.Context()
			if tc.cancelContext {
				var cancel context.CancelFunc
				ctx, cancel = context.WithCancel(ctx)
				cancel()
			}

			err := Replay(ctx, args)

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
	cases := map[string]struct {
		overwriteStateRoot      OverwriteStateRootPolicy
		writeRulesUpdateHeights bool
		requiresWriteAccess     bool
	}{
		"NoOverwriteStateRootAndNoWriteRulesUpdateHeightsRequiresOnlyReadAccess": {
			overwriteStateRoot:      OverwriteStateRootPolicyOff,
			writeRulesUpdateHeights: false,
			requiresWriteAccess:     false,
		},
		"OverwriteStateRootOnRequiresWriteAccess": {
			overwriteStateRoot:      OverwriteStateRootPolicyOn,
			writeRulesUpdateHeights: false,
			requiresWriteAccess:     true,
		},
		"OverwriteStateRootUninitializedRequiresWriteAccess": {
			overwriteStateRoot:      OverwriteStateRootPolicyUninitialized,
			writeRulesUpdateHeights: false,
			requiresWriteAccess:     true,
		},
		"WriteRulesUpdateHeightsRequiresWriteAccess": {
			overwriteStateRoot:      OverwriteStateRootPolicyOff,
			writeRulesUpdateHeights: true,
			requiresWriteAccess:     true,
		},
		"OverwriteStateRootAndWriteRulesUpdateHeightsRequiresWriteAccess": {
			overwriteStateRoot:      OverwriteStateRootPolicyOn,
			writeRulesUpdateHeights: true,
			requiresWriteAccess:     true,
		},
	}

	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			args := defaultReplayArgs()
			args.BlockDBDir, args.JSONGenesisFile = createSonicBlockDb(t, nil)
			args.OverwriteStateRoot = tc.overwriteStateRoot
			args.WriteRulesUpdateHeights = tc.writeRulesUpdateHeights
			args.EndBlock = 20

			for _, readOnly := range []bool{false, true} {
				if readOnly {
					require.NoError(t, os.Chmod(args.BlockDBDir, 0555))
					t.Cleanup(func() {
						// Restore write permissions for cleanup
						_ = os.Chmod(args.BlockDBDir, 0755)
					})
				}

				err := Replay(t.Context(), args)

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
			"StopsOnArchiveVerifierFailure":                 stopsOnArchiveVerifierFailure,
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
	executor HistoryExecutor,
	database blockdb.BlockDB,
	replayLoopContext ReplayLoopContext,
	onBlockProcessed func(*types.Block) error,
	verifier *archiveVerifier,
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
	block1 := &blockdb.Block{Number: 0, StateRoot: stateRoot[:], ExtraData: make([]byte, 12)}
	block1geth, err := convert.ConvertToGethBlock(block1)
	require.NoError(t, err)
	block2 := &blockdb.Block{Number: 1, StateRoot: stateRoot[:], ParentHash: block1geth.Hash().Bytes(), ExtraData: make([]byte, 12)}
	block2geth, err := convert.ConvertToGethBlock(block2)
	require.NoError(t, err)
	block3 := &blockdb.Block{Number: 2, StateRoot: stateRoot[:], ParentHash: block2geth.Hash().Bytes(), ExtraData: make([]byte, 12)}
	blocks := []*blockdb.Block{block1, block2, block3}

	executor := &historyExecutor{
		chainID:          chainID,
		blockExecutor:    &sonicExecutor{chainID: chainID, metadataStore: &BlockDBMetadataStore{}},
		blockHashHistory: &blockHashHistory{},
		state:            state,
		snapshotHandler:  NewSnapshotHandler(0, 0, math.MaxUint64, 1),
	}

	iter := utils.NewIter(blocks)
	counter := 0
	require.NoError(t, run(t.Context(), iter, executor, nil, ReplayLoopContext{}, func(block *types.Block) error {
		counter++
		return nil
	}, nil))
	require.Equal(t, len(blocks), counter)
}

func canProcessNonEmptyBlocks(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	executor.EXPECT().IsMptConformant().Return(true).Times(3)
	executor.EXPECT().GetBlockHash(gomock.Any()).Return(common.Hash{}).Times(2)

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

		call := executor.EXPECT().
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
	require.NoError(t, run(t.Context(), iter, executor, nil, ReplayLoopContext{}, nil, nil))
}

func failsOnFailedBlockRetrieval(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	injectedError := fmt.Errorf("injected error")
	blocks := func(yield func(*blockdb.Block, error) bool) {
		yield(nil, injectedError)
	}
	require.ErrorIs(t, run(t.Context(), blocks, executor, nil, ReplayLoopContext{}, nil, nil), injectedError)
}

func failsOnCancelledContext(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	ctxt, cancel := context.WithCancel(t.Context())
	cancel()

	blocks := utils.NewIter([]*blockdb.Block{
		{Number: 0, StateRoot: types.EmptyRootHash[:]},
	})

	require.ErrorIs(t, run(ctxt, blocks, executor, nil, ReplayLoopContext{}, nil, nil), context.Canceled)
}

func failsOnBlockConversionError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{
		Transactions: []*blockdb.Transaction{{
			TransactionType: 99_999, // invalid transaction type
		}},
	}})
	require.ErrorContains(t,
		run(ctxt, blocks, executor, nil, ReplayLoopContext{}, nil, nil),
		"failed to convert block 0",
	)
}

func failsOnBlockApplicationError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	// The single block has Number=0, so GetBlockHash is never called.

	injectedError := fmt.Errorf("injected error")
	executor.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(nil, future.Immediate(result.Ok(common.Hash{})), injectedError)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{}})
	require.ErrorIs(t,
		run(ctxt, blocks, executor, nil, ReplayLoopContext{}, nil, nil),
		injectedError,
	)
}

func failsOnCommitmentComputationError(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	// The single block has Number=0, so GetBlockHash is never called.

	injectedError := fmt.Errorf("injected error")
	executor.EXPECT().
		ApplyBlock(gomock.Any()).
		Return(nil, future.Immediate(result.Err[common.Hash](injectedError)), nil)

	ctxt := t.Context()
	blocks := utils.NewIter([]*blockdb.Block{{}})
	require.ErrorIs(t,
		run(ctxt, blocks, executor, nil, ReplayLoopContext{}, nil, nil),
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
			executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
			// ChainID is only called by the pipeline (for signer creation),
			// not by the linear loop, so the exact count varies with the runner.
			executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
			// The single block has Number=0, so GetBlockHash is never called.

			executor.EXPECT().
				ApplyBlock(gomock.Any()).
				Return(
					tc.computedReceipts,
					future.Immediate(result.Ok(common.Hash{})),
					nil,
				)

			ctxt := t.Context()
			blocks := utils.NewIter([]*blockdb.Block{{Receipts: tc.blockReceipts}})
			require.ErrorContains(t,
				run(ctxt, blocks, executor, nil, ReplayLoopContext{}, nil, nil),
				tc.expectedError,
			)
		})
	}
}

func failsOnParentHashMismatch(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	executor.EXPECT().IsMptConformant().Return(true)

	hashOfParentBlock := common.Hash{0xAB}
	executor.EXPECT().GetBlockHash(uint64(0)).Return(hashOfParentBlock)

	executor.EXPECT().
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
		run(ctxt, blocks, executor, nil, ReplayLoopContext{}, nil, nil),
		"parent hash mismatch",
	)
}

func failsOnIncorrectStateRootHash(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	executor.EXPECT().IsMptConformant().Return(true)
	// The single block has Number=0, so GetBlockHash is never called.

	executor.EXPECT().
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
		run(ctxt, blocks, executor, nil, ReplayLoopContext{}, nil, nil),
		"state root mismatch",
	)
}

func skipStateRootCheckIfNoStateRootCheckFlagIsSet(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	executor.EXPECT().IsMptConformant().Return(true)
	executor.EXPECT().GetBlockHash(gomock.Any()).Return(common.Hash{})

	executor.EXPECT().
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
		run(ctxt, blocks, executor, nil, ReplayLoopContext{
			skipStateRootCheck: true,
		}, nil, nil),
		"state root mismatch",
	)
}

func skipReceiptsCheckIfNoReceiptsCheckFlagIsSet(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	executor.EXPECT().IsMptConformant().Return(true)
	executor.EXPECT().GetBlockHash(gomock.Any()).Return(common.Hash{})

	executor.EXPECT().
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
	err := run(ctxt, blocks, executor, nil, ReplayLoopContext{skipReceiptsCheck: true}, nil, nil)
	require.NoError(t, err)
}

func stopsOnArchiveVerifierFailure(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is only called by the pipeline (for signer creation), not by
	// the linear loop, so the exact count varies with the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
	executor.EXPECT().IsMptConformant().Return(true).MinTimes(1)
	executor.EXPECT().GetBlockHash(gomock.Any()).Return(common.Hash{}).MinTimes(1)
	executor.EXPECT().CloneBlockHashHistory().Return(nil).MinTimes(1)

	// Create a cancellable context, as the Replay function does.
	ctx, cancel := context.WithCancelCause(t.Context())
	defer cancel(nil)

	injectedErr := fmt.Errorf("injected archive error")
	const cancelAfter = int64(5)

	// Simulate a verifier failure deterministically: after applying a few
	// blocks, cancel the shared context with the injected error, mimicking
	// what the archive verifier does on a real failure.
	var blocksApplied atomic.Int64
	executor.EXPECT().
		ApplyBlock(gomock.Any()).
		DoAndReturn(func(*types.Block) (types.Receipts, future.Future[result.Result[common.Hash]], error) {
			if blocksApplied.Add(1) == cancelAfter {
				cancel(injectedErr)
			}
			return nil, future.Immediate(result.Ok(common.Hash{})), nil
		}).MinTimes(int(cancelAfter))

	// Bare verifier without a dispatcher: the cancellation is driven by the
	// mock above, so we only need submit and close to be no-ops.
	pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
	require.NoError(t, err)
	verifier := &archiveVerifier{
		pool:         pool,
		cancelParent: cancel,
		ctx:          ctx,
		done:         make(chan struct{}),
	}
	defer func() { require.NoError(t, verifier.close()) }()

	// Provide many more blocks than we expect to process before cancellation.
	const totalBlocks = 10_000
	blocks := make([]*blockdb.Block, totalBlocks)
	for i := range blocks {
		blocks[i] = &blockdb.Block{Number: uint64(i), StateRoot: common.Hash{}.Bytes()}
	}

	err = run(ctx, utils.NewIter(blocks), executor, nil, ReplayLoopContext{}, nil, verifier)
	require.ErrorIs(t, err, context.Canceled)
	require.ErrorIs(t, context.Cause(ctx), injectedErr)
	// Only a small number of blocks should have been applied before the
	// cancellation propagates. The pipeline may buffer up to a few hundred
	// additional blocks in its stages; allow a generous upper bound below
	// totalBlocks to keep the test robust.
	require.Less(t, blocksApplied.Load(), int64(1000))
}

func overwriteStateRootHash(t *testing.T, run replayer) {
	ctrl := gomock.NewController(t)
	executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
	// ChainID is called by checkStateRoot via blockDB.Update. The pipeline runner
	// additionally calls it in stage 1 for signer creation, so the exact count
	// depends on the runner.
	executor.EXPECT().ChainID().Return(uint64(12)).MinTimes(1)
	// IsMptConformant is called twice per block: once in getExpectedStateRoot
	// and once in updateStateRoot when overwriting is enabled.
	executor.EXPECT().IsMptConformant().Return(true).Times(2)
	executor.EXPECT().GetBlockHash(gomock.Any()).Return(common.Hash{})

	executor.EXPECT().
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
		run(ctxt, blocks, executor, blockDB, ReplayLoopContext{
			overwriteStateRoot: OverwriteStateRootPolicyOn,
		}, nil, nil),
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
		executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
		// Pipeline-only test: ChainID is called once by stage 1 for signer
		// creation. IsMptConformant is never called: stage 3 processes the
		// first block whose state-root future returns an error before
		// getExpectedStateRoot runs, aborting the pipeline.
		executor.EXPECT().ChainID().Return(uint64(12))
		// All blocks have Number=0, so GetBlockHash is never called.

		promise, firstHash := future.Create[result.Result[common.Hash]]()

		// The first block gets the promise we will only fulfill once all other
		// stages are blocked.
		executor.EXPECT().
			ApplyBlock(gomock.Any()).
			Return(nil, firstHash, nil)

		// All other blocks are processed immediately, to fill up the output
		// channels of the first two stages.
		executor.EXPECT().
			ApplyBlock(gomock.Any()).
			Return(nil, future.Immediate(result.Ok(common.Hash{0x1})), nil).
			MinTimes(1)

		blocks := []*blockdb.Block{}
		for range 10_000 {
			blocks = append(blocks, &blockdb.Block{
				StateRoot: common.Hash{0x0}.Bytes(),
			})
		}

		issue := fmt.Errorf("injected error")

		// Start running the replay pipeline.
		go func() {
			err := runReplayPipeline(t.Context(), utils.NewIter(blocks), executor, nil, ReplayLoopContext{}, nil, nil)
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

// applyBlock applies the block with ApplyBlock and drops the state root.
func applyBlock(executor *historyExecutor, block *types.Block) (types.Receipts, error) {
	receipts, _, err := executor.ApplyBlock(block)
	return receipts, err
}

// applyBlockOnArchive applies the block with ApplyBlockOnArchive and an empty hash history.
func applyBlockOnArchive(executor *historyExecutor, block *types.Block) (types.Receipts, error) {
	return executor.ApplyBlockOnArchive(block, &blockHashHistory{})
}

func TestHistoryExecutor_ApplyBlock_And_ApplyBlockOnArchive_SkipBlockZero(t *testing.T) {
	tests := map[string]struct {
		apply       func(executor *historyExecutor, block *types.Block) (types.Receipts, error)
		recordsHash bool
	}{
		"ApplyBlock":          {apply: applyBlock, recordsHash: true},
		"ApplyBlockOnArchive": {apply: applyBlockOnArchive},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			// The block executor expects no calls.
			executor := &historyExecutor{
				blockExecutor:    NewMockBlockExecutor(gomock.NewController(t)),
				blockHashHistory: &blockHashHistory{},
				state:            state,
			}

			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(0)})
			receipts, err := tt.apply(executor, block)
			require.NoError(t, err)
			require.Nil(t, receipts)
			want := &blockHashHistory{}
			if tt.recordsHash {
				want.SetBlockHash(0, block.Hash())
			}
			require.Equal(t, want, executor.blockHashHistory)
		})
	}
}

func TestHistoryExecutor_ApplyBlock_And_ApplyBlockOnArchive_ForwardToBlockExecutor(t *testing.T) {
	injected := errors.New("injected")
	tests := map[string]struct {
		apply  func(executor *historyExecutor, block *types.Block) (types.Receipts, error)
		expect func(blockExecutor *MockBlockExecutorMockRecorder, executor *historyExecutor, block *types.Block)
	}{
		"ApplyBlock": {
			apply: applyBlock,
			expect: func(blockExecutor *MockBlockExecutorMockRecorder, executor *historyExecutor, block *types.Block) {
				blockExecutor.ExecuteBlock(executor.state, block, executor.blockHashHistory).Return(nil, uint64(0), injected)
			},
		},
		"ApplyBlockOnArchive": {
			apply: applyBlockOnArchive,
			expect: func(blockExecutor *MockBlockExecutorMockRecorder, executor *historyExecutor, block *types.Block) {
				blockExecutor.ExecuteBlockOnArchive(executor.state, block, &blockHashHistory{}).Return(nil, injected)
			},
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			blockExecutor := NewMockBlockExecutor(gomock.NewController(t))
			executor := &historyExecutor{blockExecutor: blockExecutor, blockHashHistory: &blockHashHistory{}, state: &State{}}
			// Distinguishes the executor's hash history from the empty one of applyBlockOnArchive.
			executor.blockHashHistory.SetBlockHash(0, common.Hash{1})
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1)})
			tt.expect(blockExecutor.EXPECT(), executor, block)

			_, err := tt.apply(executor, block)
			require.ErrorIs(t, err, injected)
		})
	}
}

func TestHistoryExecutor_ApplyBlock_CompletesHeaderAndRecordsItsHash(t *testing.T) {
	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()

	executor := &historyExecutor{
		chainID:          123,
		blockHashHistory: &blockHashHistory{},
		blockExecutor:    &sonicExecutor{chainID: 123, metadataStore: &BlockDBMetadataStore{}, interpreter: testInterpreter(t)},
		state:            state,
	}

	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	// Init code that emits a log.
	tx := types.MustSignNewTx(key, types.LatestSignerForChainID(big.NewInt(123)), &types.LegacyTx{
		Gas:  100_000,
		Data: []byte{byte(vm.PUSH1), 0, byte(vm.PUSH1), 0, byte(vm.LOG0)},
	})
	block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1), GasLimit: 100_000, Extra: make([]byte, 12)}).
		WithBody(types.Body{Transactions: types.Transactions{tx}})

	receipts, _, err := executor.ApplyBlock(block)
	require.NoError(t, err)
	require.Len(t, receipts, 1)
	require.Len(t, receipts[0].Logs, 1)
	require.Equal(t, types.MergeBloom(receipts), block.Bloom())
	require.Equal(t, types.DeriveSha(receipts, trie.NewStackTrie(nil)), block.ReceiptHash())
	require.Equal(t, block.Hash(), executor.blockHashHistory.GetBlockHash(1))
}

func TestHistoryExecutor_ApplyBlock_TakesGasUsedFromProcessing(t *testing.T) {
	upgrades := opera.GetBrioUpgrades()
	upgrades.TransactionBundles = true
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	// A failed bundle uses gas without adding a receipt. INVALID uses all gas
	// of the bundle's step.
	signer := types.LatestSignerForChainID(big.NewInt(146))
	envelope := bundle.NewBuilder().
		WithSigner(signer).
		With(bundle.Step(key, &types.AccessListTx{Gas: 100_000, GasPrice: big.NewInt(1), Data: []byte{byte(vm.INVALID)}})).
		Build()
	txBundle, err := bundle.OpenEnvelope(signer, envelope)
	require.NoError(t, err)

	state, err := NewState(StateParameters{Directory: t.TempDir(), Schema: 5})
	require.NoError(t, err)
	defer func() { require.NoError(t, state.Close()) }()
	setBalance(state.db, crypto.PubkeyToAddress(key.PublicKey), big.NewInt(1e18))

	executor := &historyExecutor{
		chainID: 146,
		blockExecutor: &sonicExecutor{chainID: 146, metadataStore: &BlockDBMetadataStore{metadata: Metadata{
			RulesUpdateHeights: []RulesUpdateHeight{{Block: 0, Rules: opera.FakeNetRules(upgrades)}},
			FailedBundles:      FailedBundles{1: {{Pos: 0, Tx: envelope}}},
		}}, interpreter: testInterpreter(t)},
		blockHashHistory: &blockHashHistory{},
		state:            state,
		snapshotHandler:  &SnapshotHandler{},
	}

	block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1), GasLimit: 1_000_000, BaseFee: big.NewInt(1), Extra: make([]byte, 12)})
	receipts, _, err := executor.ApplyBlock(block)
	require.NoError(t, err)
	require.Empty(t, receipts)
	require.Equal(t, txBundle.GetTransactionsInReferencedOrder()[0].Gas(), block.GasUsed())
}

func TestHistoryExecutor_ApplyBlock_And_ApplyBlockOnArchive_UseTheirHashHistory(t *testing.T) {
	// Init code that reverts if BLOCKHASH(1) is zero.
	initCode := common.FromHex("0x600140600a57600080fd5b00")
	tests := map[string]struct {
		apply      func(executor *historyExecutor, block *types.Block) (types.Receipts, error)
		wantStatus uint64
	}{
		"ApplyBlock uses the hashes of the applied blocks": {
			apply:      applyBlock,
			wantStatus: types.ReceiptStatusSuccessful,
		},
		"ApplyBlockOnArchive uses the given hashes": {
			apply: func(executor *historyExecutor, block *types.Block) (types.Receipts, error) {
				hashHistory := &blockHashHistory{}
				hashHistory.SetBlockHash(1, common.Hash{1})
				return executor.ApplyBlockOnArchive(block, hashHistory)
			},
			wantStatus: types.ReceiptStatusSuccessful,
		},
		"ApplyBlockOnArchive ignores the hashes of the applied blocks": {
			apply:      applyBlockOnArchive,
			wantStatus: types.ReceiptStatusFailed,
		},
	}
	for name, tc := range tests {
		t.Run(name, func(t *testing.T) {
			state, err := NewState(StateParameters{Directory: t.TempDir(), WithArchive: true, Schema: 5})
			require.NoError(t, err)
			defer func() { require.NoError(t, state.Close()) }()

			executor := &historyExecutor{
				chainID:          123,
				blockHashHistory: &blockHashHistory{},
				blockExecutor:    &sonicExecutor{chainID: 123, metadataStore: &BlockDBMetadataStore{}, interpreter: testInterpreter(t)},
				state:            state,
			}
			for number := range uint64(2) {
				_, _, err = executor.ApplyBlock(types.NewBlockWithHeader(&types.Header{Number: new(big.Int).SetUint64(number + 1), Extra: make([]byte, 12)}))
				require.NoError(t, err)
			}
			require.NoError(t, executor.FlushState())

			key, err := crypto.GenerateKey()
			require.NoError(t, err)
			tx := types.MustSignNewTx(key, types.LatestSignerForChainID(big.NewInt(123)), &types.LegacyTx{Gas: 100_000, Data: initCode})
			block, err := convert.ConvertToGethBlock(&blockdb.Block{
				Number:       3,
				GasLimit:     100_000,
				ExtraData:    make([]byte, 12),
				Transactions: []*blockdb.Transaction{convert.ToBerthaTransaction(tx)},
			})
			require.NoError(t, err)

			receipts, err := tc.apply(executor, block)
			require.NoError(t, err)
			require.Len(t, receipts, 1)
			require.Equal(t, tc.wantStatus, receipts[0].Status)
		})
	}
}

// synthetic block hash for tests that encodes the block number in the last 8
// bytes so hashes are unique across a 256-block window.
func testBlockHash(n uint64) common.Hash {
	var h common.Hash
	binary.BigEndian.PutUint64(h[24:], n)
	return h
}

func TestSeedBlockHashHistory(t *testing.T) {
	// makeParents constructs count blocks starting at startNum with ParentHash
	// set to testBlockHash(number-1), matching the invariant that block N's
	// ParentHash equals hash(N-1).
	makeParents := func(startNum uint64, count int) []*blockdb.Block {
		blocks := make([]*blockdb.Block, count)
		for i := range blocks {
			n := startNum + uint64(i)
			blocks[i] = &blockdb.Block{
				Number:     n,
				ParentHash: testBlockHash(n - 1).Bytes(),
			}
		}
		return blocks
	}
	// expectedHashes returns the map of block-number → seeded hash for the
	// inclusive range [first, last].
	expectedHashes := func(first, last uint64) map[uint64]common.Hash {
		m := map[uint64]common.Hash{}
		for n := first; n <= last; n++ {
			m[n] = testBlockHash(n)
		}
		return m
	}

	const chainID uint64 = 42

	cases := map[string]struct {
		firstBlockToApply uint64
		dbBlocks          []*blockdb.Block // nil → GetRange must not be called
		wantStart         uint64
		wantEnd           uint64
		wantHashes        map[uint64]common.Hash
		wantErr           bool
	}{
		"NoSeedingWhenStartBlockIsZero": {
			firstBlockToApply: 0,
			dbBlocks:          nil,
			wantHashes:        map[uint64]common.Hash{},
		},
		"SeedsGenesisHashWhenStartingAtBlockOne": {
			firstBlockToApply: 1,
			dbBlocks:          makeParents(1, 1),
			wantStart:         1,
			wantEnd:           1,
			wantHashes:        expectedHashes(0, 0),
		},
		"SeedsPartialWindowForLowStartBlock": {
			firstBlockToApply: 100,
			dbBlocks:          makeParents(1, 100),
			wantStart:         1,
			wantEnd:           100,
			wantHashes:        expectedHashes(0, 99),
		},
		"SeedsPartialWindowFor255thBlock": {
			firstBlockToApply: 255,
			dbBlocks:          makeParents(1, 255),
			wantStart:         1,
			wantEnd:           255,
			wantHashes:        expectedHashes(0, 254),
		},
		"SeedsFullWindowFor256thBlock": {
			firstBlockToApply: 256,
			dbBlocks:          makeParents(1, 256),
			wantStart:         1,
			wantEnd:           256,
			wantHashes:        expectedHashes(0, 255),
		},
		"SeedsFullWindowForLaterBlock": {
			firstBlockToApply: 1000,
			dbBlocks:          makeParents(745, 256),
			wantStart:         745,
			wantEnd:           1000,
			wantHashes:        expectedHashes(744, 999),
		},
		"ErrorsWhenNoBlocksInRange": {
			firstBlockToApply: 100,
			dbBlocks:          []*blockdb.Block{},
			wantStart:         1,
			wantEnd:           100,
			wantErr:           true,
		},
		"ErrorsWhenFewerBlocksThanExpected": {
			firstBlockToApply: 1000,
			dbBlocks:          makeParents(745, 100),
			wantStart:         745,
			wantEnd:           1000,
			wantErr:           true,
		},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := blockdb.NewMockBlockDB(ctrl)
			if tc.dbBlocks != nil {
				db.EXPECT().
					GetRange(chainID, tc.wantStart, tc.wantEnd).
					Return(utils.NewIter(tc.dbBlocks))
			}
			history := &blockHashHistory{}
			err := seedBlockHashHistory(db, chainID, tc.firstBlockToApply, history)
			if tc.wantErr {
				require.Error(t, err)
				return
			}
			require.NoError(t, err)
			for num, expected := range tc.wantHashes {
				require.Equal(t, expected, history.GetBlockHash(num))
			}
		})
	}
}

func TestSeedBlockHashHistory_PropagatesBlockDBError(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := blockdb.NewMockBlockDB(ctrl)

	var errIter iter.Seq2[*blockdb.Block, error] = func(yield func(*blockdb.Block, error) bool) {
		yield(nil, fmt.Errorf("boom"))
	}
	db.EXPECT().
		GetRange(uint64(42), uint64(1), uint64(100)).
		Return(errIter)

	history := &blockHashHistory{}
	err := seedBlockHashHistory(db, 42, 100, history)
	require.ErrorContains(t, err, "boom")
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

	expectedStateRoot := getExpectedStateRoot(&historyExecutor{
		chainID: chainID,
		state:   state,
		schema:  5,
	}, block)
	require.Equal(common.HexToHash("0xdeadbeef"), expectedStateRoot)

	expectedStateRoot = getExpectedStateRoot(&historyExecutor{
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

	updateStateRoot(&historyExecutor{
		chainID: chainID,
		state:   state,
		schema:  5,
	}, block, common.HexToHash("0xfeedface"))

	require.Equal(common.HexToHash("0xfeedface").Bytes(), block.StateRoot)

	block = &blockdb.Block{
		Number:          0,
		VerkleStateRoot: common.HexToHash("0xabad1dea").Bytes(),
	}

	updateStateRoot(&historyExecutor{
		chainID: chainID,
		state:   state,
		schema:  6,
	}, block, common.HexToHash("0xfacefeed"))

	require.Equal(common.HexToHash("0xfacefeed").Bytes(), block.VerkleStateRoot)
}

func TestBlockHashHistory_Clone_ReturnsNilForNilReceiver(t *testing.T) {
	var h *blockHashHistory
	require.Nil(t, h.Clone())
}

func TestBlockHashHistory_Clone_ProducesIndependentCopy(t *testing.T) {
	original := &blockHashHistory{}
	for i := uint64(0); i < 256; i++ {
		original.SetBlockHash(i, common.BytesToHash([]byte{byte(i)}))
	}

	clone := original.Clone()
	require.Equal(t, *original, *clone)

	// Mutating the clone must not affect the original.
	clone.SetBlockHash(0, common.Hash{0xAA})
	require.Equal(t, common.BytesToHash([]byte{0}), original.GetBlockHash(0))

	// Mutating the original must not affect the clone.
	original.SetBlockHash(1, common.Hash{0xBB})
	require.Equal(t, common.BytesToHash([]byte{1}), clone.GetBlockHash(1))
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

func newMockHistoryExecutorWithMaybeSnapshot(ctrl *gomock.Controller) *MockHistoryExecutor {
	executor := NewMockHistoryExecutor(ctrl)
	// MaybeSnapshot is called once per successfully applied block. Several
	// tests using this helper fail before any block is applied, so 0 calls
	// is a legitimate outcome.
	executor.EXPECT().
		MaybeSnapshot(gomock.Any(), gomock.Any()).
		DoAndReturn(func(
			_ uint64,
			sr future.Future[result.Result[common.Hash]],
		) (future.Future[result.Result[common.Hash]], error) {
			return sr, nil
		}).AnyTimes()
	return executor
}
