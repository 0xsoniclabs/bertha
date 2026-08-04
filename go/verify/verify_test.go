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

package verify

import (
	"context"
	"encoding/binary"
	"fmt"
	"iter"
	"math"
	"slices"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/ethereum/go-ethereum/common"
	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
	"google.golang.org/protobuf/proto"
)

func TestVerify_StartBlockAfterEndBlock_ReportsAnIssue(t *testing.T) {
	ctrl := gomock.NewController(t)
	require.ErrorContains(t,
		Verify(
			t.Context(),
			VerifyArgs{DatabaseDir: t.TempDir(), StartBlock: 10, EndBlock: 5},
			utils.NewMockLogger(ctrl),
			utils.NewMockProgressIndicatorFactory(ctrl),
		),
		"start block 10 is greater than end block 5",
	)
}

func TestVerify_RunWithoutParameters_FailsToOpenMissingDb(t *testing.T) {
	ctrl := gomock.NewController(t)

	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Info("Opening block database", "directory", "")

	require.ErrorContains(t,
		Verify(t.Context(), VerifyArgs{}, logger, utils.NewMockProgressIndicatorFactory(ctrl)),
		"failed to open database",
	)
}

func TestVerify_InvalidDirectory_ReportsAnIssue(t *testing.T) {
	ctrl := gomock.NewController(t)

	directory := t.TempDir()
	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Info("Opening block database", "directory", directory)

	require.ErrorContains(t,
		Verify(t.Context(), VerifyArgs{DatabaseDir: directory}, logger, utils.NewMockProgressIndicatorFactory(ctrl)),
		"failed to open database",
	)
}

func TestVerify_NoBlocksInRange_LogsWarning(t *testing.T) {
	chainID := uint64(123)
	storedBlocks := utils.CreateValidBlocks(t, 10) // numbered 0 to 9

	tests := map[string]struct {
		blocks     []*blockdb.Block
		startBlock uint64
		endBlock   uint64
	}{
		"empty database":            {endBlock: math.MaxUint64},
		"range above stored blocks": {blocks: storedBlocks, startBlock: 20, endBlock: 30},
		"range below stored blocks": {blocks: storedBlocks[5:], endBlock: 3},
	}
	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)

			directory := createDatabase(t, chainID, test.blocks)
			logger := utils.NewMockLogger(ctrl)
			logger.EXPECT().Info("Opening block database", "directory", directory)
			logger.EXPECT().Warn("No blocks found",
				"chain_id", chainID,
				"start_block", test.startBlock,
				"end_block", test.endBlock,
			)

			// There is nothing to verify, so no progress indicator is created.
			progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)

			require.NoError(t, Verify(t.Context(),
				VerifyArgs{
					DatabaseDir: directory,
					ChainID:     chainID,
					StartBlock:  test.startBlock,
					EndBlock:    test.endBlock,
				},
				logger,
				progressIndicatorFactory,
			))
		})
	}
}

func TestVerify_BlockRange_IsClampedToStoredBlocks(t *testing.T) {
	chainID := uint64(123)
	storedBlocks := utils.CreateValidBlocks(t, 10) // numbered 0 to 9

	tests := map[string]struct {
		blocks         []*blockdb.Block
		startBlock     uint64
		endBlock       uint64
		wantStartBlock uint64
		wantEndBlock   uint64
	}{
		"unbounded end block":      {blocks: storedBlocks, endBlock: math.MaxUint64, wantEndBlock: 9},
		"end block above highest":  {blocks: storedBlocks, endBlock: 100, wantEndBlock: 9},
		"end block below highest":  {blocks: storedBlocks, endBlock: 4, wantEndBlock: 4},
		"start block above lowest": {blocks: storedBlocks, startBlock: 3, endBlock: 100, wantStartBlock: 3, wantEndBlock: 9},
		"start block below lowest": {blocks: storedBlocks[3:], endBlock: 100, wantStartBlock: 3, wantEndBlock: 9},
	}
	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)

			directory := createDatabase(t, chainID, test.blocks)

			logger := utils.NewMockLogger(ctrl)
			logger.EXPECT().Info("Opening block database", "directory", directory)
			logger.EXPECT().Info("Verifying blocks",
				"chain_id", chainID,
				"start_block", test.wantStartBlock,
				"end_block", test.wantEndBlock,
			)

			wantNumBlocks := int64(test.wantEndBlock - test.wantStartBlock + 1)
			progressIndicator := utils.NewMockProgressIndicator(ctrl)
			progressIndicator.EXPECT().Add(1).Return(nil).Times(int(wantNumBlocks))
			progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)
			progressIndicatorFactory.EXPECT().New(wantNumBlocks, "Verifying blocks").
				Return(progressIndicator)

			require.NoError(t, Verify(t.Context(),
				VerifyArgs{
					DatabaseDir: directory,
					ChainID:     chainID,
					StartBlock:  test.startBlock,
					EndBlock:    test.endBlock,
				},
				logger,
				progressIndicatorFactory,
			))
		})
	}
}

