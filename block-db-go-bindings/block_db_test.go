package blockdb

import (
	"os"
	"testing"

	"github.com/linxGnu/grocksdb"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/proto"
)

type OpenRocksDBFunc func(path string) (RocksDB, error)

func TestOpenRocksDB(t *testing.T) {
	runTests := func(t *testing.T, runner OpenRocksDBFunc) {
		tests := map[string]func(*testing.T, OpenRocksDBFunc){
			"opens existing db":                                         opensExistingDB,
			"fails if db does not exist":                                failsIfDBDoesNotExist,
			"get returns block if it exists":                            getReturnsBlockIfItExists,
			"get returns error if block is invalid":                     getReturnsErrorIfBlockIsInvalid,
			"get returns error if block does not exist":                 getReturnsErrorIfBlockDoesNotExist,
			"get range returns existing sub range":                      getRangeReturnsExistingSubRange,
			"get range returns error if block is invalid":               getRangeReturnsErrorIfBlockIsInvalid,
			"get range rev returns existing sub range in reverse order": getRangeRevReturnsExistingSubRangeInReverseOrder,
			"get range rev returns error if block is invalid":           getRangeRevReturnsErrorIfBlockIsInvalid,
		}

		for name, test := range tests {
			t.Run(name, func(t *testing.T) {
				test(t, runner)
			})
		}
	}

	t.Run("OpenRocksDBForReading", func(t *testing.T) {
		runTests(t, OpenRocksDBForReading)
	})

	t.Run("OpenRocksDBForWriting", func(t *testing.T) {
		runTests(t, OpenRocksDBForWriting)
	})
}

func opensExistingDB(t *testing.T, dbOpener OpenRocksDBFunc) {
	path, err := os.MkdirTemp("", "blockdb-*")
	require.NoError(t, err, "failed to create temp dir")

	writeDB, err := createDB(path)
	require.NoError(t, err, "failed to create db")
	writeDB.close()

	db, err := dbOpener(path)
	require.NoError(t, err, "failed to open db")
	require.NoError(t, db.Close(), "failed to close db")
}

func failsIfDBDoesNotExist(t *testing.T, dbOpener OpenRocksDBFunc) {
	_, err := dbOpener("non-existing-db-path")
	require.Error(t, err, "opening db did not return an error although path does not exist")

}

func getReturnsBlockIfItExists(t *testing.T, dbOpener OpenRocksDBFunc) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := dbOpener(path)
	require.NoError(t, err, "failed to open db")
	defer func() {
		require.NoError(t, db.Close(), "failed to close db")
	}()

	tests := []struct {
		chainID, blockNumber uint64
	}{
		{chainID, 1}, // existing chainID and block number
		{chainID, 2}, // existing chainID and block number
		{chainID, 3}, // existing chainID and block number
	}

	for _, test := range tests {
		block, err := db.Get(test.chainID, test.blockNumber)
		require.NoError(t, err, "failed to retrieve block")
		require.NotNil(t, block, "expected block to exist for chainID %d and blockNumber %d", test.chainID, test.blockNumber)
		require.Equal(t, test.blockNumber, block.Number, "expected block number %d but got %d", test.blockNumber, block.Number)
	}
}

func getReturnsErrorIfBlockIsInvalid(t *testing.T, dbOpener OpenRocksDBFunc) {
	chainID := uint64(3)
	blockNumber := uint64(1)

	path, err := fillDBWithInvalidBlock(chainID, blockNumber)
	require.NoError(t, err, "failed to create db")

	// db now contains an invalid blocks at blocknumber 1 for chainId 3

	db, err := dbOpener(path)
	require.NoError(t, err, "failed to open db")
	defer func() {
		require.NoError(t, db.Close(), "failed to close db")
	}()

	block, err := db.Get(chainID, blockNumber)
	require.Error(t, err, "expected error when retrieving an invalid block")
	require.Nil(t, block, "expected nil block when retrieving an invalid block")
}

