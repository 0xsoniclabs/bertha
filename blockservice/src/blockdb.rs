use std::path::Path;

use bertha_types::Block;
use prost::Message;
use tempfile::TempDir;

use crate::{error::Error, proto};

impl From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Self {
        Error::StorageLayer(e.to_string())
    }
}

/// A database that allows to store and and query [Block]s for multiple different blockchains.
/// Blocks are encoded as protobuf messages before being stored in the database.
/// As database operations may fail for various reasons, all methods return a [Result].
pub trait BlockDb {
    /// Retrieves a block for the specified chain-ID and block number.
    /// Returns [None] if the block does not exist.
    fn get(&self, chain_id: u64, block_number: u64) -> Result<Option<Block>, Error> {
        match self.get_raw(chain_id, block_number)? {
            Some(data) => Ok(Some(Block::try_from(
                proto::Block::decode(data.as_slice()).map_err(Error::Protobuf)?,
            )?)),
            None => Ok(None),
        }
    }

    /// Stores a block for the specified chain-ID.
    /// The block number is obtained from the block itself.
    fn put(&mut self, chain_id: u64, block: Block) -> Result<(), Error> {
        let number = block.number;
        let data = proto::Block::from(block).encode_to_vec();
        self.put_raw(chain_id, number, &data)
    }

    /// Iterates over all blocks for the specified chain-ID starting from the given block number.
    /// The sequence of blocks is ordered by block number and may contain gaps for missing
    /// blocks.
    fn iterate(&self, chain_id: u64, from: u64) -> impl Iterator<Item = Result<Block, Error>> {
        self.iterate_raw(chain_id, from).map(|result| {
            result.and_then(|(_, data)| {
                let block = proto::Block::decode(data.as_ref()).map_err(Error::Protobuf)?;
                Block::try_from(block)
            })
        })
    }

