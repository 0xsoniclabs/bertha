package app

import (
	"bytes"
	"context"
	"encoding/binary"
	"fmt"
	"iter"
	"path/filepath"
	"slices"
	"testing"

	"github.com/0xsoniclabs/blockdb"
	"github.com/ethereum/go-ethereum/common"
	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/proto"
)

func TestVerify_RunWithoutParameters_FailsToOpenMissingDb(t *testing.T) {
	require.ErrorContains(t,
		getVerifyCommand().Run(t.Context(), []string{"test"}),
		"failed to open database",
	)
}

func TestVerify_InvalidDirectory_ReportsAnIssue(t *testing.T) {
	path := filepath.Join(t.TempDir(), "missing-db")
	require.ErrorContains(t,
		getVerifyCommand().Run(t.Context(), []string{
			"test",
			"--database-dir", path,
		}),
		"failed to open database",
	)
}

func TestVerify_EmptyDatabase_DoesNotReportIssues(t *testing.T) {
	require := require.New(t)

	path := filepath.Join(t.TempDir(), "empty-db")
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	db, err := grocksdb.OpenDb(options, path)
	require.NoError(err, "failed to create database")
	db.Close()

	require.NoError(
		getVerifyCommand().Run(t.Context(), []string{
			"test",
			"--database-dir", path,
		}),
	)
}

func TestVerify_ValidContentDatabase_DoesNotReportIssues(t *testing.T) {
	require := require.New(t)

	chainId := uint64(123)

	path := filepath.Join(t.TempDir(), "small-db")
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	db, err := grocksdb.OpenDb(options, path)
	require.NoError(err, "failed to create database")

	writeOptions := grocksdb.NewDefaultWriteOptions()
	for _, block := range createValidBlocks(t, 10) {
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
		getVerifyCommand().Run(t.Context(), []string{
			"test",
			"--database-dir", path,
			"--chain-id", "123",
		}),
	)
}

func TestVerifyBlocks_ValidBlockHashSequence_DoesNotReportIssues(t *testing.T) {
	// Create a sequence of valid blocks with proper parent-child relationships.
	validBlocks := createValidBlocks(t, 10)

	// Blocks are processed in reverse order such that the hash of a block is
	// collected from the parent-hash field of the successor before checking
	// the hash of the block itself.
	slices.Reverse(validBlocks)

	require.NoError(t, verifyBlocks(t.Context(), newIter(validBlocks), nil))
}

func TestVerifyBlocks_InvalidBlockHash_IssueIsDetected(t *testing.T) {
	blocks := []*blockdb.Block{{}, {}, {}}
	require.ErrorContains(t,
		verifyBlocks(t.Context(), newIter(blocks), nil),
		"lock verification failed for block 0: block hash mismatch",
	)
}

func TestVerifyBlocks_NilBlockInput_AbortsWithError(t *testing.T) {
	blocks := []*blockdb.Block{{}, nil}
	require.ErrorContains(t,
		verifyBlocks(t.Context(), newIter(blocks), nil),
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

	got := verifyBlocks(ctxt, newIter(blocks), progressCounter)
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
	gethBlock, err := ConvertToGethBlock(block)
	require.NoError(t, err)

	hash := gethBlock.Hash()
	require.NoError(t, verifyBlock(hash, block))
}

func newIter(blocks []*blockdb.Block) iter.Seq2[*blockdb.Block, error] {
	return func() iter.Seq2[*blockdb.Block, error] {
		return func(yield func(*blockdb.Block, error) bool) {
			for _, block := range blocks {
				if !yield(block, nil) {
					break
				}
			}
		}
	}()
}

func createValidBlocks(t *testing.T, num int) []*blockdb.Block {
	t.Helper()
	// Create a sequence of valid blocks with proper parent-child relationships.
	// The first block has no parent, and each subsequent block's parent is the
	// previous block.
	blocks := make([]*blockdb.Block, num)
	lastHash := common.Hash{}
	for i := range num {
		next := &blockdb.Block{
			Number:     uint64(i),
			ParentHash: bytes.Clone(lastHash[:]),
		}
		blocks[i] = next

		block, err := ConvertToGethBlock(next)
		require.NoError(t, err, "failed to convert block to Geth format")
		lastHash = block.Hash()
	}
	return blocks
}