func getReturnsErrorIfBlockDoesNotExist(t *testing.T, dbOpener OpenRocksDBFunc) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, and 3 for chainId 3

	db, err := dbOpener(path)
	require.NoError(t, err, "failed to open db")
	defer func() {
		require.NoError(t, db.Close(), "failed to close db")
	}()

	tests := []struct {
		chainID, blockNumber uint64
	}{
		{0, 1},       // non-existing chainID
		{chainID, 0}, // non-existing block number
	}

	for _, test := range tests {
		block, err := db.Get(test.chainID, test.blockNumber)
		require.Nil(t, block, "expected nil block for chainID %d and blockNumber %d", test.chainID, test.blockNumber)
		require.NoError(t, err, "expected nil error when retrieving non-existing block")
	}
}

func getRangeReturnsExistingSubRange(t *testing.T, dbOpener OpenRocksDBFunc) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3, 20}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, 3 and 20 for chainId 3

	db, err := dbOpener(path)
	require.NoError(t, err, "failed to open db")

	tests := []struct {
		chainID, startBlockNumber, endBlockNumber uint64
		expectedBlockNumbers                      []uint64
	}{
		{0, 0, 5, []uint64{}},              // non-existing chainID
		{chainID, 5, 10, []uint64{}},       // non-existing block number
		{chainID, 0, 5, []uint64{1, 2, 3}}, // existing chainID and range which contains existing blocks
		{chainID, 0, 2, []uint64{1, 2}},    // existing chainID and range which starts before existing blocks and ends within existing blocks
		{chainID, 2, 5, []uint64{2, 3}},    // existing chainID and range which starts within existing blocks and ends after existing blocks
	}

	for _, test := range tests {
		i := 0
		for block, err := range db.GetRange(test.chainID, test.startBlockNumber, test.endBlockNumber) {
			require.NoError(t, err, "expected nil error when retrieving a block")
			require.NotNil(t, block, "expected block to exist")
			require.Equal(t, test.expectedBlockNumbers[i], block.Number, "unexpected block number at index %d", i)
			i++
		}
		require.Equal(t, len(test.expectedBlockNumbers), i, "expected %d blocks, got %d", len(test.expectedBlockNumbers), i)
	}
}

func getRangeReturnsErrorIfBlockIsInvalid(t *testing.T, dbOpener OpenRocksDBFunc) {
	chainID := uint64(3)
	blockNumber := uint64(1)

	path, err := fillDBWithInvalidBlock(chainID, blockNumber)
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

}
func getRangeRevReturnsExistingSubRangeInReverseOrder(t *testing.T, dbOpener OpenRocksDBFunc) {
	chainID := uint64(3)
	blockNumbers := []uint64{1, 2, 3, 20}

	path, err := fillDBWithBlocks(chainID, blockNumbers)
	require.NoError(t, err, "failed to create db")

	// db now contains blocks 1, 2, 3 and 20 for chainId 3

	db, err := dbOpener(path)
	require.NoError(t, err, "failed to open db")

	tests := []struct {
		chainID, startBlockNumber, endBlockNumber uint64
		expectedBlockNumbers                      []uint64
	}{
		{0, 0, 5, []uint64{}},              // non-existing chainID
		{chainID, 5, 10, []uint64{}},       // non-existing block number
		{chainID, 0, 5, []uint64{3, 2, 1}}, // existing chainID and range which contains existing blocks
		{chainID, 0, 2, []uint64{2, 1}},    // existing chainID and range which starts before existing blocks and ends within existing blocks
		{chainID, 2, 5, []uint64{3, 2}},    // existing chainID and range which starts within existing blocks and ends after existing blocks
	}

	for _, test := range tests {
		i := 0
		for block, err := range db.GetRangeRev(test.chainID, test.startBlockNumber, test.endBlockNumber) {
			require.NoError(t, err, "expected nil error when retrieving a block")
			require.NotNil(t, block, "expected block to exist")
			require.Equal(t, test.expectedBlockNumbers[i], block.Number, "unexpected block number at index %d", i)
			i++
		}
		require.Equal(t, len(test.expectedBlockNumbers), i, "expected %d blocks, got %d", len(test.expectedBlockNumbers), i)
	}
}