func TestVerify_UnreadableBlockAtRangeBound_ReportsAnIssue(t *testing.T) {
	chainID := uint64(123)

	tests := map[string]struct {
		corruptedBlock uint64
		wantErr        string
	}{
		"lowest block":  {corruptedBlock: 0, wantErr: "failed to read lowest block"},
		"highest block": {corruptedBlock: 9, wantErr: "failed to read highest block"},
	}
	for name, test := range tests {
		t.Run(name, func(t *testing.T) {
			ctrl := gomock.NewController(t)

			directory := createDatabase(t, chainID, utils.CreateValidBlocks(t, 10))
			corruptBlock(t, directory, chainID, test.corruptedBlock)

			logger := utils.NewMockLogger(ctrl)
			logger.EXPECT().Info("Opening block database", "directory", directory)

			require.ErrorContains(t,
				Verify(t.Context(),
					VerifyArgs{DatabaseDir: directory, ChainID: chainID, EndBlock: math.MaxUint64},
					logger,
					utils.NewMockProgressIndicatorFactory(ctrl),
				),
				test.wantErr,
			)
		})
	}
}

func TestVerify_ValidContentDatabase_DoesNotReportIssues(t *testing.T) {
	ctrl := gomock.NewController(t)

	chainID := uint64(123)
	blocks := 10

	progressIndicator := utils.NewMockProgressIndicator(ctrl)
	progressIndicator.EXPECT().Add(1).Return(nil).Times(blocks)
	progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)
	progressIndicatorFactory.EXPECT().New(int64(blocks), "Verifying blocks").
		Return(progressIndicator)

	directory := createDatabase(t, chainID, utils.CreateValidBlocks(t, blocks))

	logger := utils.NewMockLogger(ctrl)
	logger.EXPECT().Info("Opening block database", "directory", directory)
	logger.EXPECT().Info("Verifying blocks",
		"chain_id", chainID,
		"start_block", uint64(0),
		"end_block", uint64(blocks-1),
	)

	require.NoError(t,
		Verify(t.Context(),
			VerifyArgs{DatabaseDir: directory, ChainID: chainID, StartBlock: 0, EndBlock: uint64(blocks - 1)},
			logger,
			progressIndicatorFactory,
		),
	)
}

func TestGetStoredBlockRange_NilBlock_ReportsAnIssue(t *testing.T) {
	database := blockdb.NewMockBlockDB(gomock.NewController(t))
	database.EXPECT().GetRange(uint64(1), uint64(2), uint64(3)).
		Return(utils.NewIter([]*blockdb.Block{nil}))

	_, _, _, err := getStoredBlockRange(database, 1, 2, 3)
	require.ErrorContains(t, err, "encountered nil block")
}

func TestVerifyBlocks_ValidBlockHashSequence_DoesNotReportIssues(t *testing.T) {
	// Create a sequence of valid blocks with proper parent-child relationships.
	validBlocks := utils.CreateValidBlocks(t, 10)

	// Blocks are processed in reverse order such that the hash of a block is
	// collected from the parent-hash field of the successor before checking
	// the hash of the block itself.
	slices.Reverse(validBlocks)

	require.NoError(t, verifyBlocks(t.Context(), utils.NewIter(validBlocks), nil))
}

func TestVerifyBlocks_InvalidBlockHash_IssueIsDetected(t *testing.T) {
	blocks := []*blockdb.Block{{}, {}, {}}
	require.ErrorContains(t,
		verifyBlocks(t.Context(), utils.NewIter(blocks), nil),
		"lock verification failed for block 0: block hash mismatch",
	)
}

