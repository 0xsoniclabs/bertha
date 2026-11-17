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

package blockdb

import (
	"encoding/binary"
	"path/filepath"
	"testing"

	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/proto"
)

type OpenRocksDBFunc func(path string) (RocksDB, error)

var dbOpeners = map[string]OpenRocksDBFunc{
	"OpenRocksDBForWriting": OpenRocksDBForWriting,
	"OpenRocksDBForReading": OpenRocksDBForReading,
}

func TestOpenRocksDB_FailsIfDBDoesNotExist(t *testing.T) {
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "non-existing-db-path")
			_, err := dbOpener(path)
			require.Error(t, err, "opening db did not return an error although path does not exist")
		})
	}
}

func TestOpenRocksDB_VerifiesBlockDBVersion(t *testing.T) {
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			tests := map[string]struct {
				prepareDB func(*testing.T, string)
				errMsg    string
			}{
				"fails when version key is missing": {
					prepareDB: func(t *testing.T, path string) {
						db, err := createDBWithoutVersion(path)
						require.NoError(t, err)
						db.close()
					},
					errMsg: "block database version not found",
				},
				"fails when version payload length is invalid": {
					prepareDB: func(t *testing.T, path string) {
						db, err := createDBWithoutVersion(path)
						require.NoError(t, err)
						require.NoError(t, db.putRaw(MakeVersionKey(), []byte{1, 2, 3}))
						db.close()
					},
					errMsg: "invalid block database version",
				},
				"fails when version is unsupported": {
					prepareDB: func(t *testing.T, path string) {
						db, err := createDBWithoutVersion(path)
						require.NoError(t, err)
						require.NoError(t, db.putVersion(CurrentVersion+1))
						db.close()
					},
					errMsg: "block database version not supported",
				},
				"succeeds when version is correct": {
					prepareDB: func(t *testing.T, path string) {
						db, err := createDBWithoutVersion(path)
						require.NoError(t, err)
						require.NoError(t, db.putVersion(CurrentVersion))
						db.close()
					},
					errMsg: "",
				},
			}

			for name, test := range tests {
				t.Run(name, func(t *testing.T) {
					path := t.TempDir()
					test.prepareDB(t, path)

					db, err := dbOpener(path)
					if test.errMsg == "" {
						require.NoError(t, err)
						require.NoError(t, db.Close())
					} else {
						require.ErrorContains(t, err, test.errMsg)
					}
				})
			}
		})
	}
}

func TestRocksDB_Get_DecodesStoredBlock(t *testing.T) {
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			chainID := uint64(3)
			path := t.TempDir()
			writeDB, err := createDB(path)
			require.NoError(t, err)
			for _, number := range []uint64{1, 2, 3} {
				require.NoError(t, writeDB.putBlock(chainID, &Block{Number: number}))
			}
			require.NoError(t, writeDB.putRaw(MakeBlockKey(chainID, 4), []byte{0x00}))
			writeDB.close()

			db, err := dbOpener(path)
			require.NoError(t, err)
			defer func() { require.NoError(t, db.Close()) }()

			tests := map[string]struct {
				chainID, blockNumber uint64
				wantBlock            bool
				wantErr              string
			}{
				"non-existing chain ID":     {chainID: 0, blockNumber: 1},
				"non-existing block number": {chainID: chainID, blockNumber: 0},
				"invalid block":             {chainID: chainID, blockNumber: 4, wantErr: "cannot parse invalid wire-format data"},
				"existing block":            {chainID: chainID, blockNumber: 2, wantBlock: true},
			}

			for name, test := range tests {
				t.Run(name, func(t *testing.T) {
					block, err := db.Get(test.chainID, test.blockNumber)
					if test.wantErr != "" {
						require.ErrorContains(t, err, test.wantErr)
					} else {
						require.NoError(t, err)
					}
					if test.wantBlock {
						require.NotNil(t, block)
						require.Equal(t, test.blockNumber, block.Number)
					} else {
						require.Nil(t, block)
					}
				})
			}
		})
	}
}

func TestRocksDB_Update_StoresBlock(t *testing.T) {
	tests := map[string]struct {
		stored *Block
	}{
		"new block":      {},
		"existing block": {stored: &Block{Number: 1, StateRoot: []byte{0x1, 0x2}}},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			path := t.TempDir()
			writeDB, err := createDB(path)
			require.NoError(t, err)
			writeDB.close()

			db, err := OpenRocksDBForWriting(path)
			require.NoError(t, err)
			defer func() { require.NoError(t, db.Close()) }()
			if tt.stored != nil {
				require.NoError(t, db.Update(1, tt.stored))
			}
			before, err := db.Get(1, 1)
			require.NoError(t, err)
			require.True(t, proto.Equal(tt.stored, before))

			block := &Block{Number: 1, StateRoot: []byte{0x3, 0x4}}
			require.NoError(t, db.Update(1, block))
			got, err := db.Get(1, 1)
			require.NoError(t, err)
			require.Equal(t, block.Number, got.Number)
			require.Equal(t, block.StateRoot, got.StateRoot)
		})
	}
}

