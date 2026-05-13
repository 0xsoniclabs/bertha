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

package replay

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/convert"
	"github.com/0xsoniclabs/carmen/go/state"
	"github.com/ethereum/go-ethereum/core/types"
	"github.com/stretchr/testify/require"
	"go.uber.org/mock/gomock"
)

func TestProgressLogger_ProducesLogMessagesEvery10kSteps(t *testing.T) {
	require := require.New(t)

	logger := startProgressLogger(nil, "", false)

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
		db:             dbMock,
		stateParameter: StateParameters{},
	}

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

	logger := startProgressLogger(state, dir, true)
	res, err := logger.LogProgress(block)
	require.NoError(err)

	require.Regexp(
		`Processing block 10000 from 1970-01-01 [0-9]{2}:[0-9]{2}:[0-9]{2} @ t= 0:00:00, 0.00 txs/s, 0.00 MGas/s, [0-9]+.[0-9]{2}x realtime, live DB size: 0.121 GiB, archive DB size: n/a`,
		res,
	)

	archiveDir := filepath.Join(dir, "archive")
	require.NoError(os.Mkdir(archiveDir, 0700))
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

	block, err := convert.ConvertToGethBlock(&blockdb.Block{
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
