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
	"github.com/0xsoniclabs/bertha/replay/sonic"
	"github.com/0xsoniclabs/bertha/replay/state"
	"github.com/0xsoniclabs/bertha/utils"
	cc "github.com/0xsoniclabs/carmen/go/common"
	"github.com/0xsoniclabs/carmen/go/common/future"
	"github.com/0xsoniclabs/carmen/go/common/result"
	carmen "github.com/0xsoniclabs/carmen/go/state"
	"github.com/0xsoniclabs/sonic/opera"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/types"
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
		"Schema4SkipsStateRootCheck": func(args *ReplayArgs) {
			args.DBSchema = 4
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

func TestReplay_ReportsUnusableSonicHead(t *testing.T) {
	rules := opera.FakeNetRules(opera.GetSonicUpgrades())
	blocks := utils.CreateSonicBlocks(t, rules, utils.NewGenesisState(t, nil), 3)
	blocks[1].ExtraData = nil
	invalid, err := convert.ConvertToGethBlock(blocks[1])
	require.NoError(t, err)
	blocks[2].ParentHash = invalid.Hash().Bytes()
	blockDbDir, genesisFile := writeBlockDb(t, rules, nil, blocks, nil)
	tests := map[string]struct {
		startBlock uint64
		wantErr    string
	}{
		"preceding block not recorded":            {startBlock: 5, wantErr: "failed to get head header: block 4 is not recorded"},
		"start block not recorded":                {startBlock: 3, wantErr: "failed to get head header: block 3 is not recorded"},
		"preceding block with invalid extra data": {startBlock: 2, wantErr: "failed to open ledger: failed to decode extra data of block 1"},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			args := defaultReplayArgs()
			args.BlockDBDir, args.JSONGenesisFile = blockDbDir, genesisFile
			args.StateDBDir = t.TempDir()
			args.StartBlock = tt.startBlock
			require.ErrorContains(t, Replay(t.Context(), args), tt.wantErr)
		})
	}
}

func TestReplay_ReportsUnrecordedEthereumHead(t *testing.T) {
	args := defaultReplayArgs()
	args.BlockDBDir, args.JSONGenesisFile = createEthereumBlockDb(t)
	args.StateDBDir = t.TempDir()
	args.StartBlock = 101
	require.ErrorContains(t, Replay(t.Context(), args), "failed to get head header: block 100 is not recorded")
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

			var corrections sonic.Corrections
			if tc.invalidReplay {
				// Correct a block after a snapshot to trigger a state root mismatch
				corrections = sonic.Corrections{15: {common.HexToAddress("0x01"): {Balance: uint256.NewInt(1)}}}
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
		"NoOverwriteStateRootAndNoWriteRulesUpdateHeightsRequiresOnlyReadAccess": {
			overwriteStateRoot:      OverwriteStateRootPolicyOff,
			writeRulesUpdateHeights: false,
			requiresWriteAccess:     false,
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
		dbBlocks          []*blockdb.Block // nil without dbErr → GetRange must not be called
		dbErr             error
		wantStart         uint64
		wantEnd           uint64
		wantHashes        map[uint64]common.Hash
		wantErr           string
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
		"PropagatesBlockDBError": {
			firstBlockToApply: 100,
			dbErr:             fmt.Errorf("boom"),
			wantStart:         1,
			wantEnd:           100,
			wantErr:           "failed to read block for hash history seeding: boom",
		},
		"ErrorsWhenNoBlocksInRange": {
			firstBlockToApply: 100,
			dbBlocks:          []*blockdb.Block{},
			wantStart:         1,
			wantEnd:           100,
			wantErr:           "expected 100 blocks in [1, 100] for chain 42, got 0",
		},
		"ErrorsWhenFewerBlocksThanExpected": {
			firstBlockToApply: 1000,
			dbBlocks:          makeParents(745, 100),
			wantStart:         745,
			wantEnd:           1000,
			wantErr:           "expected 256 blocks in [745, 1000] for chain 42, got 100",
		},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := blockdb.NewMockBlockDB(ctrl)
			if tc.dbBlocks != nil || tc.dbErr != nil {
				var blocks iter.Seq2[*blockdb.Block, error]
				if tc.dbErr != nil {
					blocks = func(yield func(*blockdb.Block, error) bool) { yield(nil, tc.dbErr) }
				} else {
					blocks = utils.NewIter(tc.dbBlocks)
				}
				db.EXPECT().GetRange(chainID, tc.wantStart, tc.wantEnd).Return(blocks)
			}
			history := &blockHashHistory{}
			err := seedBlockHashHistory(db, chainID, tc.firstBlockToApply, history)
			if tc.wantErr != "" {
				require.EqualError(t, err, tc.wantErr)
				return
			}
			require.NoError(t, err)
			for num, expected := range tc.wantHashes {
				require.Equal(t, expected, history.GetBlockHash(num))
			}
		})
	}
}