func TestRocksDB_GetRange_ReturnsExistingSubRange(t *testing.T) {
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			chainID := uint64(3)
			blockNumbers := []uint64{1, 2, 3, 20}

			path, err := fillDBWithBlocks(t, chainID, blockNumbers)
			require.NoError(t, err, "failed to create db")

			// db now contains blocks 1, 2, 3 and 20 for chainId 3

			db, err := dbOpener(path)
			require.NoError(t, err, "failed to open db")

			tests := map[string]struct {
				chainID, startBlockNumber, endBlockNumber uint64
				expectedBlockNumbers                      []uint64
			}{
				"non-existing chain ID":                      {0, 0, 5, []uint64{}},
				"non-existing block numbers":                 {chainID, 5, 10, []uint64{}},
				"range containing the blocks":                {chainID, 0, 5, []uint64{1, 2, 3}},
				"range starting before and ending within":    {chainID, 0, 2, []uint64{1, 2}},
				"range starting within and ending after":     {chainID, 2, 5, []uint64{2, 3}},
				"range covering a hole in the stored blocks": {chainID, 0, 30, []uint64{1, 2, 3, 20}},
			}

			for name, test := range tests {
				t.Run(name, func(t *testing.T) {
					i := 0
					for block, err := range db.GetRange(test.chainID, test.startBlockNumber, test.endBlockNumber) {
						require.NoError(t, err, "expected nil error when retrieving a block")
						require.NotNil(t, block, "expected block to exist")
						require.Equal(t, test.expectedBlockNumbers[i], block.Number, "unexpected block number at index %d", i)
						i++
					}
					require.Equal(t, len(test.expectedBlockNumbers), i, "expected %d blocks, got %d", len(test.expectedBlockNumbers), i)
				})
			}
		})
	}
}

func TestRocksDB_GetRange_ReturnsErrorIfBlockIsInvalid(t *testing.T) {
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			chainID := uint64(3)
			blockNumber := uint64(1)

			path, err := fillDBWithInvalidBlock(t, chainID, blockNumber)
			require.NoError(t, err, "failed to create db")

			// db now contains an invalid blocks at blocknumber 1 for chainId 3

			db, err := dbOpener(path)
			require.NoError(t, err, "failed to open db")

			count := 0
			for block, err := range db.GetRange(chainID, blockNumber, blockNumber) {
				require.Error(t, err, "expected error when retrieving an invalid block")
				require.Nil(t, block, "expected nil block when retrieving an invalid block")
				count++
			}
			require.Equal(t, 1, count, "expected %d blocks for chainID %d from %d to %d, got %d",
				1, chainID, blockNumber, blockNumber, count)
		})
	}
}

func TestRocksDB_GetRange_StopsAtNonBlockKey(t *testing.T) {
	chainID := uint64(3)
	wrongPrefixKey := MakeBlockKey(chainID, 4)
	wrongPrefixKey[0] = 0x03
	tests := map[string]struct {
		blocks []uint64
		key    []byte
		want   []uint64
	}{
		// Appending a byte to the key of block 3 gives an 18-byte key that sorts
		// between block 3 and block 4.
		"non-17-byte key": {
			blocks: []uint64{1, 2, 3, 4},
			key:    append(MakeBlockKey(chainID, 3), 0x00),
			want:   []uint64{1, 2, 3},
		},
		// The key of block 4 with prefix 0x03 instead of 0x02 sorts after all
		// block keys. Without the prefix check, it would be parsed as block 4.
		"wrong prefix key": {
			blocks: []uint64{1, 2, 3},
			key:    wrongPrefixKey,
			want:   []uint64{1, 2, 3},
		},
	}
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			for name, tt := range tests {
				t.Run(name, func(t *testing.T) {
					path := t.TempDir()
					db, err := createDB(path)
					require.NoError(t, err)
					for _, number := range tt.blocks {
						require.NoError(t, db.putBlock(chainID, &Block{Number: number}))
					}
					require.NoError(t, db.putRaw(tt.key, []byte{0x00}))
					db.close()

					rocksDB, err := dbOpener(path)
					require.NoError(t, err)
					defer func() { require.NoError(t, rocksDB.Close()) }()

					var got []uint64
					for block, err := range rocksDB.GetRange(chainID, 1, 10) {
						require.NoError(t, err)
						got = append(got, block.Number)
					}
					require.Equal(t, tt.want, got)
				})
			}
		})
	}
}

