use std::path::Path;

use rocksdb::WriteBatchWithTransaction;
use tempfile::TempDir;

use crate::{BlockRange, Error, db::BlockDb, utils::ranges::RangesExt};

impl From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Self {
        Error::StorageLayer(e.to_string())
    }
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
        // NOTE: The RocksDB documentation states that the `max_open_files` option "should" be set
        // to -1 for secondary instances. It is unclear whether this is simply a performance
        // recommendation or a functional requirement. We currently do not adhere to this advice to
        // avoid running into the rlimit (see comment in make_options).
        let db_opts = Self::make_options();
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
        // If we open more files than allowed by the rlimit, the process will panic.
        // To avoid this, we can either query the limit and set the constraint accordingly (e.g.
        // NOFILE - 100; -10 is not enough), or always set the constrain to 1.
        // According to the RocksDb documentation, a lower limit can decrease performance, but
        // benchmarks have shown that at least for the initial genesis import, there is no
        // significant difference.
        // Therefore, we set it to 1 for the time being.
        opts.set_max_open_files(1);
        opts
    }

    fn make_key(chain_id: u64, block_number: u64) -> [u8; 16] {
        let mut key = [0u8; 16];
        key[0..8].copy_from_slice(&chain_id.to_be_bytes());
        key[8..16].copy_from_slice(&block_number.to_be_bytes());
        key
    }

    /// Iterates over raw protobuf-encoded blocks for the specified chain-ID starting from the given
    /// block number in the given direction.
    /// Returns an iterator that yields tuples of (block number, data).
    /// The sequence of blocks is ordered by block number (depending on the direction in ascending
    /// or descending order) and may contain gaps for missing blocks.
    fn iterate_with_direction_raw(
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
            .map_while({
                let mut stop = false;
                move |result| {
                    if stop {
                        return None;
                    }
                    let (key, value) = match result {
                        Ok((key, value)) => (key, value),
                        Err(e) => return Some(Err(Error::StorageLayer(e.to_string()))),
                    };
                    if key.len() == 8 {
                        // we got metadata, so there is no more data for this chain id
                        return None;
                    }
                    if key.len() != 16 {
                        // we got an error: return the error and stop the iteration
                        stop = true;
                        return Some(Err(Error::StorageLayer(format!(
                            "unexpected key length {}",
                            key.len()
                        ))));
                    }
                    let cid = u64::from_be_bytes(key[0..8].try_into().unwrap());
                    if cid != chain_id {
                        return None;
                    }
                    let block_number = u64::from_be_bytes(key[8..16].try_into().unwrap());
                    Some(Ok((cid, block_number, value)))
                }
            })
            .map(|result| result.map(|(_, block_number, value)| (block_number, value)))
    }

    /// Write all blocks in the batch to the database.
    /// This also updates the metadata for the chain-ID and the block ranges.
    pub fn write_batch(&self, chain_id: u64, block_batch: BlockBatch) -> Result<(), Error> {
        self.db.write(block_batch.batch)?;
        for block_range in block_batch.block_ranges {
            self.add_range_to_ranges(chain_id, block_range)?;
        }
        Ok(())
    }
}

impl BlockDb for RocksBlockDb {
    fn get_metadata_raw(&self, key: u64) -> Result<Option<Vec<u64>>, Error> {
        self.db
            .get(key.to_be_bytes())
            .map_err(|e| Error::StorageLayer(e.to_string()))?
            .map(|value| {
                if value.len() % 8 != 0 {
                    return Err(Error::StorageLayer(format!(
                        "invalid metadata length: data length {} not a multiple of 8 bytes",
                        value.len()
                    )));
                }
                Ok(value
                    .chunks_exact(8)
                    // the length is a multiple of 8, so we can safely unwrap
                    .map(|chunk| u64::from_be_bytes(chunk.try_into().unwrap()))
                    .collect())
            })
            .transpose()
    }

    fn put_metadata_raw(&self, key: u64, data: &[u64]) -> Result<(), Error> {
        self.db
            .put(
                key.to_be_bytes(),
                data.iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<u8>>(),
            )
            .map_err(|e| Error::StorageLayer(e.to_string()))
    }

    fn delete_metadata(&self, key: u64) -> Result<(), Error> {
        self.db
            .delete(key.to_be_bytes())
            .map_err(|e| Error::StorageLayer(e.to_string()))
    }

