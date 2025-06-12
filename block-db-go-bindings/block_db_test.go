package blockdb

import (
	"os"
	"testing"

	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/proto"
)

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

func TestGetBlocksReturnsBlockIfItExists(t *testing.T) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := OpenDB(path)
	require.NoError(t, err, "failed to open db")
	defer func() {
		err = db.Close()
		require.NoError(t, err, "failed to close db")
	}()

	// try to get block for non-existing chainId
	block, _ := db.GetBlock(0, 0)
	require.Nil(t, block, "got block but db contains no blocks for this chain id")

	// try to get block with non-existing block number
	block, _ = db.GetBlock(chainID, 4)
	require.Nil(t, block, "got block but db contains no blocks with block number 4")

	// get existing block
	block, err = db.GetBlock(chainID, 1)
	require.NoError(t, err, "failed to retrieve block")
	require.NotNil(t, block, "expected block to exist, but got nil")
	require.Equal(t, block.Number, uint64(1))
}

func TestGetBlocksReturnsExistingSubRange(t *testing.T) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := OpenDB(path)
	require.NoError(t, err, "failed to open db")

	// get blocks for non-existing chainId
	count := 0
	for range db.GetBlocks(0, 0, 10) {
		count++
	}
	require.Equal(t, 0, count, "got blocks for non existing chain id")

	// get blocks for range outside of existing blocks
	count = 0
	for range db.GetBlocks(chainID, 1000, 1005) {
		count++
	}
	require.Equal(t, 0, count, "got blocks for range outside of existing blocks")

	// get blocks for for range which contains existing blocks
	count = 0
	for range db.GetBlocks(chainID, 0, 6) {
		count++
	}
	require.Equal(t, 3, count, "got invalid number of blocks")

	// get blocks for range which starts before existing blocks
	// and ends within existing blocks
	count = 0
	for range db.GetBlocks(chainID, 0, 2) {
		count++
	}
	require.Equal(t, 2, count, "got invalid number of blocks")

	// get blocks for range which starts within existing blocks
	// and ends after existing blocks
	count = 0
	for range db.GetBlocks(chainID, 2, 6) {
		count++
	}
	require.Equal(t, 2, count, "got invalid number of blocks")
}