    /// Like `iterate`, but iterates in reverse order.
    fn iterate_reverse(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<Block, Error>> {
        self.iterate_raw_reverse(chain_id, from).map(|result| {
            result.and_then(|(_, data)| {
                let block = proto::Block::decode(data.as_ref()).map_err(Error::Protobuf)?;
                Block::try_from(block)
            })
        })
    }

    /// Retrieves the raw protobuf-encoded data for the specified chain-ID and block number.
    /// Returns [None] if the block does not exist.
    fn get_raw(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error>;

    /// Stores the raw protobuf-encoded data for the specified chain-ID and block number.
    fn put_raw(&mut self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error>;

    /// Iterates over raw protobuf-encoded blocks for the specified chain-ID starting from the given
    /// block number.
    /// Returns an iterator that yields tuples of (block number, data).
    /// The sequence of blocks is ordered by block number and may contain gaps for missing
    /// blocks.
    fn iterate_raw(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>>;

    /// Like `iterate_raw`, but iterates in reverse order.
    fn iterate_raw_reverse(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>>;
}

/// A block database using RocksDB as its storage layer.
#[derive(Debug)]
pub struct RocksBlockDb {
    db: rocksdb::DB,

    /// Path of the secondary instance, if opened for reading.
    _secondary_path: Option<TempDir>,
}

impl RocksBlockDb {
    /// Creates a new RocksDB block database at the specified path.
    /// Returns an error if the database already exists.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut db_opts = Self::make_options();
        db_opts.set_error_if_exists(true);
        db_opts.create_if_missing(true);
        Ok(Self {
            db: rocksdb::DB::open(&db_opts, path).map_err(Error::from)?,
            _secondary_path: None,
        })
    }

    /// Opens an existing RocksDB database for reading and writing.
    /// Returns an error if the database does not exist or is already opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut db_opts = Self::make_options();
        db_opts.create_if_missing(false);
        Ok(Self {
            db: rocksdb::DB::open(&Self::make_options(), path).map_err(Error::from)?,
            _secondary_path: None,
        })
    }

    /// Opens an existing RocksDB database for reading only.
    /// Multiple instances can be opened in this mode, while at most one instance can simultaneously
    /// be opened for writing.
    /// Returns an error if the database does not exist.
    pub fn open_for_reading(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut db_opts = Self::make_options();
        // Opening a secondary instance requires set_max_open_files to be set to -1 (the default).
        // See https://github.com/facebook/rocksdb/blob/2dcfc5475276a524be692ab08afdc831def81066/include/rocksdb/db.h
        db_opts.set_max_open_files(-1);
        let secondary_path = tempfile::tempdir().map_err(|e| Error::StorageLayer(e.to_string()))?;
        Ok(Self {
            db: rocksdb::DB::open_as_secondary(&db_opts, path.as_ref(), secondary_path.path())
                .map_err(Error::from)?,
            // This will be removed automatically when the RocksBlockDb instance is dropped.
            _secondary_path: Some(secondary_path),
        })
    }

    /// Creates a baseline RocksDB options object which can then be further customized depending on
    /// the use case.
    fn make_options() -> rocksdb::Options {
        let mut opts = rocksdb::Options::default();
        // We use LZ4 compression for all levels, as recommended by the [RocksDB FAQ](https://github.com/facebook/rocksdb/wiki/rocksdb-faq).
        // TODO: Consider using ZStandard for bottommost layer if disk space is a concern.
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        opts
    }

    fn make_key(chain_id: u64, block_number: u64) -> [u8; 16] {
        let mut key = [0u8; 16];
        key[0..8].copy_from_slice(&chain_id.to_be_bytes());
        key[8..16].copy_from_slice(&block_number.to_be_bytes());
        key
    }

    fn iterate_raw_dir(
        &self,
        chain_id: u64,
        from: u64,
        direction: rocksdb::Direction,
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
        self.db
            .iterator(rocksdb::IteratorMode::From(
                &Self::make_key(chain_id, from),
                direction,
            ))
            .map(|result| {
                result.map_err(Error::from).and_then(|(key, value)| {
                    if key.len() != 16 {
                        return Err(Error::StorageLayer("unexpected key length".to_string()));
                    }
                    let chain_id = u64::from_be_bytes(key[0..8].try_into().unwrap());
                    let block_number = u64::from_be_bytes(key[8..16].try_into().unwrap());
                    Ok((chain_id, block_number, value))
                })
            })
            .take_while(move |result| match result {
                Ok((cid, _, _)) => *cid == chain_id,
                Err(_) => true,
            })
            .map(|result| result.map(|(_, block_number, value)| (block_number, value)))
    }
}

impl BlockDb for RocksBlockDb {
    fn get_raw(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error> {
        self.db
            .get(Self::make_key(chain_id, block_number))
            .map_err(|e| Error::StorageLayer(e.to_string()))
    }

    fn put_raw(&mut self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
        self.db
            .put(Self::make_key(chain_id, block_number), data)
            .map_err(|e| Error::StorageLayer(e.to_string()))
    }

    fn iterate_raw(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
        self.iterate_raw_dir(chain_id, from, rocksdb::Direction::Forward)
    }

    fn iterate_raw_reverse(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
        self.iterate_raw_dir(chain_id, from, rocksdb::Direction::Reverse)
    }
}

#[cfg(test)]
mod tests {
    use bertha_types::{Hash, HexConvert};

    use super::*;
    use crate::proto;

    struct StubDb(Vec<u8>);
    impl BlockDb for StubDb {
        fn get_raw(&self, _chain_id: u64, _block_number: u64) -> Result<Option<Vec<u8>>, Error> {
            Ok(Some(self.0.clone()))
        }

        fn put_raw(
            &mut self,
            _chain_id: u64,
            _block_number: u64,
            data: &[u8],
        ) -> Result<(), Error> {
            self.0 = data.to_vec();
            Ok(())
        }

        fn iterate_raw(
            &self,
            _chain_id: u64,
            _from: u64,
        ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
            std::iter::once(Ok((0, self.0.clone().into_boxed_slice())))
        }

        fn iterate_raw_reverse(
            &self,
            _chain_id: u64,
            _from: u64,
        ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
            std::iter::once(Ok((0, self.0.clone().into_boxed_slice())))
        }
    }

    #[test]
    fn blockdb_get_converts_from_protobuf() {
        let block = Block {
            number: 123,
            parent_hash: Hash::try_from_hex(
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            )
            .unwrap(),
            ..Block::default()
        };

        let db = StubDb(proto::Block::from(block.clone()).encode_to_vec());
        let received = db.get(0, 0).unwrap().unwrap();
        assert_eq!(received, block);
    }

    #[test]
    fn blockdb_get_returns_error_for_invalid_protobuf() {
        let db = StubDb(vec![0, 1, 2, 3]);
        let result = db.get(0, 0);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));
    }

    #[test]
    fn blockdb_put_converts_to_protobuf() {
        let mut db = StubDb(vec![]);
        let block = Block {
            number: 123,
            parent_hash: Hash::try_from_hex(
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            )
            .unwrap(),
            ..Block::default()
        };
        db.put(0, block.clone()).unwrap();
        assert_eq!(proto::Block::decode(db.0.as_slice()).unwrap(), block.into());
    }

    #[test]
    fn blockdb_iterate_converts_from_protobuf() {
        let mut db = StubDb(vec![]);
        let block = Block {
            number: 123,
            parent_hash: Hash::try_from_hex(
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            )
            .unwrap(),
            ..Block::default()
        };
        db.put(0, block.clone()).unwrap();

        // Forward
        let mut iter = db.iterate(0, 0);
        let received = iter.next().unwrap().unwrap();
        assert_eq!(received, block);

        // Reverse
        let mut iter = db.iterate_reverse(0, 0);
        let received = iter.next().unwrap().unwrap();
        assert_eq!(received, block);
    }

    #[test]
    fn blockdb_iterate_returns_error_for_invalid_protobuf() {
        let db = StubDb(vec![0, 1, 2, 3]);

        // Forward
        let mut iter = db.iterate(0, 0);
        let result = iter.next().unwrap();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));

        // Reverse
        let mut iter = db.iterate_reverse(0, 0);
        let result = iter.next().unwrap();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));
    }

    #[test]
    fn rocksblockdb_create_creates_new_db() {
        let tmpdir = tempfile::tempdir().unwrap();
        RocksBlockDb::create(tmpdir.path()).unwrap();
        let lock_file = tmpdir.path().join("LOCK");
        assert!(lock_file.exists(), "RocksDB LOCK file should exist");
    }

    #[test]
    fn rocksblockdb_create_returns_error_if_db_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
        RocksBlockDb::create(tmpdir.path()).unwrap();
        let result = RocksBlockDb::create(tmpdir.path());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "error in underlying storage layer: Invalid argument: {}: exists (error_if_exists is true)",
                tmpdir.path().display()
            )
        );
    }

    #[test]
    fn rocksblockdb_open_opens_existing_db_for_reading_and_writing() {
        let tmpdir = tempfile::tempdir().unwrap();
        {
            RocksBlockDb::create(tmpdir.path()).unwrap();
        }
        let db = RocksBlockDb::open(tmpdir.path()).unwrap();
        db.db.put(b"foo", b"bar").unwrap();
        let value = db.db.get(b"foo").unwrap().unwrap();
        assert_eq!(value, b"bar");
    }

    #[test]
    fn rocksblockdb_open_returns_error_if_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let result = RocksBlockDb::open(tmpdir.path());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "error in underlying storage layer: Invalid argument: {}/CURRENT: does not exist (create_if_missing is false)",
                tmpdir.path().display()
            )
        );
    }

    #[test]
    fn rocksblockdb_open_returns_error_if_db_already_opened() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let result = RocksBlockDb::open(tmpdir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No locks available")
        );
    }

    #[test]
    fn rocksblockdb_open_for_reading_opens_existing_db_for_reading() {
        let tmpdir = tempfile::tempdir().unwrap();
        {
            let db = RocksBlockDb::create(tmpdir.path()).unwrap();
            db.db.put(b"foo", b"bar").unwrap();
        }
        let db = RocksBlockDb::open_for_reading(tmpdir.path()).unwrap();
        let value = db.db.get(b"foo").unwrap().unwrap();
        assert_eq!(value, b"bar");
    }

    #[test]
    fn rocksblockdb_removes_secondary_path_on_drop() {
        let tmpdir = tempfile::tempdir().unwrap();
        {
            RocksBlockDb::create(tmpdir.path()).unwrap();
        }
        let secondary_path;
        {
            let db = RocksBlockDb::open_for_reading(tmpdir.path()).unwrap();
            assert!(db._secondary_path.is_some());
            secondary_path = db._secondary_path.as_ref().unwrap().path().to_owned();
            assert!(secondary_path.exists());
        }
        assert!(!secondary_path.exists());
    }

    #[test]
    fn rocksblockdb_open_for_reading_returns_error_if_db_does_not_exist() {
        let tmpdir = tempfile::tempdir().unwrap();
        let result = RocksBlockDb::open_for_reading(tmpdir.path());
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "error in underlying storage layer: IO error: No such file or directory: While opening a file for sequentially reading: {}/CURRENT: No such file or directory",
                tmpdir.path().display()
            )
        );
    }

    #[test]
    fn rocksblockdb_can_be_opened_multiple_times_for_reading() {
        let tmpdir = tempfile::tempdir().unwrap();
        {
            let db = RocksBlockDb::create(tmpdir.path()).unwrap();
            db.db.put(b"foo", b"bar").unwrap();
        }
        let db1 = RocksBlockDb::open_for_reading(tmpdir.path()).unwrap();
        let db2 = RocksBlockDb::open_for_reading(tmpdir.path()).unwrap();
        let value = db1.db.get(b"foo").unwrap().unwrap();
        assert_eq!(value, b"bar");
        let value = db2.db.get(b"foo").unwrap().unwrap();
        assert_eq!(value, b"bar");
    }

    #[test]
    fn rocksblockdb_can_be_opened_for_reading_and_writing_concurrently() {
        let tmpdir = tempfile::tempdir().unwrap();
        let write_db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let read_db = RocksBlockDb::open_for_reading(tmpdir.path()).unwrap();
        write_db.db.put(b"foo", b"bar").unwrap();
        read_db.db.try_catch_up_with_primary().unwrap();
        let value = read_db.db.get(b"foo").unwrap().unwrap();
        assert_eq!(value, b"bar");
    }

    #[test]
    fn rocksblockdb_put_raw_writes_raw_data() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let chain_id = 1;
        let block_number = 42;
        let data = b"test data";

        db.put_raw(chain_id, block_number, data).unwrap();
        let key = RocksBlockDb::make_key(chain_id, block_number);
        let value = db.db.get(key).unwrap().unwrap();
        assert_eq!(value, data);
    }

    #[test]
    fn rocksblockdb_put_raw_fails_if_db_opened_as_read_only() {
        let tmpdir = tempfile::tempdir().unwrap();
        {
            RocksBlockDb::create(tmpdir.path()).unwrap();
        }
        let mut db = RocksBlockDb::open_for_reading(tmpdir.path()).unwrap();
        let result = db.put_raw(1, 42, b"test data");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "error in underlying storage layer: Not implemented: Not supported operation in secondary mode."
        );
    }

    #[test]
    fn rocksblockdb_get_raw_returns_raw_data() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let chain_id = 1;
        let block_number = 42;
        let data = b"test data";
        let key = RocksBlockDb::make_key(chain_id, block_number);
        db.db.put(key, data).unwrap();

        let result = db.get_raw(chain_id, block_number).unwrap();
        assert_eq!(result, Some(data.to_vec()));
    }

    #[test]
    fn rocksblockdb_iterate_raw_returns_blocks_for_single_chain_in_order() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();

        db.put_raw(0, 0, b"block0-0").unwrap();
        db.put_raw(7, 2, b"block7-2").unwrap();
        db.put_raw(7, 3, b"block7-3").unwrap();
        db.put_raw(7, 6, b"block7-6").unwrap(); // insert out of order
        db.put_raw(7, 4, b"block7-4").unwrap();
        db.put_raw(9, 1, b"block9-1").unwrap();

        // Forward
        let blocks: Vec<_> = db.iterate_raw(7, 3).collect::<Result<_, _>>().unwrap();
        assert_eq!(
            blocks,
            vec![
                (3u64, Box::from(b"block7-3".as_slice())),
                (4u64, Box::from(b"block7-4".as_slice())),
                (6u64, Box::from(b"block7-6".as_slice()))
            ]
        );

        // Reverse
        let blocks: Vec<_> = db
            .iterate_raw_reverse(7, 4)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            blocks,
            vec![
                (4u64, Box::from(b"block7-4".as_slice())),
                (3u64, Box::from(b"block7-3".as_slice())),
                (2u64, Box::from(b"block7-2".as_slice()))
            ]
        );
    }
}