    fn get_raw(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error> {
        self.db
            .get(Self::make_key(chain_id, block_number))
            .map_err(|e| Error::StorageLayer(e.to_string()))
    }

    fn put_raw(&self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
        self.db
            .put(Self::make_key(chain_id, block_number), data)
            .map_err(|e| Error::StorageLayer(e.to_string()))?;
        self.add_block_number_to_ranges(chain_id, block_number)
    }

    fn iterate_raw(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
        self.iterate_with_direction_raw(chain_id, from, rocksdb::Direction::Forward)
    }

    fn iterate_reverse_raw(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
        self.iterate_with_direction_raw(chain_id, from, rocksdb::Direction::Reverse)
    }

    fn delete_range(
        &self,
        chain_id: u64,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> Result<(), Error> {
        let from_block = from_block.unwrap_or(0);
        let to_block = to_block.unwrap_or(u64::MAX);
        if from_block > to_block {
            return Err(Error::StorageLayer(
                "Invalid argument: end key comes before start key".to_string(),
            ));
        }
        let from = Self::make_key(chain_id, from_block);
        let to = Self::make_key(chain_id, to_block.saturating_add(1)); // make 'to' inclusive

        let mut batch = WriteBatchWithTransaction::<false>::default();
        batch.delete_range(from, to);
        self.db.write(batch)?;

        self.remove_range_from_ranges(chain_id, &(from_block..=to_block))
    }
}

/// A batch of blocks to be written to the database.
/// This wrapper keeps track of the block ranges that are added to the batch, so that they can be
/// added to the database after the batch is written.
pub struct BlockBatch {
    /// The buffer of blocks to be written.
    /// The `false` parameter indicates that this batch is not transactional.
    batch: WriteBatchWithTransaction<false>,
    /// The block ranges of the blocks in `batch`.
    block_ranges: Vec<BlockRange>,
}

impl BlockBatch {
    /// Creates a new empty block batch.
    pub fn new() -> Self {
        BlockBatch {
            batch: WriteBatchWithTransaction::default(),
            block_ranges: Vec::new(),
        }
    }

    /// Stores the raw protobuf-encoded data for the specified chain-ID and block number in the
    /// batch.
    pub fn put_raw(&mut self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
        self.batch
            .put(RocksBlockDb::make_key(chain_id, block_number), data);

        self.block_ranges.add_range(block_number..=block_number);

        Ok(())
    }

    /// Returns the current number of blocks in the batch.
    pub fn count(&self) -> usize {
        self.batch.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_utils::{make_meta_value, make_range_value};

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
    fn rocksblockdb_get_metadata_raw_returns_raw_metadata() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();

        let key = 1u64;
        let data = [1u64; 2];
        db.db
            .put(
                key.to_be_bytes(),
                data.into_iter()
                    .flat_map(u64::to_be_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();

        let result = db.get_metadata_raw(key).unwrap();
        assert_eq!(result, Some(data.to_vec()));

        // query non existing key
        let key = 2u64;

        let result = db.get_metadata_raw(key).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn rocksblockdb_get_metadata_raw_returns_error_if_value_length_is_invalid() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let key = 1u64;
        let data = [1u8]; // not a multiple of 8 bytes
        db.db.put(key.to_be_bytes(), data).unwrap();

        assert_eq!(
            db.get_metadata_raw(key),
            Err(Error::StorageLayer(
                "invalid metadata length: data length 1 not a multiple of 8 bytes".to_owned()
            ))
        );
    }

    #[test]
    fn rocksblockdb_put_metadata_raw_write_raw_metadata() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let key = 1u64;
        let data = [1u64; 2];

        db.put_metadata_raw(key, &data).unwrap();

        let result = db.db.get(key.to_be_bytes()).unwrap();

        assert_eq!(result, Some(make_meta_value(data)));
    }

    #[test]
    fn blockrocksdb_put_raw_adds_range_and_chain_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();

        let chain_id = 146;
        let block_number = 123;
        db.put_raw(chain_id, block_number, b"block").unwrap();

        assert_eq!(
            db.db.get(0u64.to_be_bytes().as_slice()),
            Ok(Some(make_meta_value([chain_id])))
        );
        assert_eq!(
            db.db.get(chain_id.to_be_bytes().as_slice()),
            Ok(Some(make_range_value([block_number..=block_number])))
        );
    }

    #[test]
    fn blockrocksdb_delete_range_removes_range_and_chain_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();

        let chain_id = 146;
        let block_number = 123;
        db.put_chain_ids(&[chain_id]).unwrap();
        db.put_ranges_of_chain_id(chain_id, &[block_number..=block_number])
            .unwrap();
        db.delete_range(chain_id, Some(block_number), Some(block_number))
            .unwrap();

        assert_eq!(db.db.get(0u64.to_be_bytes().as_slice()), Ok(Some(vec![])));
        assert_eq!(db.db.get(chain_id.to_be_bytes().as_slice()), Ok(None));
    }

    #[test]
    fn rocksblockdb_put_raw_writes_raw_data() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();
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
        let db = RocksBlockDb::open_for_reading(tmpdir.path()).unwrap();
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

        // query non existing key
        let result = db.get_raw(chain_id, 100).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn rocksblockdb_iterate_raw_returns_blocks_for_single_chain_in_order() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();

        db.put_raw(1, 0, b"block1-0").unwrap();
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
            .iterate_reverse_raw(7, 4)
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

    #[test]
    fn rocksblockdb_iterate_raw_returns_error_if_error_occurs_and_stops() {
        let chain_id = 1;

        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();

        db.put_raw(chain_id, 0, b"block1-0").unwrap();
        let mut invalid_key = [0; 17];
        invalid_key[0..8].copy_from_slice(&chain_id.to_be_bytes());
        db.db.put(invalid_key, b"block0-1").unwrap();
        db.put_raw(chain_id, 2, b"block1-2").unwrap();

        let blocks: Vec<_> = db.iterate_raw(chain_id, 0).collect();
        assert_eq!(
            blocks,
            vec![
                Ok((0u64, Box::from(b"block1-0".as_slice()))),
                Err(Error::StorageLayer("unexpected key length 17".to_string()))
            ],
        );
    }

    #[test]
    fn rocksblockdb_delete_range_deletes_blocks_in_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let chain_id = 1;

        // Insert blocks 1 to 5
        for i in 1..=5 {
            db.put_raw(chain_id, i, format!("block {i}").as_bytes())
                .unwrap();
        }

        // Delete blocks 2 to 4
        db.delete_range(chain_id, Some(2), Some(4)).unwrap();

        // Check remaining blocks
        assert_eq!(db.get_raw(chain_id, 1).unwrap(), Some(b"block 1".to_vec()));
        assert_eq!(db.get_raw(chain_id, 2).unwrap(), None);
        assert_eq!(db.get_raw(chain_id, 3).unwrap(), None);
        assert_eq!(db.get_raw(chain_id, 4).unwrap(), None); // range end is exclusive
        assert_eq!(db.get_raw(chain_id, 5).unwrap(), Some(b"block 5".to_vec()));
    }

