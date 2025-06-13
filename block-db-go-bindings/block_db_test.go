package blockdb

import (
	"os"
	"testing"

	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/proto"
)

func TestGetBlocksReturnsBlockIfItExists(t *testing.T) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := OpenDB(path)
	require.NoError(t, err, "failed to open db")
	defer func() {
		require.NoError(t, db.Close(), "failed to close db")
	}()

	tests := []struct {
		chainID, blockNumber uint64
		expectedBlock        *Block
	}{
		{0, 1, nil},                     // non-existing chainID
		{chainID, 4, nil},               // non-existing block number
		{chainID, 1, &Block{Number: 1}}, // existing chainID and block number
	}

	for _, test := range tests {
		block, err := db.GetBlock(test.chainID, test.blockNumber)
		require.NoError(t, err, "failed to retrieve block")
		if test.expectedBlock == nil {
			require.Nil(t, block, "expected nil block for chainID %d and blockNumber %d", test.chainID, test.blockNumber)
		} else {
			require.NotNil(t, block, "expected block to exist for chainID %d and blockNumber %d", test.chainID, test.blockNumber)
			require.Equal(t, test.expectedBlock.Number, block.Number, "expected block number %d but got %d", test.expectedBlock.Number, block.Number)
		}
	}
}

func TestGetBlocksReturnsExistingSubRange(t *testing.T) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := OpenDB(path)
	require.NoError(t, err, "failed to open db")

	tests := []struct {
		chainID, startBlockNumber, endBlockNumber uint64
		expectedBlockCount                        int
	}{
		{0, 0, 5, 0},        // non-existing chainID
		{chainID, 5, 10, 0}, // non-existing block number
		{chainID, 0, 5, 3},  // existing chainID and range which contains existing blocks
		{chainID, 0, 2, 2},  // existing chainID and range which starts before existing blocks and ends within existing blocks
		{chainID, 2, 5, 2},  // existing chainID and range which starts within existing blocks and ends after existing blocks
	}

	for _, test := range tests {
		count := 0
		for range db.GetBlocks(test.chainID, test.startBlockNumber, test.endBlockNumber) {
			count++
		}
		require.Equal(t, test.expectedBlockCount, count, "expected %d blocks for chainID %d from %d to %d, got %d",
			test.expectedBlockCount, test.chainID, test.startBlockNumber, test.endBlockNumber, count)
	}
}

// writeDB is a wrapper around grocksdb.DB that provides methods to write blocks to the database.
// It is used for testing purposes to fill a database with blocks that can be queried later using the DB type (which only provides read access).
type writeDB struct {
	db *grocksdb.DB
}

func createDB(path string) (writeDB, error) {
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
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

func (db writeDB) putBlock(chainID uint64, block *Block) error {
	key := computeKey(chainID, block.Number)
	data, err := proto.Marshal(block)
	if err != nil {
		return err
	}
	return db.putRaw(key, data)
}

func fillDBWithBlocks(chainID uint64, blockNumbers []uint64) (string, error) {
	path, err := os.MkdirTemp("", "blockdb-*")
	if err != nil {
		return "", err
	}

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
