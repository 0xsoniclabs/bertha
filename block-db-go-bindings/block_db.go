package block_db

import (
	"encoding/binary"
	"iter"
	"os"
	"slices"

	"github.com/linxGnu/grocksdb"
	"google.golang.org/protobuf/proto"
)

type Db struct {
	db            *grocksdb.DB
	secondaryPath string
}

// Open the database for reading.
func OpenDb(path string) (Db, error) {
	secondaryPath, err := os.MkdirTemp("", "blockdb-secondary-*")
	if err != nil {
		return Db{}, err
	}
	options := grocksdb.NewDefaultOptions()
	db, err := grocksdb.OpenDbAsSecondary(options, path, secondaryPath)
	if err != nil {
		return Db{}, err
	}
	return Db{db: db, secondaryPath: secondaryPath}, nil
}

// Clone the database.
func (db Db) Close() error {
	if db.db != nil {
		db.db.Close()
	}
	if db.secondaryPath != "" {
		if err := os.RemoveAll(db.secondaryPath); err != nil {
			return err
		}
	}
	return nil
}

func computeKey(chainId uint64, blockNumber uint64) []byte {
	key := make([]byte, 16)
	binary.BigEndian.PutUint64(key[:8], chainId)
	binary.BigEndian.PutUint64(key[8:], blockNumber)
	return key
}

// Retrieve a single block by chain ID and block number.
// If the block does not exist, it returns nil.
func (db Db) GetBlock(chainId uint64, blockNumber uint64) (*Block, error) {
	key := computeKey(chainId, blockNumber)

	readOptions := grocksdb.NewDefaultReadOptions()
	defer readOptions.Destroy()
	value, err := db.db.GetBytes(readOptions, key)
	if err != nil {
		return nil, err
	}
	if value == nil {
		return nil, nil
	}

	var block Block
	if err := proto.Unmarshal(value, &block); err != nil {
		return nil, err
	}

	return &block, nil
}

// Retrieve multiple block by chain ID and block number.
// This function returns an iterator that yields blocks in the specified range.
// If there are no blocks in the range in the database, the iterator will not yield any blocks.
func (db Db) GetBlocks(chainId uint64, startBlockNumber uint64, endBlockNumber uint64) iter.Seq[*Block] {
	startKey := computeKey(chainId, startBlockNumber)
	endKey := computeKey(chainId, endBlockNumber)

	readOptions := grocksdb.NewDefaultReadOptions()
	it := db.db.NewIterator(readOptions)
	it.Seek(startKey)

	stop := false

	return func(yield func(*Block) bool) {
		for it.Valid() && !stop {
			key := it.Key().Data()
			if !slices.Equal(key[:8], startKey[:8]) {
				break
			}
			if slices.Equal(key, endKey) {
				stop = true
			}
			value := it.Value().Data()
			var block Block
			if err := proto.Unmarshal(value, &block); err != nil {
				break
			}
			if !yield(&block) {
				break
			}
			it.Next()
		}
		it.Close()
	}
}

// for testing purposes

type writeDb struct {
	db *grocksdb.DB
}

func createDb(path string) (writeDb, error) {
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(true)
	db, err := grocksdb.OpenDb(options, path)
	if err != nil {
		return writeDb{}, err
	}
	return writeDb{db: db}, nil
}

func (db writeDb) close() {
	if db.db != nil {
		db.db.Close()
	}
}

func (db writeDb) putRaw(key []byte, value []byte) error {
	writeOptions := grocksdb.NewDefaultWriteOptions()
	defer writeOptions.Destroy()
	return db.db.Put(writeOptions, key, value)
}

func (db writeDb) putBlock(chainId uint64, block *Block) error {
	key := computeKey(chainId, block.Number)
	data, err := proto.Marshal(block)
	if err != nil {
		return err
	}
	return db.putRaw(key, data)
}