func TestGetRecordedHeader_ReturnsHeaderOfRecordedBlockOrReportsFailure(t *testing.T) {
	block := &blockdb.Block{Number: 4}
	converted, err := convert.ConvertToGethBlock(block)
	require.NoError(t, err)
	tests := map[string]struct {
		recorded *blockdb.Block
		err      error
		want     *types.Header
		wantErr  string
	}{
		"block DB error": {
			err:     fmt.Errorf("boom"),
			wantErr: "failed to read recorded block 4: boom",
		},
		"block not recorded": {
			wantErr: "block 4 is not recorded",
		},
		"invalid block": {
			recorded: &blockdb.Block{Number: 4, Transactions: []*blockdb.Transaction{nil}},
			wantErr:  "failed to convert recorded block 4: failed to convert transaction 0: cannot convert nil transaction",
		},
		"recorded block": {
			recorded: block,
			want:     converted.Header(),
		},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			db := blockdb.NewMockBlockDB(gomock.NewController(t))
			db.EXPECT().Get(uint64(42), uint64(4)).Return(tt.recorded, tt.err)

			header, err := getRecordedHeader(db, 42, 4)
			if tt.wantErr != "" {
				require.EqualError(t, err, tt.wantErr)
				return
			}
			require.NoError(t, err)
			require.Equal(t, tt.want, header)
		})
	}
}

func TestGetLedgerHead_SearchesGasUsedOfRecordedHashUpToGasLimitOfFailedBundles(t *testing.T) {
	recorded := &blockdb.Block{Number: 4}
	converted, err := convert.ConvertToGethBlock(recorded)
	require.NoError(t, err)
	failedBundles := []sonic.FailedBundle{{Tx: types.NewTx(&types.LegacyTx{Gas: 400})}, {Tx: types.NewTx(&types.LegacyTx{Gas: 600})}}
	tests := map[string]struct {
		gasUsed       uint64
		failedBundles []sonic.FailedBundle
		wantErr       bool
	}{
		"gas used of receipts":                      {},
		"gas used within failed bundles' gas limit": {gasUsed: 700, failedBundles: failedBundles},
		"gas used up to failed bundles' gas limit":  {gasUsed: 1000, failedBundles: failedBundles},
		"gas used beyond failed bundles' gas limit": {gasUsed: 1001, failedBundles: failedBundles, wantErr: true},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			header := converted.Header()
			header.GasUsed = tt.gasUsed

			ctrl := gomock.NewController(t)
			db := blockdb.NewMockBlockDB(ctrl)
			db.EXPECT().Get(uint64(42), uint64(4)).Return(recorded, nil)
			db.EXPECT().Get(uint64(42), uint64(5)).Return(&blockdb.Block{Number: 5, ParentHash: header.Hash().Bytes()}, nil)
			metadataStore := sonic.NewMockMetadataStore(ctrl)
			if tt.failedBundles != nil {
				metadataStore.EXPECT().GetFailedBundlesAtBlock(uint64(4)).Return(tt.failedBundles)
			}

			head, err := getLedgerHead(db, metadataStore, 42, 4)
			if tt.wantErr {
				require.EqualError(t, err, fmt.Sprintf("no gas used of block 4 up to 1000 gives the hash %v recorded by block 5", header.Hash()))
				return
			}
			require.NoError(t, err)
			require.Equal(t, header, head)
		})
	}
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

var replayers = map[string]replayer{
	"Loop":     runReplayLoop,
	"Pipeline": runReplayPipeline,
}

