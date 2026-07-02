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
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestProgressLogger_ProducesLogMessagesEvery10kSteps(t *testing.T) {
	require := require.New(t)
	ctrl := gomock.NewController(t)
	mockLogger := utils.NewMockLogger(ctrl)

	logger := startProgressLogger(mockLogger, nil, "", false)

	block0, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:    0,
		Timestamp: 1000,
	})
	require.NoError(err)
	block10k, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:    10_000,
		Timestamp: 2000,
	})
	require.NoError(err)
	block15k, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:    15_000,
		Timestamp: 2000,
	})
	require.NoError(err)
	block20k, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:    20_000,
		Timestamp: 3500,
	})
	require.NoError(err)

	require.NoError(logger.LogProgress(block0))

	mockLogger.EXPECT().Info(
		"Processing block",
		"block", uint64(10_000),
		"block_time", time.Unix(2000, 0).UTC().Format(time.RFC3339),
		"elapsed", gomock.Any(),
		"txs/s", 0,
		"MGas/s", 0,
		"realtime", gomock.Any(),
	)
	require.NoError(logger.LogProgress(block10k))

	require.NoError(logger.LogProgress(block15k))

	mockLogger.EXPECT().Info(
		"Processing block",
		"block", uint64(20_000),
		"block_time", time.Unix(3500, 0).UTC().Format(time.RFC3339),
		"elapsed", gomock.Any(),
		"txs/s", 0,
		"MGas/s", 0,
		"realtime", gomock.Any(),
	)
	require.NoError(logger.LogProgress(block20k))
}

func TestProgressLogger_PrintsDirSizeIfEnabled(t *testing.T) {
	require := require.New(t)
	ctrl := gomock.NewController(t)
	flusher := NewMockStateFlusher(ctrl)
	flusher.EXPECT().FlushState().Return(nil).Times(2)

	dir := t.TempDir()
	liveDir := filepath.Join(dir, "live")
	require.NoError(os.Mkdir(liveDir, 0700))

	filePath := filepath.Join(liveDir, "file1.txt")
	data := make([]byte, 124*1024*1024)
	err := os.WriteFile(filePath, data, 0644)
	require.NoError(err)

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:       10000,
		Timestamp:    1000,
		Transactions: []*blockdb.Transaction{},
	})
	require.NoError(err)

	mockLogger := utils.NewMockLogger(ctrl)
	logger := startProgressLogger(mockLogger, flusher, dir, true)
	mockLogger.EXPECT().Info(
		"Processing block",
		"block", uint64(10000),
		"block_time", gomock.Any(),
		"elapsed", gomock.Any(),
		"txs/s", 0,
		"MGas/s", 0,
		"realtime", gomock.Any(),
		"LiveDB size", "0.121GiB",
	)
	err = logger.LogProgress(block)
	require.NoError(err)

	archiveDir := filepath.Join(dir, "archive")
	require.NoError(os.Mkdir(archiveDir, 0700))
	filePath = filepath.Join(archiveDir, "file2.txt")
	data = make([]byte, 156*1024*1024)
	err = os.WriteFile(filePath, data, 0644)
	require.NoError(err)

	mockLogger2 := utils.NewMockLogger(ctrl)
	logger = startProgressLogger(mockLogger2, flusher, dir, true)
	mockLogger2.EXPECT().Info(
		"Processing block",
		"block", uint64(10000),
		"block_time", gomock.Any(),
		"elapsed", gomock.Any(),
		"txs/s", 0,
		"MGas/s", 0,
		"realtime", gomock.Any(),
		"LiveDB size", "0.121GiB",
		"ArchiveDB size", "0.152GiB",
	)
	err = logger.LogProgress(block)
	require.NoError(err)
}

func TestProgressLogger_ProducesASummary(t *testing.T) {
	require := require.New(t)
	ctrl := gomock.NewController(t)
	mockLogger := utils.NewMockLogger(ctrl)

	logger := startProgressLogger(mockLogger, nil, "", false)

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
		Number:    0,
		Timestamp: 1000,
		Transactions: []*blockdb.Transaction{
			{TransactionType: types.LegacyTxType, Nonce: 0},
			{TransactionType: types.LegacyTxType, Nonce: 1},
		},
	})
	require.NoError(err)

	require.NoError(logger.LogProgress(block))

	mockLogger.EXPECT().Info(
		"Replay finished",
		"elapsed", gomock.Any(),
		"txs", uint64(2),
		"TGas", float64(0),
		"txs/s", gomock.Any(),
		"MGas/s", 0,
		"realtime", 0,
	)
	logger.LogSummary()
}
