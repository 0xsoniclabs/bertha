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

// DB is a handle to the block database which can be used for point and range queries.
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

func computeKey(chainID, blockNumber uint64) []byte {
	key := make([]byte, 16)
	binary.BigEndian.PutUint64(key[:8], chainID)
	binary.BigEndian.PutUint64(key[8:], blockNumber)
	return key
}

// GetBlock retrieves a single block by chain ID and block number.
// If the block does not exist, it returns nil.
func (db DB) GetBlock(chainID, blockNumber uint64) (*Block, error) {
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
// If parsing the block fails, the block will be nil and an error is returned.
// The iterator needs to be used in a range loop because otherwise the inner iterator will not be closed properly.
func (db DB) GetBlocks(chainID, startBlockNumber, endBlockNumber uint64) iter.Seq2[*Block, error] {
	startKey := computeKey(chainID, startBlockNumber)
	endKey := computeKey(chainID, endBlockNumber)

	readOptions := grocksdb.NewDefaultReadOptions()
	it := db.db.NewIterator(readOptions)
	it.Seek(startKey)

	return func(yield func(*Block, error) bool) {
		defer it.Close()
		for it.Valid() {
			key := it.Key().Data()
			// Stop if we reach a key that has a different chain ID
			if !slices.Equal(key[:8], startKey[:8]) {
				break
			}
			value := it.Value().Data()
			block := &Block{}
			err := proto.Unmarshal(value, block)
			if err != nil {
				block = nil
			}
			if !yield(block, err) {
				break
			}
			// Stop if we reach the end key
			if slices.Equal(key, endKey) {
				break
			}
			it.Next()
		}
	}
}