func TestReplayLoop_CanProcessBlocks(t *testing.T) {
	for name, run := range replayers {
		t.Run(name, func(t *testing.T) {
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
				require.NoError(t, err)

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

			var done []uint64
			require.NoError(t, run(t.Context(), utils.NewIter(blocks), executor, nil, ReplayLoopContext{}, func(block *types.Block) error {
				done = append(done, block.NumberU64())
				return nil
			}, nil))
			require.Equal(t, []uint64{0, 1, 2}, done)
		})
	}
}

func TestReplayLoop_ReportsFailures(t *testing.T) {
	injectedError := fmt.Errorf("injected error")
	applyBlock := func(receipts types.Receipts, stateRoot common.Hash, err error) func(*MockHistoryExecutor) {
		return func(executor *MockHistoryExecutor) {
			executor.EXPECT().ApplyBlock(gomock.Any()).Return(receipts, future.Immediate(result.Ok(stateRoot)), err)
		}
	}
	tests := map[string]struct {
		blocks    iter.Seq2[*blockdb.Block, error]
		cancelled bool
		expect    func(*MockHistoryExecutor)
		wantErr   string
		cause     error
	}{
		"failed block retrieval": {
			blocks:  func(yield func(*blockdb.Block, error) bool) { yield(nil, injectedError) },
			wantErr: "failed to get next block",
			cause:   injectedError,
		},
		"cancelled context": {
			blocks:    utils.NewIter([]*blockdb.Block{{Number: 0, StateRoot: types.EmptyRootHash[:]}}),
			cancelled: true,
			wantErr:   "context canceled",
			cause:     context.Canceled,
		},
		"block conversion error": {
			blocks:  utils.NewIter([]*blockdb.Block{{Transactions: []*blockdb.Transaction{{TransactionType: 99_999}}}}),
			wantErr: "failed to convert block 0",
		},
		// The blocks of the following cases have number 0, so GetBlockHash is
		// never called.
		"block application error": {
			blocks:  utils.NewIter([]*blockdb.Block{{}}),
			expect:  applyBlock(nil, common.Hash{}, injectedError),
			wantErr: "failed to apply block 0",
			cause:   injectedError,
		},
		"different receipts": {
			blocks:  utils.NewIter([]*blockdb.Block{{}}),
			expect:  applyBlock(types.Receipts{{}}, common.Hash{}, nil),
			wantErr: "number of receipts mismatch",
		},
		"incorrect state root": {
			blocks: utils.NewIter([]*blockdb.Block{{StateRoot: common.Hash{0x2}.Bytes()}}),
			expect: func(executor *MockHistoryExecutor) {
				executor.EXPECT().IsMptConformant().Return(true)
				applyBlock(nil, common.Hash{0x1}, nil)(executor)
			},
			wantErr: "state root mismatch",
		},
		"parent hash mismatch": {
			blocks: utils.NewIter([]*blockdb.Block{{Number: 1, ParentHash: common.Hash{0xCD}.Bytes()}}),
			expect: func(executor *MockHistoryExecutor) {
				executor.EXPECT().IsMptConformant().Return(true)
				executor.EXPECT().GetBlockHash(uint64(0)).Return(common.Hash{0xAB})
				applyBlock(nil, common.Hash{0x1}, nil)(executor)
			},
			wantErr: "parent hash mismatch",
		},
	}
	for name, tt := range tests {
		for runName, run := range replayers {
			t.Run(name+"/"+runName, func(t *testing.T) {
				executor := newMockHistoryExecutorWithMaybeSnapshot(gomock.NewController(t))
				// ChainID is only called by the pipeline (for signer creation), not by
				// the linear loop, so the exact count varies with the runner.
				executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
				if tt.expect != nil {
					tt.expect(executor)
				}
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				if tt.cancelled {
					cancel()
				}

				err := run(ctx, tt.blocks, executor, nil, ReplayLoopContext{}, nil, nil)
				require.ErrorContains(t, err, tt.wantErr)
				if tt.cause != nil {
					require.ErrorIs(t, err, tt.cause)
				}
			})
		}
	}
}

