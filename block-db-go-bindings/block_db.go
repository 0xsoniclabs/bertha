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

//go:generate mockgen -source=block_db.go -destination=block_db_mock.go -package=blockdb

// BlockDb is an interface to the block database which can be used for point and range queries.
type BlockDb interface {
	Get(chainID, blockNumber uint64) (*Block, error)
	Update(chainID uint64, block *Block) error
	GetRange(chainID, startBlockNumber, endBlockNumber uint64) iter.Seq2[*Block, error]
	GetRangeRev(chainID, startBlockNumber, endBlockNumber uint64) iter.Seq2[*Block, error]
	Close() error
}

// RocksDB wraps a rocksDB database and provides the `BlockDb` interface.
type RocksDB struct {
	db            *grocksdb.DB
	secondaryPath string
}

// OpenRocksDBForWriting opens the database.
func OpenRocksDBForWriting(path string) (RocksDB, error) {
	options := grocksdb.NewDefaultOptions()
	options.SetCreateIfMissing(false)
	db, err := grocksdb.OpenDb(options, path)
	if err != nil {
		return RocksDB{}, err
	}
	return RocksDB{db: db}, nil
}

// OpenRocksDBForReading opens the database for reading.
func OpenRocksDBForReading(path string) (RocksDB, error) {
	secondaryPath, err := os.MkdirTemp("", "blockdb-secondary-*")
	if err != nil {
		return RocksDB{}, err
	}
	options := grocksdb.NewDefaultOptions()
	db, err := grocksdb.OpenDbAsSecondary(options, path, secondaryPath)
	if err != nil {
		return RocksDB{}, err
	}
	return RocksDB{db: db, secondaryPath: secondaryPath}, nil
}

// Close closes the database.
func (db RocksDB) Close() error {
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

// Get retrieves a single block by chain ID and block number.
// If the block does not exist, it returns nil.
func (db RocksDB) Get(chainID, blockNumber uint64) (*Block, error) {
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

// Update inserts or updates a block by chain ID and block number.
func (db RocksDB) Update(chainID uint64, block *Block) error {
	key := computeKey(chainID, block.Number)

	writeOptions := grocksdb.NewDefaultWriteOptions()
	defer writeOptions.Destroy()

	value, err := proto.Marshal(block)
	if err != nil {
		return err
	}
	return db.db.Put(writeOptions, key, value)
}

// GetRange retrieves multiple blocks by chain ID and block number.
// This function returns an iterator that yields blocks in the specified range.
// If there are no blocks in the range in the database, the iterator will not yield any blocks.
// If parsing the block fails, the block will be nil and an error is returned.
// The iterator needs to be used in a range loop because otherwise the inner iterator will not be closed properly.
func (db RocksDB) GetRange(chainID, startBlockNumber, endBlockNumber uint64) iter.Seq2[*Block, error] {
	startKey := computeKey(chainID, startBlockNumber)

	readOptions := grocksdb.NewDefaultReadOptions()
	it := db.db.NewIterator(readOptions)
	it.Seek(startKey)

	return func(yield func(*Block, error) bool) {
		defer it.Close()
		for it.Valid() {
			key := it.Key().Data()
			// Stop if we reach a key that has a different chain ID or a key number that is greater than the end block number
			keyNum := binary.BigEndian.Uint64(key[8:])
			if !slices.Equal(key[:8], startKey[:8]) || keyNum > endBlockNumber {
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
			it.Next()
		}
	}
}

// GetRangeRev retrieves multiple blocks by chain ID and block number in reverse order.
// This function returns an iterator that yields blocks in the specified range.
// If there are no blocks in the range in the database, the iterator will not yield any blocks.
// If parsing the block fails, the block will be nil and an error is returned.
// The iterator needs to be used in a range loop because otherwise the inner iterator will not be closed properly.
func (db RocksDB) GetRangeRev(chainID, startBlockNumber, endBlockNumber uint64) iter.Seq2[*Block, error] {
	endKey := computeKey(chainID, endBlockNumber)

	readOptions := grocksdb.NewDefaultReadOptions()
	it := db.db.NewIterator(readOptions)
	it.SeekForPrev(endKey)

	return func(yield func(*Block, error) bool) {
		defer it.Close()
		for it.Valid() {
			key := it.Key().Data()
			// Stop if we reach a key that has a different chain ID or a key number that is less than the start block number
			keyNum := binary.BigEndian.Uint64(key[8:])
			if !slices.Equal(key[:8], endKey[:8]) || keyNum < startBlockNumber {
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
			it.Prev()
		}
	}
}
