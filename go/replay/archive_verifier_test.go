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
	"encoding/binary"
	"math"
	"os"
	"path/filepath"
	"testing"

	"github.com/0xsoniclabs/bertha/blockdb"
	"github.com/0xsoniclabs/bertha/utils"
	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/proto"
)

func TestArchiveVerifier_VerifiesBlocksSuccessfully(t *testing.T) {
	chainID := uint64(123)
	numBlocks := 1100

	dir := t.TempDir()
	genesis := filepath.Join(dir, "genesis.json")
	require.NoError(t, os.WriteFile(genesis, []byte(`{
		"Rules": {
			"NetworkID": 123
		}
	}`), 0644))

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

	for _, block := range utils.CreateValidBlocks(t, numBlocks) {
		key := blockdb.MakeBlockKey(chainID, uint64(block.Number))
		value, err := proto.Marshal(block)
		require.NoError(t, err)
		require.NoError(t, db.Put(writeOptions, key, value))
	}

	db.Close()

	require.NoError(t,
		Replay(t.Context(), ReplayArgs{
			BlockDBDir:      path,
			JSONGenesisFile: genesis,
			WithArchive:     true,
			Interpreter:     "sfvm",
			DBSchema:        5,
			DBVariant:       "go-file",
			EndBlock:        math.MaxUint64,
		}),
	)
}

func TestArchiveVerifier_DetectsReceiptMismatch(t *testing.T) {
	chainID := uint64(123)
	numBlocks := 1100

	dir := t.TempDir()
	genesis := filepath.Join(dir, "genesis.json")
	require.NoError(t, os.WriteFile(genesis, []byte(`{
		"Rules": {
			"NetworkID": 123
		}
	}`), 0644))

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

	blocks := utils.CreateValidBlocks(t, numBlocks)
	// Inject a fake receipt into block 50 — when block 1050 is processed,
	// the archive verifier will try to verify block 50 and detect the mismatch.
	blocks[50].Receipts = []*blockdb.TransactionReceipt{{
		PostStateOrStatus: &blockdb.TransactionReceipt_Status{Status: 1},
		CumulativeGasUsed: 999,
	}}

	for _, block := range blocks {
		key := blockdb.MakeBlockKey(chainID, uint64(block.Number))
		value, err := proto.Marshal(block)
		require.NoError(t, err)
		require.NoError(t, db.Put(writeOptions, key, value))
	}

	db.Close()

	err = Replay(t.Context(), ReplayArgs{
		BlockDBDir:      path,
		JSONGenesisFile: genesis,
		WithArchive:     true,
		NoReceiptsCheck: true,
		Interpreter:     "sfvm",
		DBSchema:        5,
		DBVariant:       "go-file",
		EndBlock:        math.MaxUint64,
	})
	require.ErrorContains(t, err, "archive verifier")
	require.ErrorContains(t, err, "receipts mismatch")
}