func TestReplayLoop_StopsOnArchiveVerifierFailure(t *testing.T) {
	for name, run := range replayers {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
			// ChainID is only called by the pipeline (for signer creation), not by
			// the linear loop, so the exact count varies with the runner.
			executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
			executor.EXPECT().IsMptConformant().Return(true).MinTimes(1)
			executor.EXPECT().GetBlockHash(gomock.Any()).Return(common.Hash{}).MinTimes(1)
			executor.EXPECT().CloneBlockHashHistory().Return(nil).MinTimes(1)
			executor.EXPECT().GetHeadHeader().Return(nil).MinTimes(1)

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
		})
	}
}

func TestReplayLoop_SubmitsBlocksWithParentHeaderAndHashHistory(t *testing.T) {
	for name, run := range replayers {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			executor := newMockHistoryExecutorWithMaybeSnapshot(ctrl)
			// ChainID is only called by the pipeline (for signer creation), not by
			// the linear loop, so the exact count varies with the runner.
			executor.EXPECT().ChainID().Return(uint64(12)).AnyTimes()
			executor.EXPECT().IsMptConformant().Return(true)
			executor.EXPECT().GetBlockHash(uint64(0)).Return(common.Hash{})

			// Both are captured before the block is applied.
			parentHeader := &types.Header{}
			hashHistory := &blockHashHistory{}
			executor.EXPECT().ApplyBlock(gomock.Any()).
				After(executor.EXPECT().GetHeadHeader().Return(parentHeader)).
				After(executor.EXPECT().CloneBlockHashHistory().Return(hashHistory)).
				Return(nil, future.Immediate(result.Ok(common.Hash{})), nil)

			pool, err := utils.NewRandomRetentionPool[blockWithHashHistory](poolCapacity)
			require.NoError(t, err)
			blocks := utils.NewIter([]*blockdb.Block{{Number: 1, StateRoot: common.Hash{}.Bytes()}})
			require.NoError(t, run(t.Context(), blocks, executor, nil, ReplayLoopContext{}, nil, &archiveVerifier{pool: pool}))

			item, ok := pool.GetRandom()
			require.True(t, ok)
			require.Same(t, parentHeader, item.parentHeader)
			require.Same(t, hashHistory, item.hashHistory)
		})
	}
}

func TestReplayLoop_OverwriteStateRootHash(t *testing.T) {
	for name, run := range replayers {
		t.Run(name, func(t *testing.T) {
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
			)

			ctxt := t.Context()
			blocks := utils.NewIter([]*blockdb.Block{{
				Number:    1,
				StateRoot: common.Hash{0x2}.Bytes(),
			}})
			require.NoError(t,
				run(ctxt, blocks, executor, blockDB, ReplayLoopContext{
					overwriteStateRoot: OverwriteStateRootPolicyOn,
				}, nil, nil),
			)
		})
	}
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

// applyBlockOnArchive applies the block with ApplyBlockOnArchive, an empty
// parent header and an empty hash history.
func applyBlockOnArchive(executor *historyExecutor, block *types.Block) (types.Receipts, error) {
	return executor.ApplyBlockOnArchive(block, &types.Header{}, &blockHashHistory{})
}

func TestHistoryExecutor_ApplyBlock_And_ApplyBlockOnArchive_SkipBlockZero(t *testing.T) {
	tests := map[string]struct {
		apply          func(executor *historyExecutor, block *types.Block) (types.Receipts, error)
		readsStateRoot bool
		recordsHash    bool
	}{
		"ApplyBlock":          {apply: applyBlock, readsStateRoot: true, recordsHash: true},
		"ApplyBlockOnArchive": {apply: applyBlockOnArchive},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)
			db := carmen.NewMockStateDB(ctrl)
			if tt.readsStateRoot {
				db.EXPECT().GetCommitment().Return(future.Immediate(result.Ok(cc.Hash{})))
			}
			// The block executor expects no calls.
			executor := &historyExecutor{
				blockExecutor:    NewMockBlockExecutor(ctrl),
				blockHashHistory: &blockHashHistory{},
				state:            &state.State{DB: db},
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
				blockExecutor.ExecuteBlock(executor.state, block, executor.blockHashHistory).Return(nil, nil, injected)
			},
		},
		"ApplyBlockOnArchive": {
			apply: applyBlockOnArchive,
			expect: func(blockExecutor *MockBlockExecutorMockRecorder, executor *historyExecutor, block *types.Block) {
				blockExecutor.ExecuteBlockOnArchive(executor.state, block, &types.Header{}, &blockHashHistory{}).Return(nil, injected)
			},
		},
	}

	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			blockExecutor := NewMockBlockExecutor(gomock.NewController(t))
			// The executor's head header and hash history differ from the empty ones of applyBlockOnArchive.
			executor := &historyExecutor{blockExecutor: blockExecutor, blockHashHistory: &blockHashHistory{}, headHeader: &types.Header{GasUsed: 1}, state: &state.State{}}
			executor.blockHashHistory.SetBlockHash(0, common.Hash{1})
			block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1)})
			tt.expect(blockExecutor.EXPECT(), executor, block)

			_, err := tt.apply(executor, block)
			require.ErrorIs(t, err, injected)
		})
	}
}