func TestRocksDB_GetRangeRev_ReturnsExistingSubRangeInReverseOrder(t *testing.T) {
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			chainID := uint64(3)
			blockNumbers := []uint64{1, 2, 3, 20}

			path, err := fillDBWithBlocks(t, chainID, blockNumbers)
			require.NoError(t, err, "failed to create db")

			// db now contains blocks 1, 2, 3 and 20 for chainId 3

			db, err := dbOpener(path)
			require.NoError(t, err, "failed to open db")

			tests := map[string]struct {
				chainID, startBlockNumber, endBlockNumber uint64
				expectedBlockNumbers                      []uint64
			}{
				"non-existing chain ID":                      {0, 0, 5, []uint64{}},
				"non-existing block numbers":                 {chainID, 5, 10, []uint64{}},
				"range containing the blocks":                {chainID, 0, 5, []uint64{3, 2, 1}},
				"range starting before and ending within":    {chainID, 0, 2, []uint64{2, 1}},
				"range starting within and ending after":     {chainID, 2, 5, []uint64{3, 2}},
				"range covering a hole in the stored blocks": {chainID, 0, 30, []uint64{20, 3, 2, 1}},
			}

			for name, test := range tests {
				t.Run(name, func(t *testing.T) {
					i := 0
					for block, err := range db.GetRangeRev(test.chainID, test.startBlockNumber, test.endBlockNumber) {
						require.NoError(t, err, "expected nil error when retrieving a block")
						require.NotNil(t, block, "expected block to exist")
						require.Equal(t, test.expectedBlockNumbers[i], block.Number, "unexpected block number at index %d", i)
						i++
					}
					require.Equal(t, len(test.expectedBlockNumbers), i, "expected %d blocks, got %d", len(test.expectedBlockNumbers), i)
				})
			}
		})
	}
}

func TestRocksDB_GetRangeRev_StopsAtNonBlockKey(t *testing.T) {
	chainID := uint64(3)
	wrongPrefixKey := MakeBlockKey(chainID, 0)
	wrongPrefixKey[0] = 0x01
	tests := map[string]struct {
		blocks []uint64
		key    []byte
		want   []uint64
	}{
		// Appending a byte to the key of block 3 gives an 18-byte key that sorts
		// between block 3 and block 4.
		"non-17-byte key": {
			blocks: []uint64{2, 3, 4, 5},
			key:    append(MakeBlockKey(chainID, 3), 0x00),
			want:   []uint64{5, 4},
		},
		// The key of block 0 with prefix 0x01 instead of 0x02 sorts before all
		// block keys. Without the prefix check, it would be parsed as block 0.
		"wrong prefix key": {
			blocks: []uint64{1, 2, 3},
			key:    wrongPrefixKey,
			want:   []uint64{3, 2, 1},
		},
	}
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			for name, tt := range tests {
				t.Run(name, func(t *testing.T) {
					path := t.TempDir()
					db, err := createDB(path)
					require.NoError(t, err)
					for _, number := range tt.blocks {
						require.NoError(t, db.putBlock(chainID, &Block{Number: number}))
					}
					require.NoError(t, db.putRaw(tt.key, []byte{0x00}))
					db.close()

					rocksDB, err := dbOpener(path)
					require.NoError(t, err)
					defer func() { require.NoError(t, rocksDB.Close()) }()

					var got []uint64
					for block, err := range rocksDB.GetRangeRev(chainID, 0, 10) {
						require.NoError(t, err)
						got = append(got, block.Number)
					}
					require.Equal(t, tt.want, got)
				})
			}
		})
	}
}

func TestRocksDB_GetRangeRev_ReturnsErrorIfBlockIsInvalid(t *testing.T) {
	for name, dbOpener := range dbOpeners {
		t.Run(name, func(t *testing.T) {
			chainID := uint64(3)
			blockNumber := uint64(1)

			path, err := fillDBWithInvalidBlock(t, chainID, blockNumber)
			require.NoError(t, err, "failed to create db")

			// db now contains an invalid blocks at blocknumber 1 for chainId 3

			db, err := dbOpener(path)
			require.NoError(t, err, "failed to open db")

			count := 0
			for block, err := range db.GetRangeRev(chainID, blockNumber, blockNumber) {
				require.Error(t, err, "expected error when retrieving an invalid block")
				require.Nil(t, block, "expected nil block when retrieving an invalid block")
				count++
			}
			require.Equal(t, 1, count, "expected %d blocks for chainID %d from %d to %d, got %d",
				1, chainID, blockNumber, blockNumber, count)

		})
	}
}