func TestVerifyBlocks_NilBlockInput_AbortsWithError(t *testing.T) {
	blocks := []*blockdb.Block{{}, nil}
	require.ErrorContains(t,
		verifyBlocks(t.Context(), utils.NewIter(blocks), nil),
		"encountered nil block",
	)
}

func TestVerifyBlocks_ErrorDuringBlockRetrieval_AbortsWithError(t *testing.T) {
	issue := fmt.Errorf("deliberately introduced error")
	blocks := func() iter.Seq2[*blockdb.Block, error] {
		return func(yield func(*blockdb.Block, error) bool) {
			yield(nil, issue)
		}
	}()

	got := verifyBlocks(t.Context(), blocks, nil)
	require.ErrorContains(t, got, "failed to get block")
	require.ErrorIs(t, got, issue)
}

func TestVerifyBlocks_CancelledContext_ValidationAbortsWithError(t *testing.T) {
	blocks := []*blockdb.Block{{}, {}, {}}

	ctxt, cancel := context.WithCancel(t.Context())

	counter := 0
	progressCounter := func(uint64) {
		counter++
		if counter == 1 {
			cancel()
		}
	}

	got := verifyBlocks(ctxt, utils.NewIter(blocks), progressCounter)
	want := ctxt.Err()
	require.Error(t, want, "context should be cancelled")
	require.ErrorIs(t, got, want)
	require.Equal(t, 1, counter, "progress callback should not be called after context cancellation")
}

func TestVerifyBlock_InvalidBlock_FailsOnBlockConversion(t *testing.T) {
	block := &blockdb.Block{
		Transactions: []*blockdb.Transaction{
			{TransactionType: 999}, // Invalid transaction type
		},
	}
	err := verifyBlock(common.Hash{}, block)
	require.ErrorContains(t, err, "unsupported transaction type")
}

func TestVerifyBlock_InvalidHash_ReportsInvalidHash(t *testing.T) {
	// Believe it or not, this is a valid encoding of a block.
	block := &blockdb.Block{}
	err := verifyBlock(common.Hash{}, block)
	require.ErrorContains(t, err, "block hash mismatch")
}

func TestVerifyBlock_CorrectHash_VerifyPasses(t *testing.T) {
	block := &blockdb.Block{}
	gethBlock, err := convert.ConvertToGethBlock(block)
	require.NoError(t, err)

	hash := gethBlock.Hash()
	require.NoError(t, verifyBlock(hash, block))
}

// createDatabase creates a block database holding the given blocks under the given chain ID and
// returns its directory. The database is closed before returning so that it can be opened again.
func createDatabase(t *testing.T, chainID uint64, blocks []*blockdb.Block) string {
	t.Helper()
	directory := t.TempDir()
	db, writeOptions := openDatabaseForWriting(t, directory)
	defer db.Close()

	version := make([]byte, 8)
	binary.BigEndian.PutUint64(version, blockdb.CurrentVersion)
	require.NoError(t, db.Put(writeOptions, blockdb.MakeVersionKey(), version))

	for _, block := range blocks {
		value, err := proto.Marshal(block)
		require.NoError(t, err)
		require.NoError(t, db.Put(writeOptions, blockdb.MakeBlockKey(chainID, block.Number), value))
	}

	return directory
}

// corruptBlock replaces the value stored for the given block with data that cannot be parsed.
// The database is closed before returning so that it can be opened again.
func corruptBlock(t *testing.T, directory string, chainID uint64, blockNumber uint64) {
	t.Helper()
	db, writeOptions := openDatabaseForWriting(t, directory)
	defer db.Close()

	invalidBlock := []byte{0x00}
	require.NoError(t, db.Put(writeOptions, blockdb.MakeBlockKey(chainID, blockNumber), invalidBlock))
}

// openDatabaseForWriting opens the database in the given directory, creating it if it is missing.
// The returned options are destroyed when the test ends, the database must be closed by the caller.
func openDatabaseForWriting(t *testing.T, directory string) (*grocksdb.DB, *grocksdb.WriteOptions) {
	t.Helper()
	options := grocksdb.NewDefaultOptions()
	t.Cleanup(options.Destroy)
	options.SetCreateIfMissing(true)
	db, err := grocksdb.OpenDb(options, directory)
	require.NoError(t, err)

	writeOptions := grocksdb.NewDefaultWriteOptions()
	t.Cleanup(writeOptions.Destroy)
	return db, writeOptions
}