func TestHistoryExecutor_ApplyBlock_ReplacesHeaderAndRecordsItAsHead(t *testing.T) {
	ctrl := gomock.NewController(t)
	db := carmen.NewMockStateDB(ctrl)
	db.EXPECT().GetCommitment().Return(future.Immediate(result.Ok(cc.Hash{})))
	state := &state.State{DB: db}
	blockExecutor := NewMockBlockExecutor(ctrl)
	executor := &historyExecutor{blockExecutor: blockExecutor, blockHashHistory: &blockHashHistory{}, state: state}
	block := types.NewBlockWithHeader(&types.Header{Number: big.NewInt(1)})
	header := &types.Header{Number: big.NewInt(1), GasUsed: 21_000}
	want := types.Receipts{{}}
	blockExecutor.EXPECT().ExecuteBlock(state, block, executor.blockHashHistory).Return(want, header, nil)

	receipts, _, err := executor.ApplyBlock(block)
	require.NoError(t, err)
	require.Equal(t, want, receipts)
	require.Equal(t, header.Hash(), block.Hash())
	require.Equal(t, header, executor.GetHeadHeader())
	require.Equal(t, header.Hash(), executor.blockHashHistory.GetBlockHash(1))
}

func Test_getExpectedStateRoot_ReturnsStateRootOfSchema(t *testing.T) {
	block := &blockdb.Block{StateRoot: common.Hash{1}.Bytes(), VerkleStateRoot: common.Hash{2}.Bytes()}
	tests := map[string]struct {
		schema carmen.Schema
		want   common.Hash
	}{
		"MPT":    {schema: 5, want: common.Hash{1}},
		"Verkle": {schema: 6, want: common.Hash{2}},
		"other":  {schema: 4},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			require.Equal(t, tt.want, getExpectedStateRoot(&historyExecutor{schema: tt.schema}, block))
		})
	}
}

func Test_updateStateRoot_UpdatesStateRootOfSchema(t *testing.T) {
	root := common.Hash{1}.Bytes()
	tests := map[string]struct {
		schema              carmen.Schema
		wantStateRoot       []byte
		wantVerkleStateRoot []byte
	}{
		"MPT":    {schema: 5, wantStateRoot: root},
		"Verkle": {schema: 6, wantVerkleStateRoot: root},
		"other":  {schema: 4},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			block := &blockdb.Block{}
			updateStateRoot(&historyExecutor{schema: tt.schema}, block, common.Hash{1})
			require.Equal(t, tt.wantStateRoot, block.StateRoot)
			require.Equal(t, tt.wantVerkleStateRoot, block.VerkleStateRoot)
		})
	}
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
	history.SetBlockHash(12, common.Hash{1, 2, 3})
	history.SetBlockHash(11, common.Hash{4, 5, 6})
	history.SetBlockHash(10, common.Hash{7, 8, 9})

	tests := map[string]struct {
		number     uint64
		hash       common.Hash
		parentHash common.Hash
	}{
		"block 12": {number: 12, hash: common.Hash{1, 2, 3}, parentHash: common.Hash{4, 5, 6}},
		"block 11": {number: 11, hash: common.Hash{4, 5, 6}, parentHash: common.Hash{7, 8, 9}},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			header := history.Header(common.Hash{}, tt.number)
			require.Equal(t, tt.number, header.Number.Uint64())
			require.Equal(t, tt.hash, header.Hash)
			require.Equal(t, tt.parentHash, header.ParentHash)
		})
	}
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