func getRangeRevReturnsErrorIfBlockIsInvalid(t *testing.T, dbOpener OpenRocksDBFunc) {
	chainID := uint64(3)
	blockNumber := uint64(1)

	path, err := fillDBWithInvalidBlock(chainID, blockNumber)
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

}

func TestRocksDB_Update_CreatesNewBlock(t *testing.T) {
	path, err := os.MkdirTemp("", "blockdb-*")
	require.NoError(t, err, "failed to create temp dir")

	writeDB, err := createDB(path)
	require.NoError(t, err, "failed to create db")
	writeDB.close()

	db, err := OpenRocksDBForWriting(path)
	require.NoError(t, err, "failed to open db")
	defer func() {
		require.NoError(t, db.Close(), "failed to close db")
	}()

	retrievedBlock, err := db.Get(1, 1)
	require.NoError(t, err, "failed to get block from db")
	require.Nil(t, retrievedBlock, "retrieved block is not nil")

	block := &Block{Number: 1}
	require.NoError(t, db.Update(1, block))
	retrievedBlock, err = db.Get(1, 1)
	require.NoError(t, err, "failed to get block from db")
	require.NotNil(t, retrievedBlock, "retrieved block is nil")
	require.Equal(t, block.Number, retrievedBlock.Number, "retrieved block number does not match")
}

func TestRocksDB_Update_OverwritesExistingBlock(t *testing.T) {
	path, err := os.MkdirTemp("", "blockdb-*")
	require.NoError(t, err, "failed to create temp dir")

	writeDB, err := createDB(path)
	require.NoError(t, err, "failed to create db")
	writeDB.close()

	db, err := OpenRocksDBForWriting(path)
	require.NoError(t, err, "failed to open db")
	defer func() {
		require.NoError(t, db.Close(), "failed to close db")
	}()

	block := &Block{Number: 1, StateRoot: []byte{0x1, 0x2}}
	require.NoError(t, db.Update(1, block))
	retrievedBlock, err := db.Get(1, 1)
	require.NoError(t, err, "failed to get block from db")
	require.NotNil(t, retrievedBlock, "retrieved block is nil")
	require.Equal(t, block.Number, retrievedBlock.Number, "retrieved block number does not match")
	require.Equal(t, block.StateRoot, retrievedBlock.StateRoot, "retrieved block state root does not match")

	// Overwrite existing block
	updatedBlock := &Block{Number: 1, StateRoot: []byte{0x3, 0x4}}
	require.NoError(t, db.Update(1, updatedBlock))
	retrievedBlock, err = db.Get(1, 1)
	require.NoError(t, err, "failed to get block from db")
	require.NotNil(t, retrievedBlock, "retrieved block is nil")
	require.Equal(t, updatedBlock.Number, retrievedBlock.Number, "retrieved block number does not match after update")
	require.Equal(t, updatedBlock.StateRoot, retrievedBlock.StateRoot, "retrieved block state root does not match after update")
}

// writeDB is a wrapper around grocksdb.DB that provides methods to write blocks to the database.
// It is used for testing purposes to fill a database with blocks that can be queried later using the RocksDB type (which only provides an update function).
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

func fillDBWithInvalidBlock(chainID uint64, blockNumber uint64) (string, error) {
	path, err := os.MkdirTemp("", "blockdb-*")
	if err != nil {
		return "", err
	}

	db, err := createDB(path)
	if err != nil {
		return "", err
	}
	defer db.close()

	key := computeKey(chainID, blockNumber)
	value := []byte{0x00} // Invalid block data, just a single byte
	if err := db.putRaw(key, value); err != nil {
		return "", err
	}

	return path, nil
}
