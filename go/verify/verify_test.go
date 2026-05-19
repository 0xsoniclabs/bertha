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

package verify

import (
	"context"
	"encoding/binary"
	"fmt"
	"iter"
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

func TestVerify_RunWithoutParameters_FailsToOpenMissingDb(t *testing.T) {
	ctrl := gomock.NewController(t)
	require.ErrorContains(t,
		Verify(t.Context(), VerifyArgs{}, utils.NewMockProgressIndicatorFactory(ctrl)),
		"failed to open database",
	)
}

func TestVerify_InvalidDirectory_ReportsAnIssue(t *testing.T) {
	ctrl := gomock.NewController(t)
	require.ErrorContains(t,
		Verify(t.Context(), VerifyArgs{DatabaseDir: t.TempDir()}, utils.NewMockProgressIndicatorFactory(ctrl)),
		"failed to open database",
	)
}

func TestVerify_EmptyDatabase_DoesNotReportIssues(t *testing.T) {
	ctrl := gomock.NewController(t)
	require := require.New(t)

	progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)
	// Expect 1 block because interval 0..=0 contains 1 block and the number
	// of blocks is computed from the requested interval, not from the actual
	// content of the database.
	progressIndicatorFactory.EXPECT().New(int64(1), "Verifying blocks").
		Return(utils.NewMockProgressIndicator(ctrl)).Times(1)

	path := t.TempDir()
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	db, err := grocksdb.OpenDb(options, path)
	require.NoError(err, "failed to create database")
	db.Close()

	require.NoError(
		Verify(t.Context(), VerifyArgs{DatabaseDir: path}, progressIndicatorFactory),
	)
}

func TestVerify_ValidContentDatabase_DoesNotReportIssues(t *testing.T) {
	ctrl := gomock.NewController(t)
	require := require.New(t)

	chainID := uint64(123)
	blocks := 10

	progressIndicator := utils.NewMockProgressIndicator(ctrl)
	progressIndicator.EXPECT().Add(1).Return(nil).Times(blocks)
	progressIndicatorFactory := utils.NewMockProgressIndicatorFactory(ctrl)
	progressIndicatorFactory.EXPECT().New(int64(blocks), "Verifying blocks").
		Return(progressIndicator).Times(1)

	path := t.TempDir()
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	db, err := grocksdb.OpenDb(options, path)
	require.NoError(err, "failed to create database")

	writeOptions := grocksdb.NewDefaultWriteOptions()
	for _, block := range utils.CreateValidBlocks(t, blocks) {
		key := make([]byte, 16)
		binary.BigEndian.PutUint64(key[:8], chainID)
		binary.BigEndian.PutUint64(key[8:], uint64(block.Number))

		value, err := proto.Marshal(block)
		require.NoError(err, "failed to marshal block")
		require.NoError(db.Put(writeOptions, key, value))
	}
	writeOptions.Destroy()

	db.Close()

	require.NoError(
		Verify(t.Context(),
			VerifyArgs{DatabaseDir: path, ChainID: chainID, StartBlock: 0, EndBlock: uint64(blocks - 1)},
			progressIndicatorFactory,
		),
	)
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
