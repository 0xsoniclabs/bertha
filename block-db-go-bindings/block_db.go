// Package blockdb provides bindings for interacting with a RocksDB-backed block database.
// It supports opening the database in secondary (read-only) mode, retrieving individual blocks
// or ranges of blocks by chain ID and block number.
package blockdb

import (
	"encoding/binary"
	"iter"
	"os"
	"slices"

	"github.com/linxGnu/grocksdb"
	"google.golang.org/protobuf/proto"
)

type DB struct {
	db            *grocksdb.DB
	secondaryPath string
}

// OpenDB opens the database for reading.
func OpenDB(path string) (DB, error) {
	secondaryPath, err := os.MkdirTemp("", "blockdb-secondary-*")
	if err != nil {
		return DB{}, err
	}
	options := grocksdb.NewDefaultOptions()
	db, err := grocksdb.OpenDbAsSecondary(options, path, secondaryPath)
	if err != nil {
		return DB{}, err
	}
	return DB{db: db, secondaryPath: secondaryPath}, nil
}

// Close closes the database.
func (db DB) Close() error {
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

func computeKey(chainID uint64, blockNumber uint64) []byte {
	key := make([]byte, 16)
	binary.BigEndian.PutUint64(key[:8], chainID)
	binary.BigEndian.PutUint64(key[8:], blockNumber)
	return key
}

// GetBlock retrieves a single block by chain ID and block number.
// If the block does not exist, it returns nil.
func (db DB) GetBlock(chainID uint64, blockNumber uint64) (*Block, error) {
	key := computeKey(chainID, blockNumber)

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

// GetBlocks retrieves multiple block by chain ID and block number.
// This function returns an iterator that yields blocks in the specified range.
// If there are no blocks in the range in the database, the iterator will not yield any blocks.
func (db DB) GetBlocks(chainID uint64, startBlockNumber uint64, endBlockNumber uint64) iter.Seq[*Block] {
	startKey := computeKey(chainID, startBlockNumber)
	endKey := computeKey(chainID, endBlockNumber)

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

func (db writeDB) putRaw(key []byte, value []byte) error {
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