func TestRocksDB_GetRulesUpdateHeights_And_GetCorrections_And_GetFailedBundles_ReturnStoredDataOrNil(t *testing.T) {
	chainID := uint64(3)
	getters := map[string]struct {
		key func(uint64) []byte
		get func(RocksDB, uint64) ([]byte, error)
	}{
		"GetRulesUpdateHeights": {key: MakeRulesUpdateHeightsKey, get: RocksDB.GetRulesUpdateHeights},
		"GetCorrections":        {key: MakeCorrectionsKey, get: RocksDB.GetCorrections},
		"GetFailedBundles":      {key: MakeFailedBundlesKey, get: RocksDB.GetFailedBundles},
	}
	stored := map[string][]byte{
		"stored":     []byte(`stored data`),
		"not stored": nil,
	}
	for openerName, dbOpener := range dbOpeners {
		for getterName, getter := range getters {
			for storedName, data := range stored {
				t.Run(openerName+"/"+getterName+"/"+storedName, func(t *testing.T) {
					path := t.TempDir()
					db, err := createDB(path)
					require.NoError(t, err)
					if data != nil {
						require.NoError(t, db.putRaw(getter.key(chainID), data))
					}
					db.close()

					rocksDB, err := dbOpener(path)
					require.NoError(t, err)
					defer func() { require.NoError(t, rocksDB.Close()) }()

					got, err := getter.get(rocksDB, chainID)
					require.NoError(t, err)
					if data == nil {
						require.Nil(t, got)
					} else {
						require.Equal(t, data, got)
					}
				})
			}
		}
	}
}

func TestRocksDB_PutRulesUpdateHeights_StoresDataIfWritable(t *testing.T) {
	tests := map[string]struct {
		dbOpener OpenRocksDBFunc
		wantErr  string
	}{
		"writable db":  {dbOpener: OpenRocksDBForWriting},
		"read-only db": {dbOpener: OpenRocksDBForReading, wantErr: "Not supported operation in secondary mode"},
	}
	for name, tt := range tests {
		t.Run(name, func(t *testing.T) {
			chainID := uint64(3)
			data := []byte(`rules update heights data`)

			path := t.TempDir()
			db, err := createDB(path)
			require.NoError(t, err)
			db.close()

			rocksDB, err := tt.dbOpener(path)
			require.NoError(t, err)
			defer func() { require.NoError(t, rocksDB.Close()) }()

			err = rocksDB.PutRulesUpdateHeights(chainID, data)
			if tt.wantErr != "" {
				require.ErrorContains(t, err, tt.wantErr)
				return
			}
			require.NoError(t, err)
			got, err := rocksDB.GetRulesUpdateHeights(chainID)
			require.NoError(t, err)
			require.Equal(t, data, got)
		})
	}
}

// writeDB is a wrapper around grocksdb.DB that provides methods to write blocks to the database.
// It is used for testing purposes to fill a database with blocks that can be queried later using the RocksDB type (which only provides an update function).
type writeDB struct {
	db *grocksdb.DB
}

func createDB(path string) (writeDB, error) {
	db, err := createDBWithoutVersion(path)
	if err != nil {
		return writeDB{}, err
	}
	if err := db.putVersion(CurrentVersion); err != nil {
		db.close()
		return writeDB{}, err
	}
	return db, nil
}

func createDBWithoutVersion(path string) (writeDB, error) {
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	defer options.Destroy()
	db, err := grocksdb.OpenDb(options, path)
	if err != nil {
		return writeDB{}, err
	}
	return writeDB{db: db}, nil
}

func (db writeDB) close() {
	if db.db != nil {
		db.db.Close()
	}
}

func (db writeDB) putRaw(key, value []byte) error {
	writeOptions := grocksdb.NewDefaultWriteOptions()
	defer writeOptions.Destroy()
	return db.db.Put(writeOptions, key, value)
}

func (db writeDB) putVersion(version uint64) error {
	versionBytes := make([]byte, 8)
	binary.BigEndian.PutUint64(versionBytes, version)
	return db.putRaw(MakeVersionKey(), versionBytes)
}

func (db writeDB) putBlock(chainID uint64, block *Block) error {
	key := MakeBlockKey(chainID, block.Number)
	data, err := proto.Marshal(block)
	if err != nil {
		return err
	}
	return db.putRaw(key, data)
}

func fillDBWithBlocks(t *testing.T, chainID uint64, blockNumbers []uint64) (string, error) {
	path := t.TempDir()

	db, err := createDB(path)
	if err != nil {
		return "", err
	}
	defer db.close()

	for _, num := range blockNumbers {
		block := Block{Number: num}
		if err := db.putBlock(chainID, &block); err != nil {
			return "", err
		}
	}

	return path, nil
}

func fillDBWithInvalidBlock(t *testing.T, chainID uint64, blockNumber uint64) (string, error) {
	path := t.TempDir()

	db, err := createDB(path)
	if err != nil {
		return "", err
	}
	defer db.close()

	key := MakeBlockKey(chainID, blockNumber)
	value := []byte{0x00} // Invalid block data, just a single byte
	if err := db.putRaw(key, value); err != nil {
		return "", err
	}

	return path, nil
}
