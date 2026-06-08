// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

use std::path::Path;

use rocksdb::WriteBatchWithTransaction;
use tempfile::TempDir;

use crate::{
    Error,
    db::blockdb::{IterationDirection, KvDb, KvDbBatch},
};

impl From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Self {
        Error::StorageLayer(e.to_string())
    }
}

/// A key-value store using RocksDB as its storage layer.
#[derive(Debug)]
pub struct RocksDb {
    db: rocksdb::DB,

    /// Path of the secondary instance, if opened for reading.
    _secondary_path: Option<TempDir>,
}

impl RocksDb {
    /// Creates a new RocksDB database at the specified path.
    /// Returns an error if the database already exists.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut db_opts = Self::make_options();
        db_opts.set_error_if_exists(true);
        db_opts.create_if_missing(true);
        Ok(Self {
            db: rocksdb::DB::open(&db_opts, path)?,
            _secondary_path: None,
        })
    }

    /// Opens an existing RocksDB database for reading and writing.
    /// Returns an error if the database does not exist or is already opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut db_opts = Self::make_options();
        db_opts.create_if_missing(false);
        Ok(Self {
            db: rocksdb::DB::open(&db_opts, path)?,
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
        let secondary_path = tempfile::tempdir()?;
        Ok(Self {
            db: rocksdb::DB::open_as_secondary(&db_opts, path.as_ref(), secondary_path.path())?,
            // This will be removed automatically when the rocksdb instance is dropped.
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
}

impl KvDb for RocksDb {
    type Batch = RocksDbBatch;

    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        self.db.get(key).map_err(Error::from)
    }

    fn put_raw(&self, key: &[u8], data: &[u8]) -> Result<(), Error> {
        self.db.put(key, data).map_err(Error::from)
    }

    fn delete_raw(&self, key: &[u8]) -> Result<(), Error> {
        self.db.delete(key).map_err(Error::from)
    }

    fn iterate_raw(
        &self,
        start: Box<[u8]>,
        direction: IterationDirection,
    ) -> impl Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), Error>> {
        let rocksdb_direction = match direction {
            IterationDirection::Forward => rocksdb::Direction::Forward,
            IterationDirection::Reverse => rocksdb::Direction::Reverse,
        };
        self.db
            .iterator(rocksdb::IteratorMode::From(&start, rocksdb_direction))
            .map(|item| item.map_err(Error::from))
    }

    fn batch_raw(&self) -> Self::Batch {
        Self::Batch::default()
    }

    fn write_batch_raw(&self, batch: Self::Batch) -> Result<(), Error> {
        self.db.write(batch.batch).map_err(Error::from)
    }
}

/// A batch of write/ delete operations to be written to the database.
#[derive(Default)]
pub struct RocksDbBatch {
    /// The underlying Rocks DB batch.
    /// The `false` parameter indicates that this batch is not transactional.
    batch: WriteBatchWithTransaction<false>,
}

impl KvDbBatch for RocksDbBatch {
    fn put_raw(&mut self, key: &[u8], data: &[u8]) {
        self.batch.put(key, data);
    }

    fn delete_raw(&mut self, key: &[u8]) {
        self.batch.delete(key);
    }

    fn delete_range_raw(&mut self, start_key: &[u8], end_key: &[u8]) {
        // RocksDB's delete_range is exclusive of the end key, but this function should delete an
        // inclusive range, so the end key is deleted separately.
        self.batch.delete(end_key);
        self.batch.delete_range(start_key, end_key);
    }

    fn size(&self) -> usize {
        self.batch.size_in_bytes()
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate::eq;

    use super::*;
    use crate::utils::test_dir::{Permissions, TestDir};

    #[test]
    fn rocksdb_create_creates_new_db_in_existing_directory() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        RocksDb::create(tmpdir.path()).unwrap();
        let lock_file = tmpdir.path().join("LOCK");
        assert!(lock_file.exists());
    }

    #[test]
    fn rocksdb_create_creates_new_db_in_non_existent_directory() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let non_existent_path = tmpdir.path().join("non_existent_dir");
        RocksDb::create(&non_existent_path).unwrap();
        let lock_file = non_existent_path.join("LOCK");
        assert!(lock_file.exists());
    }

    #[test]
    fn rocksdb_create_returns_error_if_db_exists() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        RocksDb::create(tmpdir.path()).unwrap();
        let result = RocksDb::create(tmpdir.path());
        assert!(matches!(
            result,
            Err(Error::StorageLayer(msg))
            if msg.contains("exists (error_if_exists is true)")
        ));
    }

    #[test]
    fn rocksdb_create_returns_error_if_path_is_not_writable() {
        let tmpdir = TestDir::try_new(Permissions::ReadOnly).unwrap();
        let result = RocksDb::create(tmpdir.path());
        assert!(matches!(
            result,
            Err(Error::StorageLayer(msg))
            if msg.contains("Permission denied")
        ));
    }

    #[test]
    fn rocksdb_open_opens_existing_db_for_reading_and_writing() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        RocksDb::create(tmpdir.path()).unwrap();
        let db = RocksDb::open(tmpdir.path()).unwrap();
        db.db.put(b"key", b"value").unwrap();
        let value = db.db.get(b"key").unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
    }

    #[test]
    fn rocksdb_open_returns_error_if_db_does_not_exist() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let result = RocksDb::open(tmpdir.path());
        assert!(matches!(
            result,
            Err(Error::StorageLayer(msg))
            if msg.contains("does not exist (create_if_missing is false)")
        ));
    }

    #[test]
    fn rocksdb_open_returns_error_if_db_already_opened() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let _db = RocksDb::create(tmpdir.path()).unwrap();
        let result = RocksDb::open(tmpdir.path());
        assert!(matches!(
            result,
            Err(Error::StorageLayer(msg))
            if msg.contains("No locks available")
        ));
    }

    #[test]
    fn rocksdb_open_for_reading_opens_existing_db_for_reading() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        {
            let db = RocksDb::create(tmpdir.path()).unwrap();
            db.db.put(b"key", b"value").unwrap();
        }
        let db = RocksDb::open_for_reading(tmpdir.path()).unwrap();
        let value = db.db.get(b"key").unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
        let result = db.db.put(b"key", b"baz");
        assert!(matches!(result, Err(e) if e.kind() == rocksdb::ErrorKind::NotSupported));
    }

    #[test]
    fn rocksdb_open_for_reading_returns_error_if_db_does_not_exist() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let result = RocksDb::open_for_reading(tmpdir.path());
        assert!(matches!(
            result,
            Err(Error::StorageLayer(msg))
            if msg.contains("No such file or directory")
        ));
    }

    #[test]
    fn rocksdb_open_for_reading_can_be_called_multiple_times() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        {
            let db = RocksDb::create(tmpdir.path()).unwrap();
            db.db.put(b"key", b"value").unwrap();
        }
        let db1 = RocksDb::open_for_reading(tmpdir.path()).unwrap();
        let db2 = RocksDb::open_for_reading(tmpdir.path()).unwrap();
        let value = db1.db.get(b"key").unwrap().unwrap();
        assert_eq!(value, b"value");
        let value = db2.db.get(b"key").unwrap().unwrap();
        assert_eq!(value, b"value");
    }

    #[test]
    fn rocksdb_removes_secondary_path_on_drop() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        RocksDb::create(tmpdir.path()).unwrap();
        let db = RocksDb::open_for_reading(tmpdir.path()).unwrap();
        assert!(db._secondary_path.is_some());
        let secondary_path = db._secondary_path.as_ref().unwrap().path().to_owned();
        assert!(secondary_path.exists());
        drop(db);
        assert!(!secondary_path.exists());
    }

    #[test]
    fn rocksdb_can_be_opened_for_reading_and_writing_concurrently() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let write_db = RocksDb::create(tmpdir.path()).unwrap();
        let read_db = RocksDb::open_for_reading(tmpdir.path()).unwrap();
        write_db.db.put(b"key", b"value").unwrap();
        read_db.db.try_catch_up_with_primary().unwrap();
        let value = read_db.db.get(b"key").unwrap().unwrap();
        assert_eq!(value, b"value");
    }

    #[test]
    fn rocksdb_get_raw_returns_stored_data() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();
        db.db.put(b"key", b"value").unwrap();

        let result = db.get_raw(b"key").unwrap();
        assert_eq!(result, Some(b"value".to_vec()));
    }

    #[test]
    fn rocksdb_get_raw_returns_none_for_missing_key() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();

        assert_eq!(db.get_raw(b"missing").unwrap(), None);
    }

    #[test]
    fn rocksdb_put_raw_stores_data() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();

        db.put_raw(b"key", b"value").unwrap();
        let value = db.db.get(b"key").unwrap();
        assert_eq!(value, Some(b"value".to_vec()));
    }

    #[test]
    fn rocksdb_put_raw_overwrites_existing_data() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();
        db.db.put(b"key", b"value1").unwrap();

        db.put_raw(b"key", b"value2").unwrap();
        let value = db.db.get(b"key").unwrap();
        assert_eq!(value, Some(b"value2".to_vec()));
    }

    #[test]
    fn rocksdb_put_raw_fails_if_db_opened_as_read_only() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        RocksDb::create(tmpdir.path()).unwrap();
        let db = RocksDb::open_for_reading(tmpdir.path()).unwrap();

        let result = db.put_raw(b"key", b"value");
        assert!(matches!(
            result,
            Err(e) if e.to_string().contains("Not supported operation in secondary mode")
        ));
    }

    #[test]
    fn rocksdb_delete_raw_removes_key() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();
        db.db.put(b"key", b"value").unwrap();
        db.delete_raw(b"key").unwrap();
        assert_eq!(db.db.get(b"key").unwrap(), None);
    }

    #[test]
    fn rocksdb_delete_raw_is_noop_on_non_existing_key() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();
        db.delete_raw(b"key").unwrap();
    }

    #[test]
    fn rocksdb_iterate_raw_forward_starts_at_given_key_and_continues_until_end() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();

        db.put_raw(b"a", b"1").unwrap();
        db.put_raw(b"b", b"2").unwrap();
        db.put_raw(b"c", b"3").unwrap();

        let last_two_entries = vec![
            (Box::from(b"b".as_slice()), Box::from(b"2".as_slice())),
            (Box::from(b"c".as_slice()), Box::from(b"3".as_slice())),
        ];

        // start at existing key
        let results: Vec<_> = db
            .iterate_raw(Box::from(b"b".as_slice()), IterationDirection::Forward)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(results, last_two_entries);

        // start at non-existing key
        let results: Vec<_> = db
            .iterate_raw(Box::from(b"a0".as_slice()), IterationDirection::Forward)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(results, last_two_entries);
    }

    #[test]
    fn rocksdb_iterate_raw_reverse_starts_at_given_key_and_continues_until_start() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();

        db.put_raw(b"a", b"1").unwrap();
        db.put_raw(b"b", b"2").unwrap();
        db.put_raw(b"c", b"3").unwrap();

        let first_two_entries = vec![
            (Box::from(b"b".as_slice()), Box::from(b"2".as_slice())),
            (Box::from(b"a".as_slice()), Box::from(b"1".as_slice())),
        ];

        // start at existing key
        let results: Vec<_> = db
            .iterate_raw(Box::from(b"b".as_slice()), IterationDirection::Reverse)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results, first_two_entries);

        // start at non-existing key
        let results: Vec<_> = db
            .iterate_raw(Box::from(b"b0".as_slice()), IterationDirection::Reverse)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(results, first_two_entries,);
    }

    #[test]
    fn rocksdb_batch_raw_returns_empty_rocks_batch() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();

        let batch = db.batch_raw();
        assert_eq!(batch.batch.len(), 0);
    }

    #[test]
    fn rocksdb_write_batch_raw_writes_batch() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();

        let mut batch = db.batch_raw();
        batch.batch.put(b"key1", b"value1");
        batch.batch.put(b"key2", b"value2");

        db.write_batch_raw(batch).unwrap();

        assert_eq!(db.db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(db.db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn rocksbatch_put_raw_adds_put_operation_to_batch() {
        let mut batch = RocksDbBatch::default();
        batch.put_raw(b"key", b"value");

        let mut inspector = MockBatchInspector::new();
        inspector
            .expect_put()
            .with(eq(b"key".as_slice()), eq(b"value".as_slice()))
            .times(1)
            .return_const(());
        batch.batch.iterate(&mut inspector);
    }

    #[test]
    fn rocksbatch_delete_raw_adds_delete_operation_to_batch() {
        let mut batch = RocksDbBatch::default();
        batch.delete_raw(b"key");

        let mut inspector = MockBatchInspector::new();
        inspector
            .expect_delete()
            .with(eq(b"key".as_slice()))
            .times(1)
            .return_const(());
        batch.batch.iterate(&mut inspector);
    }

    #[test]
    fn rocksbatch_delete_range_raw_adds_delete_operation_for_end_key_to_batch() {
        let mut batch = RocksDbBatch::default();
        batch.delete_range_raw(b"start", b"end");

        let mut inspector = MockBatchInspector::new();
        inspector
            .expect_delete()
            .with(eq(b"end".as_slice()))
            .return_const(())
            .times(1);
        batch.batch.iterate(&mut inspector);
        // Note: We currently cannot verify that the delete_range operation was added to the batch,
        // because the WriteBatchIterator trait does not provide support for this.
        // This is tested in the test below by writing the batch to a RocksDb instance.
    }

    #[test]
    fn rocksbatch_delete_range_raw_deletes_range_inclusively() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let db = RocksDb::create(tmpdir.path()).unwrap();

        // Populate keys
        db.put_raw(b"a", b"1").unwrap();
        db.put_raw(b"b", b"2").unwrap();
        db.put_raw(b"c", b"3").unwrap();
        db.put_raw(b"d", b"4").unwrap();

        // delete_range_raw should delete b, c (inclusive range)
        let mut batch = db.batch_raw();
        batch.delete_range_raw(b"b", b"c");
        db.write_batch_raw(batch).unwrap();

        assert_eq!(db.get_raw(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(db.get_raw(b"b").unwrap(), None);
        assert_eq!(db.get_raw(b"c").unwrap(), None);
        assert_eq!(db.get_raw(b"d").unwrap(), Some(b"4".to_vec())); // end key is inclusive
    }

    #[test]
    fn rocksbatch_size_returns_size_of_batch() {
        let mut batch = RocksDbBatch::default();
        let size0 = batch.size();

        batch.put_raw(b"key", b"value");
        let size1 = batch.size();
        assert!(size1 > size0);

        batch.put_raw(b"key2", b"value");
        let size2 = batch.size();
        assert!(size2 > size1);

        batch.delete_raw(b"key");
        let size3 = batch.size();
        assert!(size3 > size2);
    }

    mockall::mock! {
        pub BatchInspector {}

        impl rocksdb::WriteBatchIterator for BatchInspector  {
            fn put(&mut self, key: &[u8], value: &[u8]);
            fn delete(&mut self, key: &[u8]);
        }
    }
}