    #[test]
    fn rocksblockdb_delete_range_succeeds_if_no_blocks_in_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();

        // delete range when no blocks exist
        assert!(db.delete_range(1, Some(1), Some(5)).is_ok());
    }

    #[test]
    fn rocksblockdb_delete_range_returns_error_if_start_greater_than_end() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let chain_id = 1;

        // Insert blocks 1 to 5
        for i in 1..=5 {
            db.put_raw(chain_id, i, format!("block {i}").as_bytes())
                .unwrap();
        }

        // Delete range with start greater than end
        assert_eq!(
            db.delete_range(chain_id, Some(2), Some(1)),
            Err(Error::StorageLayer(
                "Invalid argument: end key comes before start key".to_string(),
            ))
        );

        // Ensure all blocks are still present
        for i in 1..=5 {
            assert_eq!(
                db.get_raw(chain_id, i).unwrap(),
                Some(format!("block {i}").as_bytes().to_vec())
            );
        }
    }

    #[test]
    fn rocksblockdb_batched_writes_write_all_elements_and_update_metadata() {
        let tmpdir = tempfile::tempdir().unwrap();
        let db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let chain_id = 1;
        let block_numbers = [1, 2, 4];
        let ranges = [1..=2, 4..=4];

        let mut batch = BlockBatch::new();
        for (i, &block_number) in block_numbers.iter().enumerate() {
            assert!(batch.count() == i);
            batch
                .put_raw(
                    chain_id,
                    block_number,
                    format!("block {block_number}").as_bytes(),
                )
                .unwrap();
            assert!(batch.count() == i + 1);
        }
        assert_eq!(batch.block_ranges, ranges);

        db.write_batch(chain_id, batch).unwrap();

        for block_number in block_numbers {
            assert_eq!(
                db.get_raw(chain_id, block_number).unwrap(),
                Some(format!("block {block_number}").as_bytes().to_vec())
            );
        }
        assert_eq!(db.get_ranges_of_chain_id(chain_id).unwrap(), ranges);
    }
}
