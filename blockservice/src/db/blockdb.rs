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

use std::collections::HashMap;

use bertha_types::Block;
use prost::Message;

use crate::{BlockRange, db::proto, error::Error, utils::ranges::RangesExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IterationDirection {
    Forward,
    Reverse,
}

type RawEntry = (Box<[u8]>, Box<[u8]>);

type IterBytesItem = (u64, Box<[u8]>);

/// A batch of operations for the [KvDb] that can be written atomically.
#[cfg_attr(test, mockall::automock)]
pub trait KvDbBatch: Default {
    /// Stores raw bytes for an arbitrary key in the batch.
    fn put_raw(&mut self, key: &[u8], data: &[u8]);

    /// Deletes the value for an arbitrary key in the batch.
    fn delete_raw(&mut self, key: &[u8]);

    /// Deletes all entries with keys in the range [start_key, end_key] in the batch.
    fn delete_range_raw(&mut self, start_key: &[u8], end_key: &[u8]);

    /// Returns the size of the serialized batch in bytes.
    fn size(&self) -> usize;
}

/// A lower-level key-value database interface that operates on raw byte keys and values.
/// This is used by [BlockDb] to store blocks and metadata, but can also be used directly for
/// other purposes.
#[cfg_attr(test, mockall::automock(type Batch = MockKvDbBatch;))]
pub trait KvDb {
    type Batch: KvDbBatch;

    /// Retrieves raw bytes for an arbitrary key.
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error>;

    /// Stores raw bytes for an arbitrary key.
    fn put_raw(&self, key: &[u8], data: &[u8]) -> Result<(), Error>;

    /// Deletes the value for an arbitrary key.
    fn delete_raw(&self, key: &[u8]) -> Result<(), Error>;

    /// Iterates over raw key-value pairs starting from the given key.
    /// The iterator yields tuples of (key, value) ordered lexicographically and may contain gaps
    /// for missing keys.
    fn iterate_raw(
        &self,
        start: Box<[u8]>,
        direction: IterationDirection,
    ) -> impl Iterator<Item = Result<RawEntry, Error>> + Send;

    /// Creates a new batch for atomic writes to the database.
    fn batch_raw(&self) -> Self::Batch;

    /// Writes the batch to the database atomically.
    fn write_batch_raw(&self, batch: Self::Batch) -> Result<(), Error>;

    /// Attempts to catch up with the primary instance by applying any new data written by the
    /// primary since the last catch-up. This is a no-op for primary instances.
    fn try_catch_up_with_primary(&self) -> Result<(), Error>;
}

/// A batch of operations for the [BlockDb] that can be written atomically.
#[cfg_attr(test, mockall::automock)]
pub trait BlockDbBatch {
    /// Stores a block for the specified chain-ID and block number in the batch. The data is
    /// expected to be a protobuf-encoded block.
    fn put_bytes(&mut self, chain_id: u64, block_number: u64, data: &[u8]);

    /// Returns the size of the serialized batch in bytes.
    fn size(&self) -> usize;
}

/// A database that allows to store and query [Block]s and metadata for multiple
/// different blockchains. Blocks are stored as encoded protobuf messages.
///
/// Implementations have to ensure that all operations that modify the block database also update
/// the metadata as one atomic operation, ensuring metadata is always consistent.
///
/// All operations that modify the database require a mutable reference to the database, which
/// ensures that other operations that read or modify the database cannot run concurrently.
/// Implementations can rely on this to ensure thread safety without needing to implement additional
/// synchronization. In particular, methods that have to read, update and write data can do so
/// without needing to worry about concurrent modifications.
/// The only exception is the `try_catch_up_with_primary` method, which is intended to be called on
/// a secondary instance while the primary instance is running. But since the secondary instance
/// only allows reading the database, it is still guaranteed that the database is always in a
/// consistent state.
#[cfg_attr(test, mockall::automock(type Batch = MockBlockDbBatch;))]
pub trait BlockDb {
    type Batch: BlockDbBatch;

    /// Retrieves the IDs of all chains stored in the database.
    fn get_chain_ids(&self) -> Result<Vec<u64>, Error>;

    /// Retrieves the stored ranges of blocks for the specified chain-ID.
    /// The start and end of each range are inclusive.
    fn get_ranges_of_chain_id(&self, chain_id: u64) -> Result<Vec<BlockRange>, Error>;

    /// Retrieves the JSON-encoded upgrade heights for the specified chain-ID.
    fn get_upgrade_heights(&self, chain_id: u64) -> Result<Option<Vec<u8>>, Error>;

    /// Stores the JSON-encoded upgrade heights for the specified chain-ID.
    fn put_upgrade_heights(&mut self, chain_id: u64, data: &[u8]) -> Result<(), Error>;

    /// Retrieves the JSON-encoded corrections for the specified chain-ID.
    fn get_corrections(&self, chain_id: u64) -> Result<Option<Vec<u8>>, Error>;

    /// Stores the JSON-encoded corrections for the specified chain-ID.
    fn put_corrections(&mut self, chain_id: u64, data: &[u8]) -> Result<(), Error>;

    /// Retrieves a block for the specified chain-ID and block number.
    /// Returns [None] if the block does not exist.
    fn get(&self, chain_id: u64, block_number: u64) -> Result<Option<Block>, Error>;

    /// Retrieves the raw protobuf-encoded bytes of a block for the specified chain-ID and block
    /// number. Returns [None] if the block does not exist.
    fn get_bytes(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error>;

    /// Stores a block for the specified chain-ID and updates the metadata.
    /// The block number is obtained from the block itself.
    fn put(&mut self, chain_id: u64, block: Block) -> Result<(), Error>;

    /// Stores a block for the specified chain-ID and block number and updates the metadata.
    /// The data is expected to be a protobuf-encoded block.
    fn put_bytes(&mut self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error>;

    /// Deletes blocks in the specified block number range (inclusive) for the specified chain-ID
    /// and updates the metadata.
    fn delete_range(
        &mut self,
        chain_id: u64,
        start_block_number: u64,
        end_block_number: u64,
    ) -> Result<(), Error>;

    /// Iterates over all block numbers (extracted from the keys) and blocks for the specified
    /// chain-ID starting from the given block number in the given direction. The sequence is
    /// ordered by block number and may contain gaps for missing blocks.
    fn iterate(
        &self,
        chain_id: u64,
        start_block_number: u64,
        direction: IterationDirection,
    ) -> impl Iterator<Item = Result<(u64, Block), Error>> + Send;

    /// Iterates over all block numbers (extracted from the keys) and protobuf encoded blocks for
    /// the specified chain-ID starting from the given block number in the given direction. The
    /// sequence is ordered by block number and may contain gaps for missing blocks.
    fn iterate_bytes(
        &self,
        chain_id: u64,
        start_block_number: u64,
        direction: IterationDirection,
    ) -> impl Iterator<Item = Result<IterBytesItem, Error>> + Send;

    /// Creates a new batch for atomic writes to the database. The batch can be used to group
    /// multiple write operations into a single atomic operation.
    fn batch(&self) -> Self::Batch;

    /// Writes a batch to the database atomically. Updates metadata (ranges and chain IDs)
    /// based on the blocks in the batch.
    fn write_batch(&mut self, batch: Self::Batch) -> Result<(), Error>;

    /// Attempts to catch up with the primary instance by applying any new data written by the
    /// primary since the last catch-up. This is a no-op for primary instances.
    fn try_catch_up_with_primary(&self) -> Result<(), Error>;
}

/// A wrapper around a [KvDbBatch] that allows writing multiple blocks and their metadata as one
/// atomic operation.
#[derive(Debug)]
pub struct KvDbBatchWrapper<B: KvDbBatch> {
    /// The batch of the underlying key-value database.
    kv_batch: B,
    /// The block ranges of blocks in this batch.
    block_ranges: HashMap<u64, Vec<BlockRange>>,
}

impl<B: KvDbBatch> BlockDbBatch for KvDbBatchWrapper<B> {
    fn put_bytes(&mut self, chain_id: u64, block_number: u64, data: &[u8]) {
        self.block_ranges
            .entry(chain_id)
            .or_default()
            .add_range(block_number..=block_number);
        self.kv_batch
            .put_raw(&make_block_key(chain_id, block_number), data);
    }

    fn size(&self) -> usize {
        self.kv_batch.size()
    }
}

#[derive(Debug)]
pub struct KvDbBackedBlockDb<D: KvDb> {
    db: D,
}

impl<D: KvDb> KvDbBackedBlockDb<D>
where
    D::Batch: Send + 'static,
{
    /// Creates a new [BlockDb] backed by the given key-value store implementation.
    pub fn create(db: D) -> Result<Self, Error> {
        if db
            .iterate_raw(Box::from([]), IterationDirection::Forward)
            .next()
            .transpose()?
            .is_some()
        {
            return Err(Error::StorageLayer(
                "block database already initialized".to_owned(),
            ));
        }
        db.put_raw(&VERSION_KEY, &serialize_version(CURRENT_VERSION))?;
        Ok(Self { db })
    }

    /// Opens an existing [BlockDb] backed by the given key-value store implementation.
    pub fn open(db: D) -> Result<Self, Error> {
        let Some(version_bytes) = db.get_raw(&VERSION_KEY)? else {
            return Err(Error::StorageLayer(
                "block database version not found".to_owned(),
            ));
        };
        let version = deserialize_version(version_bytes)?;
        if version != CURRENT_VERSION {
            return Err(Error::StorageLayer(format!(
                "block database version is not supported: code expects version {CURRENT_VERSION} but database has version {version}",
            )));
        }
        Ok(Self { db })
    }

    /// Executes a function that operates on a batch, then writes the batch atomically.
    fn execute_in_batch<R>(&mut self, f: impl FnOnce(&mut D::Batch) -> R) -> Result<R, Error> {
        let mut batch = self.db.batch_raw();
        let r = f(&mut batch);
        self.db.write_batch_raw(batch)?;
        Ok(r)
    }
}

impl<D: KvDb> BlockDb for KvDbBackedBlockDb<D>
where
    D::Batch: Send + 'static,
{
    type Batch = KvDbBatchWrapper<D::Batch>;

    fn get_chain_ids(&self) -> Result<Vec<u64>, Error> {
        Ok(self
            .db
            .get_raw(&CHAIN_IDS_KEY)?
            .map(deserialize_chain_ids)
            .transpose()?
            .unwrap_or_default())
    }

    fn get_ranges_of_chain_id(&self, chain_id: u64) -> Result<Vec<BlockRange>, Error> {
        Ok(self
            .db
            .get_raw(&make_block_ranges_key(chain_id))?
            .map(deserialize_block_ranges)
            .transpose()?
            .unwrap_or_default())
    }

    fn get_upgrade_heights(&self, chain_id: u64) -> Result<Option<Vec<u8>>, Error> {
        self.db.get_raw(&make_upgrade_heights_key(chain_id))
    }

    fn put_upgrade_heights(&mut self, chain_id: u64, data: &[u8]) -> Result<(), Error> {
        let mut chain_ids = self.get_chain_ids()?;
        if let Err(idx) = chain_ids.binary_search(&chain_id) {
            chain_ids.insert(idx, chain_id);
        }
        self.execute_in_batch(|batch| {
            batch.put_raw(&make_upgrade_heights_key(chain_id), data);
            batch.put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids(chain_ids));
        })?;
        Ok(())
    }

    fn get_corrections(&self, chain_id: u64) -> Result<Option<Vec<u8>>, Error> {
        self.db.get_raw(&make_corrections_key(chain_id))
    }

    fn put_corrections(&mut self, chain_id: u64, data: &[u8]) -> Result<(), Error> {
        let mut chain_ids = self.get_chain_ids()?;
        if let Err(idx) = chain_ids.binary_search(&chain_id) {
            chain_ids.insert(idx, chain_id);
        }
        self.execute_in_batch(|batch| {
            batch.put_raw(&make_corrections_key(chain_id), data);
            batch.put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids(chain_ids));
        })?;
        Ok(())
    }

    fn get(&self, chain_id: u64, block_number: u64) -> Result<Option<Block>, Error> {
        self.get_bytes(chain_id, block_number)?
            .map(|data| {
                proto::Block::decode(data.as_slice())
                    .map_err(Error::Protobuf)
                    .and_then(Block::try_from)
            })
            .transpose()
    }

    fn get_bytes(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error> {
        self.db.get_raw(&make_block_key(chain_id, block_number))
    }

    fn put(&mut self, chain_id: u64, block: Block) -> Result<(), Error> {
        let number = block.number;
        let data = proto::Block::from(block).encode_to_vec();
        self.put_bytes(chain_id, number, &data)
    }

    fn put_bytes(&mut self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
        let mut chain_ids = self.get_chain_ids()?;
        if let Err(idx) = chain_ids.binary_search(&chain_id) {
            chain_ids.insert(idx, chain_id);
        }
        let mut ranges = self.get_ranges_of_chain_id(chain_id)?;
        ranges.add_range(block_number..=block_number);
        self.execute_in_batch(|batch| {
            batch.put_raw(&make_block_key(chain_id, block_number), data);
            batch.put_raw(
                &make_block_ranges_key(chain_id),
                &serialize_block_ranges(ranges.clone()),
            );
            batch.put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids(chain_ids));
        })
    }

    fn delete_range(
        &mut self,
        chain_id: u64,
        start_block_number: u64,
        end_block_number: u64,
    ) -> Result<(), Error> {
        if start_block_number > end_block_number {
            return Err(Error::StorageLayer(format!(
                "invalid block range: start block number {start_block_number} is greater than end block number {end_block_number}"
            )));
        }
        let mut ranges = self.get_ranges_of_chain_id(chain_id)?;
        ranges.subtract_range(&(start_block_number..=end_block_number));
        self.execute_in_batch(|batch| {
            let start_key = make_block_key(chain_id, start_block_number);
            let end_key = make_block_key(chain_id, end_block_number);
            batch.delete_range_raw(&start_key, &end_key);

            batch.put_raw(
                &make_block_ranges_key(chain_id),
                &serialize_block_ranges(ranges.clone()),
            );
        })
    }

    fn iterate(
        &self,
        chain_id: u64,
        start_block_number: u64,
        direction: IterationDirection,
    ) -> impl Iterator<Item = Result<(u64, Block), Error>> + Send {
        self.iterate_bytes(chain_id, start_block_number, direction)
            .map(|result| {
                result.and_then(|(block_number, data)| {
                    let block = proto::Block::decode(data.as_ref())?;
                    Ok((block_number, Block::try_from(block)?))
                })
            })
    }

    fn iterate_bytes(
        &self,
        chain_id: u64,
        start_block_number: u64,
        direction: IterationDirection,
    ) -> impl Iterator<Item = Result<IterBytesItem, Error>> + Send {
        let key = make_block_key(chain_id, start_block_number);
        self.db
            .iterate_raw(key.into(), direction)
            .map(move |result| match result {
                Ok((key, value)) => {
                    if matches!(key.len(), 2 | 10) {
                        // global or chain metadata, no more blocks for this chain id
                        return None;
                    }
                    if key.len() != 17 || key[0] != 0x02 {
                        return Some(Err(Error::StorageLayer(format!("unexpected key {key:?}",))));
                    }
                    let cid = u64::from_be_bytes(key[1..9].try_into().unwrap());
                    if cid != chain_id {
                        return None;
                    }
                    let block_number = u64::from_be_bytes(key[9..17].try_into().unwrap());
                    Some(Ok((block_number, value)))
                }
                Err(e) => Some(Err(e)),
            })
            .map_while({
                // stop iteration on first error or once keys are no longer valid for the chain ID
                let mut stop = false;
                move |result| {
                    if stop {
                        return None;
                    }
                    result.inspect(|res| {
                        let _ = res.as_ref().inspect_err(|_| stop = true);
                    })
                }
            })
    }

    fn batch(&self) -> Self::Batch {
        KvDbBatchWrapper {
            kv_batch: self.db.batch_raw(),
            block_ranges: HashMap::default(),
        }
    }

    fn write_batch(&mut self, mut batch: Self::Batch) -> Result<(), Error> {
        if batch.block_ranges.is_empty() {
            return Ok(());
        }

        let mut chain_ids = self.get_chain_ids()?;

        for (chain_id, new_ranges) in batch.block_ranges {
            if let Err(idx) = chain_ids.binary_search(&chain_id) {
                chain_ids.insert(idx, chain_id);
            }

            let mut existing_ranges = self.get_ranges_of_chain_id(chain_id)?;
            for range in new_ranges {
                existing_ranges.add_range(range.clone());
            }
            batch.kv_batch.put_raw(
                &make_block_ranges_key(chain_id),
                &serialize_block_ranges(existing_ranges),
            );
        }

        batch
            .kv_batch
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids(chain_ids));
        self.db.write_batch_raw(batch.kv_batch)
    }

    fn try_catch_up_with_primary(&self) -> Result<(), Error> {
        self.db.try_catch_up_with_primary()
    }
}

// Key/value layout:
// - Global metadata (2-byte keys): [0x00 prefix, suffix]
//   - 0x00 => DB format version (u64, big-endian)
//   - 0x01 => chain IDs (u64 array, big-endian)
// - Chain metadata (10-byte keys): [0x01 prefix, chain_id (8 bytes big endian), suffix]
//   - suffix 0x00 => block ranges as (start, end) u64 pairs (big-endian)
//   - suffix 0x01 => upgrade heights (JSON)
//   - suffix 0x02 => corrections (JSON)
// - Blocks (17-byte keys): [0x02 prefix, chain_id (8 bytes big endian), block_number (8 bytes big
//   endian)] => protobuf block.

/// Key for storing the version of the block database format.
const VERSION_KEY: [u8; 2] = [0x00, 0x00];

/// The current version of the block database format. The version should be incremented whenever a
/// change is made to the key/value layout or the protobuf format.
const CURRENT_VERSION: u64 = 3;

/// Converts the version into the byte format for storage.
fn serialize_version(version: u64) -> [u8; 8] {
    version.to_be_bytes()
}

/// Converts the byte format of the version back into a u64.
fn deserialize_version(data: impl AsRef<[u8]>) -> Result<u64, Error> {
    let data = data.as_ref();
    if data.len() != 8 {
        return Err(Error::StorageLayer(format!(
            "invalid block database version length: expected 8 bytes, got {} bytes",
            data.len()
        )));
    }
    Ok(u64::from_be_bytes(data.try_into().unwrap()))
}

/// Key for storing the IDs of all chains in the database.
pub const CHAIN_IDS_KEY: [u8; 2] = [0x00, 0x01];

/// Converts the chain IDs into the byte format for storage.
pub fn serialize_chain_ids(value: impl IntoIterator<Item = u64>) -> Vec<u8> {
    value.into_iter().flat_map(u64::to_be_bytes).collect()
}

/// Converts the byte format of chain IDs back into a vector of chain IDs.
fn deserialize_chain_ids(data: impl AsRef<[u8]>) -> Result<Vec<u64>, Error> {
    let data = data.as_ref();
    if !data.len().is_multiple_of(8) {
        return Err(Error::StorageLayer(format!(
            "invalid chain IDs length: data length {} not a multiple of 8 bytes",
            data.len()
        )));
    }
    Ok(data
        .chunks_exact(8)
        .map(|chunk| u64::from_be_bytes(chunk.try_into().unwrap()))
        .collect())
}

/// Returns the key for storing the block ranges for the given chain ID.
pub fn make_block_ranges_key(chain_id: u64) -> [u8; 10] {
    let mut key = [0u8; 10];
    key[0] = 0x01;
    key[1..9].copy_from_slice(&chain_id.to_be_bytes());
    key[9] = 0x00;
    key
}

/// Converts the block ranges into the byte format for storage.
pub fn serialize_block_ranges(ranges: impl IntoIterator<Item = BlockRange>) -> Vec<u8> {
    ranges
        .into_iter()
        .flat_map(|range| {
            let mut bytes = [0; 16];
            bytes[0..8].copy_from_slice(&range.start().to_be_bytes());
            bytes[8..16].copy_from_slice(&range.end().to_be_bytes());
            bytes
        })
        .collect()
}

/// Converts the byte format of block ranges back into a vector of [BlockRange]s.
fn deserialize_block_ranges(data: impl AsRef<[u8]>) -> Result<Vec<BlockRange>, Error> {
    let data = data.as_ref();
    if !data.len().is_multiple_of(16) {
        return Err(Error::StorageLayer(format!(
            "invalid block ranges length: data length {} not a multiple of 16 bytes",
            data.len()
        )));
    }
    Ok(data
        .chunks_exact(16)
        .map(|chunk| {
            let start = u64::from_be_bytes(chunk[0..8].try_into().unwrap());
            let end = u64::from_be_bytes(chunk[8..16].try_into().unwrap());
            BlockRange::new(start, end)
        })
        .collect())
}

/// Returns the key for storing the upgrade heights for the given chain ID.
pub fn make_upgrade_heights_key(chain_id: u64) -> [u8; 10] {
    let mut key = [0u8; 10];
    key[0] = 0x01;
    key[1..9].copy_from_slice(&chain_id.to_be_bytes());
    key[9] = 0x01;
    key
}

/// Returns the key for storing the corrections for the given chain ID.
pub fn make_corrections_key(chain_id: u64) -> [u8; 10] {
    let mut key = [0u8; 10];
    key[0] = 0x01;
    key[1..9].copy_from_slice(&chain_id.to_be_bytes());
    key[9] = 0x02;
    key
}

/// Returns the key for storing a block for the given chain ID and block number.
pub fn make_block_key(chain_id: u64, block_number: u64) -> [u8; 17] {
    let mut key = [0u8; 17];
    key[0] = 0x02;
    key[1..9].copy_from_slice(&chain_id.to_be_bytes());
    key[9..17].copy_from_slice(&block_number.to_be_bytes());
    key
}

#[cfg(test)]
mod tests {
    // Note: The block db is tested with two backends:
    // - a mocked KvDb to test which functions get called on the underlying KvDb
    // - the RocksDb implementation to test the expected effects on the database

    use std::{assert_matches, ops::Not};

    use mockall::predicate::eq;

    use super::*;
    use crate::{
        db::RocksDb,
        utils::test_dir::{Permissions, TestDir},
    };

    #[rstest::rstest]
    #[case::non_empty_db(
        false,
        Err(Error::StorageLayer("block database already initialized".to_owned()))
    )]
    #[case::empty_db(true, Ok(()))]
    fn kv_db_backed_block_db_create_checks_that_block_db_is_empty_and_writes_version(
        #[case] db_empty: bool,
        #[case] expected: Result<(), Error>,
    ) {
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_iterate_raw()
            .with(eq(Box::<[u8]>::from([])), eq(IterationDirection::Forward))
            .return_once(move |_, _| {
                Box::new(
                    db_empty
                        .not()
                        .then_some(Ok((Box::from([0]), Box::from([1, 2, 3]))))
                        .into_iter(),
                )
            })
            .times(1);
        if expected.is_ok() {
            kv_db
                .expect_put_raw()
                .with(eq(VERSION_KEY), eq(serialize_version(CURRENT_VERSION)))
                .return_once(|_, _| Ok(()))
                .times(1);
        }
        let result = KvDbBackedBlockDb::create(kv_db);
        match expected {
            Ok(()) => assert!(result.is_ok()),
            Err(expected_err) => {
                assert!(result.is_err());
                assert_eq!(result.unwrap_err(), expected_err);
            }
        }
    }

    #[rstest::rstest]
    #[case::non_empty_db(
        false,
        Err(Error::StorageLayer("block database already initialized".to_owned()))
    )]
    #[case::empty_db(true, Ok(()))]
    fn rocks_block_db_create_checks_that_block_db_is_empty_and_writes_version(
        #[case] db_empty: bool,
        #[case] expected: Result<(), Error>,
    ) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let rocks = RocksDb::create(tmpdir.path()).unwrap();
        if !db_empty {
            rocks.put_raw(b"key", b"value").unwrap();
        }
        let result = KvDbBackedBlockDb::create(rocks);
        match expected {
            Ok(()) => {
                assert!(result.is_ok());
                let db = result.unwrap();
                assert_eq!(
                    db.db.get_raw(&VERSION_KEY).unwrap(),
                    Some(serialize_version(CURRENT_VERSION).to_vec())
                );
            }
            Err(expected_err) => {
                assert!(result.is_err());
                assert_eq!(result.unwrap_err(), expected_err);
            }
        }
    }

    #[rstest::rstest]
    #[case::version_missing(None, Some("block database version not found"))]
    #[case::version_invalid(
        Some(vec![0]),
        Some("invalid block database version")
    )]
    #[case::version_too_low(
        Some((CURRENT_VERSION-1).to_be_bytes().to_vec()),
        Some("block database version is not supported")
    )]
    #[case::version_too_high(
        Some((CURRENT_VERSION+1).to_be_bytes().to_vec()),
        Some("block database version is not supported")
    )]
    #[case::version_valid(
        Some(CURRENT_VERSION.to_be_bytes().to_vec()),
        None
    )]
    fn kv_db_backed_block_db_open_checks_block_db_version(
        #[case] version: Option<Vec<u8>>,
        #[case] expected_err_msg: Option<&str>,
    ) {
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_get_raw()
            .with(eq(VERSION_KEY))
            .return_once(|_| Ok(version))
            .times(1);
        let result = KvDbBackedBlockDb::open(kv_db);
        match expected_err_msg {
            None => assert!(result.is_ok()),
            Some(expected_err) => {
                assert_matches!(
                    result, Err(Error::StorageLayer(msg)) if msg.contains(expected_err)
                );
            }
        }
    }

    #[rstest::rstest]
    #[case::version_missing(None, Some("block database version not found"))]
    #[case::version_invalid(
        Some(vec![0]),
        Some("invalid block database version")
    )]
    #[case::version_too_low(
        Some((CURRENT_VERSION-1).to_be_bytes().to_vec()),
        Some("block database version is not supported")
    )]
    #[case::version_too_high(
        Some((CURRENT_VERSION+1).to_be_bytes().to_vec()),
        Some("block database version is not supported")
    )]
    #[case::version_valid(
        Some(CURRENT_VERSION.to_be_bytes().to_vec()),
        None
    )]
    fn rocks_block_db_open_checks_block_db_version(
        #[case] version: Option<Vec<u8>>,
        #[case] expected_err_msg: Option<&str>,
    ) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let rocks = RocksDb::create(tmpdir.path()).unwrap();
        if let Some(v) = version {
            rocks.put_raw(&VERSION_KEY, &v).unwrap();
        }
        let result = KvDbBackedBlockDb::open(rocks);
        match expected_err_msg {
            None => assert!(result.is_ok()),
            Some(expected_err) => {
                assert_matches!(
                    result, Err(Error::StorageLayer(msg)) if msg.contains(expected_err)
                );
            }
        }
    }

    #[rstest::rstest]
    #[case::valid_chain_ids(
        Some(serialize_chain_ids([1u64, 2u64])),
        Ok(vec![1u64, 2u64]),
    )]
    #[case::no_chain_ids(None, Ok(vec![]))]
    #[case::invalid_chain_ids(
        Some(vec![0]),
        Err(Error::StorageLayer(
            "invalid chain IDs length: data length 1 not a multiple of 8 bytes".to_owned()
        )),
    )]
    fn kv_db_backed_block_db_get_chain_ids_returns_chain_ids_stored_at_chain_ids_key(
        #[case] stored_payload: Option<Vec<u8>>,
        #[case] expected: Result<Vec<u64>, Error>,
    ) {
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_get_raw()
            .with(eq(CHAIN_IDS_KEY))
            .return_once(move |_| Ok(stored_payload))
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        assert_eq!(db.get_chain_ids(), expected);
    }

    #[rstest::rstest]
    #[case::valid_chain_ids(
        Some(serialize_chain_ids([1u64, 2u64])),
        Ok(vec![1u64, 2u64]),
    )]
    #[case::no_chain_ids(None, Ok(vec![]))]
    #[case::invalid_chain_ids(
        Some(vec![0]),
        Err(Error::StorageLayer(
            "invalid chain IDs length: data length 1 not a multiple of 8 bytes".to_owned()
        )),
    )]
    fn rocks_block_db_get_chain_ids_returns_chain_ids_stored_at_chain_ids_key(
        #[case] stored_payload: Option<Vec<u8>>,
        #[case] expected: Result<Vec<u64>, Error>,
    ) {
        let (_tmpdir, db) = create_rocks_block_db();
        if let Some(payload) = stored_payload {
            db.db.put_raw(&CHAIN_IDS_KEY, &payload).unwrap();
        }
        assert_eq!(db.get_chain_ids(), expected);
    }

    #[rstest::rstest]
    #[case::valid_ranges(
        Some(serialize_block_ranges(vec![0..=1, 2..=3])),
        Ok(vec![0..=1, 2..=3]),
    )]
    #[case::no_ranges(None, Ok(vec![]))]
    #[case::invalid_ranges(
        Some(vec![0; 8]),
        Err(Error::StorageLayer(
            "invalid block ranges length: data length 8 not a multiple of 16 bytes".to_owned()
        )),
    )]
    fn kv_db_backed_block_db_get_ranges_of_chain_id_returns_ranges_stored_at_key(
        #[case] stored_payload: Option<Vec<u8>>,
        #[case] expected: Result<Vec<BlockRange>, Error>,
    ) {
        let chain_id = 1;
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_get_raw()
            .with(eq(make_block_ranges_key(chain_id)))
            .return_once(move |_| Ok(stored_payload))
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        assert_eq!(db.get_ranges_of_chain_id(chain_id), expected);
    }

    #[rstest::rstest]
    #[case::valid_ranges(
        Some(serialize_block_ranges(vec![0..=1, 2..=3])),
        Ok(vec![0..=1, 2..=3]),
    )]
    #[case::no_ranges(None, Ok(vec![]))]
    #[case::invalid_ranges(
        Some(vec![0; 8]),
        Err(Error::StorageLayer(
            "invalid block ranges length: data length 8 not a multiple of 16 bytes".to_owned()
        )),
    )]
    fn rocks_block_db_get_ranges_of_chain_id_returns_ranges_stored_at_key(
        #[case] stored_payload: Option<Vec<u8>>,
        #[case] expected: Result<Vec<BlockRange>, Error>,
    ) {
        let chain_id = 1;
        let (_tmpdir, db) = create_rocks_block_db();
        if let Some(payload) = stored_payload {
            db.db
                .put_raw(&make_block_ranges_key(chain_id), &payload)
                .unwrap();
        }
        assert_eq!(db.get_ranges_of_chain_id(chain_id), expected);
    }

    #[rstest::rstest]
    #[case::no_upgrade_heights(None)]
    #[case::upgrade_heights(Some(vec![1, 2, 3]))]
    fn kv_db_backed_block_db_get_upgrade_heights_returns_data_stored_at_key(
        #[case] stored_payload: Option<Vec<u8>>,
    ) {
        let chain_id = 1;
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_get_raw()
            .with(eq(make_upgrade_heights_key(chain_id)))
            .return_once({
                let values = stored_payload.clone();
                move |_| Ok(values)
            })
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        let result = db.get_upgrade_heights(chain_id);
        assert_eq!(result, Ok(stored_payload));
    }

    #[rstest::rstest]
    #[case::no_upgrade_heights(None)]
    #[case::upgrade_heights(Some(vec![1, 2, 3]))]
    fn rocks_block_db_get_upgrade_heights_returns_data_stored_at_key(
        #[case] stored_payload: Option<Vec<u8>>,
    ) {
        let chain_id = 1;
        let (_tmpdir, db) = create_rocks_block_db();
        if let Some(payload) = &stored_payload {
            db.db
                .put_raw(&make_upgrade_heights_key(chain_id), payload)
                .unwrap();
        }
        let result = db.get_upgrade_heights(chain_id);
        assert_eq!(result, Ok(stored_payload));
    }

    #[rstest::rstest]
    #[case::chain_id_exists(vec![1, 2, 3])]
    #[case::chain_id_not_exists(vec![1, 3])]
    fn kv_db_backed_block_db_put_upgrade_heights_stores_data_at_key_and_adds_chain_id(
        #[case] existing_chain_ids: Vec<u64>,
    ) {
        let chain_id = 2;
        let data = vec![1, 2, 3];
        let mut kv_db = MockKvDb::new();
        let mut kv_db_batch = MockKvDbBatch::new();
        kv_db
            .expect_get_raw()
            .with(eq(CHAIN_IDS_KEY))
            .return_once(move |_| Ok(Some(serialize_chain_ids(existing_chain_ids))))
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(eq(make_upgrade_heights_key(chain_id)), eq(data.clone()))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(eq(CHAIN_IDS_KEY), eq(serialize_chain_ids([1, 2, 3])))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_size()
            .return_once(move || UNIQUE_IDENTIFIER)
            .times(1);
        kv_db
            .expect_batch_raw()
            .return_once(move || kv_db_batch)
            .times(1);
        kv_db
            .expect_write_batch_raw()
            .withf(move |b| b.size() == UNIQUE_IDENTIFIER)
            .return_once(|_| Ok(()))
            .times(1);
        let mut db = KvDbBackedBlockDb { db: kv_db };
        assert!(db.put_upgrade_heights(chain_id, &data).is_ok());
    }

    #[rstest::rstest]
    #[case::chain_id_exists(vec![1, 2, 3])]
    #[case::chain_id_not_exists(vec![1, 3])]
    fn rocks_block_db_put_upgrade_heights_stores_data_at_key_and_adds_chain_id(
        #[case] existing_chain_ids: Vec<u64>,
    ) {
        let chain_id = 2;
        let data = vec![1, 2, 3];
        let (_tmpdir, mut db) = create_rocks_block_db();
        db.db
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids(existing_chain_ids))
            .unwrap();
        db.put_upgrade_heights(chain_id, &data).unwrap();

        assert_eq!(
            db.db.get_raw(&make_upgrade_heights_key(chain_id)).unwrap(),
            Some(data)
        );
        assert_eq!(
            db.db.get_raw(&CHAIN_IDS_KEY).unwrap(),
            Some(serialize_chain_ids([1, 2, 3]))
        );
    }

    #[rstest::rstest]
    #[case::no_corrections(None)]
    #[case::corrections(Some(vec![1, 2, 3]))]
    fn kv_db_backed_block_db_get_corrections_returns_data_stored_at_key(
        #[case] stored_payload: Option<Vec<u8>>,
    ) {
        let chain_id = 1;
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_get_raw()
            .with(eq(make_corrections_key(chain_id)))
            .return_once({
                let values = stored_payload.clone();
                move |_| Ok(values)
            })
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        let result = db.get_corrections(chain_id);
        assert_eq!(result, Ok(stored_payload));
    }

    #[rstest::rstest]
    #[case::no_corrections(None)]
    #[case::corrections(Some(vec![1, 2, 3]))]
    fn rocks_block_db_get_corrections_returns_data_stored_at_key(
        #[case] stored_payload: Option<Vec<u8>>,
    ) {
        let chain_id = 1;
        let (_tmpdir, db) = create_rocks_block_db();
        if let Some(payload) = &stored_payload {
            db.db
                .put_raw(&make_corrections_key(chain_id), payload)
                .unwrap();
        }
        let result = db.get_corrections(chain_id);
        assert_eq!(result, Ok(stored_payload));
    }

    #[rstest::rstest]
    #[case::chain_id_exists(vec![1, 2, 3])]
    #[case::chain_id_not_exists(vec![1, 3])]
    fn kv_db_backed_block_db_put_corrections_stores_data_at_key_and_adds_chain_id(
        #[case] existing_chain_ids: Vec<u64>,
    ) {
        let chain_id = 2;
        let data = vec![1, 2, 3];
        let mut kv_db = MockKvDb::new();
        let mut kv_db_batch = MockKvDbBatch::new();
        kv_db
            .expect_get_raw()
            .with(eq(CHAIN_IDS_KEY))
            .return_once(move |_| Ok(Some(serialize_chain_ids(existing_chain_ids))))
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(eq(make_corrections_key(chain_id)), eq(data.clone()))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(eq(CHAIN_IDS_KEY), eq(serialize_chain_ids([1, 2, 3])))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_size()
            .return_once(move || UNIQUE_IDENTIFIER)
            .times(1);
        kv_db
            .expect_batch_raw()
            .return_once(move || kv_db_batch)
            .times(1);
        kv_db
            .expect_write_batch_raw()
            .withf(move |b| b.size() == UNIQUE_IDENTIFIER)
            .return_once(|_| Ok(()))
            .times(1);
        let mut db = KvDbBackedBlockDb { db: kv_db };
        assert!(db.put_corrections(chain_id, &data).is_ok());
    }

    #[rstest::rstest]
    #[case::chain_id_exists(vec![1, 2, 3])]
    #[case::chain_id_not_exists(vec![1, 3])]
    fn rocks_block_db_put_corrections_stores_data_at_key_and_adds_chain_id(
        #[case] existing_chain_ids: Vec<u64>,
    ) {
        let chain_id = 2;
        let data = vec![1, 2, 3];
        let (_tmpdir, mut db) = create_rocks_block_db();
        db.db
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids(existing_chain_ids))
            .unwrap();
        db.put_corrections(chain_id, &data).unwrap();

        assert_eq!(
            db.db.get_raw(&make_corrections_key(chain_id)).unwrap(),
            Some(data)
        );
        assert_eq!(
            db.db.get_raw(&CHAIN_IDS_KEY).unwrap(),
            Some(serialize_chain_ids([1, 2, 3]))
        );
    }

    #[test]
    fn kv_db_backed_block_db_get_returns_parsed_block() {
        let chain_id = 1;
        let block = some_block();
        let encoded = proto::Block::from(block.clone()).encode_to_vec();
        let block_key = make_block_key(chain_id, block.number);
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_get_raw()
            .with(eq(block_key))
            .return_once(move |_| Ok(Some(encoded)))
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        let received = db.get(chain_id, block.number).unwrap().unwrap();
        assert_eq!(received, block);
    }

    #[test]
    fn rocks_block_db_get_returns_parsed_block() {
        let chain_id = 1;
        let block = some_block();
        let encoded = proto::Block::from(block.clone()).encode_to_vec();
        let (_tmpdir, db) = create_rocks_block_db();
        db.db
            .put_raw(&make_block_key(chain_id, block.number), &encoded)
            .unwrap();
        let received = db.get(chain_id, block.number).unwrap().unwrap();
        assert_eq!(received, block);
    }

    #[test]
    fn kv_db_backed_block_db_get_returns_error_if_parsing_fails() {
        let chain_id = 1;
        let block_number = 0;
        let mut kv_db = MockKvDb::new();
        let block_key = make_block_key(chain_id, block_number);
        kv_db
            .expect_get_raw()
            .with(eq(block_key))
            .return_once(|_| Ok(Some(vec![0, 1, 2, 3])))
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        let result = db.get(chain_id, block_number);
        assert_matches!(result, Err(Error::Protobuf(_)));
    }

    #[test]
    fn rocks_block_db_get_returns_error_if_parsing_fails() {
        let chain_id = 1;
        let block_number = 0;
        let (_tmpdir, db) = create_rocks_block_db();
        db.db
            .put_raw(&make_block_key(chain_id, block_number), &[1, 2, 3])
            .unwrap();
        let result = db.get(chain_id, block_number);
        assert_matches!(result, Err(Error::Protobuf(_)));
    }

    #[test]
    fn kv_db_backed_block_db_get_bytes_returns_raw_data() {
        let chain_id = 1;
        let block_number = 2;
        let block_key = make_block_key(chain_id, block_number);
        let raw_data = [0, 1, 2];
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_get_raw()
            .with(eq(block_key))
            .return_once(move |_| Ok(Some(raw_data.to_vec())))
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        let received = db.get_bytes(chain_id, block_number).unwrap().unwrap();
        assert_eq!(received, raw_data);
    }

    #[test]
    fn rocks_block_db_get_bytes_returns_raw_data() {
        let chain_id = 1;
        let block_number = 2;
        let raw_data = [0, 1, 2];
        let (_tmpdir, db) = create_rocks_block_db();
        db.db
            .put_raw(&make_block_key(chain_id, block_number), &raw_data)
            .unwrap();
        let received = db.get_bytes(chain_id, block_number).unwrap().unwrap();
        assert_eq!(received, raw_data);
    }

    #[test]
    fn kv_db_backed_block_db_get_bytes_returns_none_for_missing_block() {
        let mut kv_db = MockKvDb::new();
        kv_db.expect_get_raw().return_once(|_| Ok(None)).times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        assert_eq!(db.get_bytes(1, 2).unwrap(), None);
    }

    #[test]
    fn rocks_block_db_get_bytes_returns_none_for_missing_block() {
        let (_tmpdir, db) = create_rocks_block_db();
        assert_eq!(db.get_bytes(1, 2).unwrap(), None);
    }

    #[test]
    fn kv_db_backed_block_db_put_stores_encoded_block_as_protobuf_using_number_from_block_as_key() {
        let chain_id: u64 = 1;
        let block = some_block();
        let encoded = proto::Block::from(block.clone()).encode_to_vec();

        let mut kv_db = MockKvDb::new();
        let mut kv_db_batch = MockKvDbBatch::new();
        kv_db
            .expect_get_raw()
            .with(eq(CHAIN_IDS_KEY))
            .return_once(|_| Ok(None))
            .times(1);
        kv_db
            .expect_get_raw()
            .with(eq(make_block_ranges_key(chain_id)))
            .return_once(|_| Ok(None))
            .times(1);
        // expect to store the protobuf-encoded block under block.number
        kv_db_batch
            .expect_put_raw()
            .with(eq(make_block_key(chain_id, block.number)), eq(encoded))
            .return_once(|_, _| ())
            .times(1);
        // expect to update ranges
        kv_db_batch
            .expect_put_raw()
            .with(
                eq(make_block_ranges_key(chain_id)),
                eq(serialize_block_ranges([block.number..=block.number])),
            )
            .return_once(|_, _| ())
            .times(1);
        // expect to update chain IDs with [1]
        kv_db_batch
            .expect_put_raw()
            .with(eq(CHAIN_IDS_KEY), eq(serialize_chain_ids([chain_id])))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_size()
            .return_once(move || UNIQUE_IDENTIFIER)
            .times(1);
        kv_db
            .expect_batch_raw()
            .return_once(move || kv_db_batch)
            .times(1);
        kv_db
            .expect_write_batch_raw()
            .withf(move |b| b.size() == UNIQUE_IDENTIFIER)
            .return_once(|_| Ok(()))
            .times(1);

        let mut db = KvDbBackedBlockDb { db: kv_db };
        db.put(chain_id, block).unwrap();
    }

    #[test]
    fn rocks_block_db_put_stores_encoded_block_as_protobuf_using_number_from_block_as_key() {
        let chain_id: u64 = 1;
        let block = some_block();
        let encoded = proto::Block::from(block.clone()).encode_to_vec();

        let (_tmpdir, mut db) = create_rocks_block_db();
        db.put(chain_id, block.clone()).unwrap();

        assert_eq!(
            db.db
                .get_raw(&make_block_key(chain_id, block.number))
                .unwrap(),
            Some(encoded)
        );
    }

    #[rstest::rstest]
    #[case::no_existing_metadata(vec![], vec![], 1, 11, vec![1], vec![11..=11])]
    #[case::only_other_chain_ids(vec![1, 3], vec![], 2, 11, vec![1, 2, 3], vec![11..=11])]
    #[case::same_chain_id(vec![1, 2], vec![], 2, 11, vec![1, 2], vec![11..=11])]
    #[case::same_chain_id_with_overlapping_ranges(vec![1, 2], vec![11..=12, 14..=15], 2, 13, vec![1, 2], vec![11..=15])]
    #[case::same_chain_id_with_non_overlapping_ranges(vec![1, 2], vec![11..=12, 16..=17], 2, 14, vec![1, 2], vec![11..=12, 14..=14, 16..=17])]
    fn kv_db_backed_block_db_put_bytes_stores_data_and_updates_metadata_using_batch(
        #[case] existing_chain_ids: Vec<u64>,
        #[case] existing_ranges: Vec<BlockRange>,
        #[case] chain_id: u64,
        #[case] block_number: u64,
        #[case] new_chain_ids: Vec<u64>,
        #[case] new_ranges: Vec<BlockRange>,
    ) {
        let mut kv_db = MockKvDb::new();
        let mut kv_db_batch = MockKvDbBatch::new();
        kv_db
            .expect_get_raw()
            .with(eq(CHAIN_IDS_KEY))
            .return_once(move |_| Ok(Some(serialize_chain_ids(existing_chain_ids))))
            .times(1);
        kv_db
            .expect_get_raw()
            .with(eq(make_block_ranges_key(chain_id)))
            .return_once(move |_| Ok(Some(serialize_block_ranges(existing_ranges))))
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(
                eq(make_block_key(chain_id, block_number)),
                eq(b"data".to_vec()),
            )
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(
                eq(make_block_ranges_key(chain_id)),
                eq(serialize_block_ranges(new_ranges)),
            )
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(eq(CHAIN_IDS_KEY), eq(serialize_chain_ids(new_chain_ids)))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_size()
            .return_once(move || UNIQUE_IDENTIFIER)
            .times(1);
        kv_db
            .expect_batch_raw()
            .return_once(move || kv_db_batch)
            .times(1);
        kv_db
            .expect_write_batch_raw()
            .withf(move |b| b.size() == UNIQUE_IDENTIFIER)
            .return_once(|_| Ok(()))
            .times(1);

        let mut db = KvDbBackedBlockDb { db: kv_db };
        db.put_bytes(chain_id, block_number, b"data").unwrap();
    }

    #[rstest::rstest]
    #[case::no_existing_metadata(None, None, 1, 11, vec![1], vec![11..=11])]
    #[case::no_ranges(Some(vec![1, 3]), None, 2, 11, vec![1, 2, 3], vec![11..=11])]
    #[case::empty_existing_metadata(Some(vec![]), Some(vec![]), 1, 11, vec![1], vec![11..=11])]
    #[case::empty_ranges(Some(vec![1, 3]), Some(vec![]), 2, 11, vec![1, 2, 3], vec![11..=11])]
    #[case::same_chain_id(Some(vec![1, 2]), Some(vec![]), 2, 11, vec![1, 2], vec![11..=11])]
    #[case::same_chain_id_with_overlapping_ranges(Some(vec![1, 2]), Some(vec![11..=12, 14..=15]), 2, 13, vec![1, 2], vec![11..=15])]
    #[case::same_chain_id_with_non_overlapping_ranges(Some(vec![1, 2]), Some(vec![11..=12, 16..=17]), 2, 14, vec![1, 2], vec![11..=12, 14..=14, 16..=17])]
    fn rocks_block_db_put_bytes_stores_data_and_updates_metadata(
        #[case] existing_chain_ids: Option<Vec<u64>>,
        #[case] existing_ranges: Option<Vec<BlockRange>>,
        #[case] chain_id: u64,
        #[case] block_number: u64,
        #[case] new_chain_ids: Vec<u64>,
        #[case] new_ranges: Vec<BlockRange>,
    ) {
        let (_tmpdir, mut db) = create_rocks_block_db();

        if let Some(existing_chain_ids) = existing_chain_ids {
            db.db
                .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids(existing_chain_ids))
                .unwrap();
        }
        if let Some(existing_ranges) = existing_ranges {
            db.db
                .put_raw(
                    &make_block_ranges_key(chain_id),
                    &serialize_block_ranges(existing_ranges.clone()),
                )
                .unwrap();
        }

        db.put_bytes(chain_id, block_number, b"data").unwrap();

        assert_eq!(
            db.db
                .get_raw(&make_block_key(chain_id, block_number))
                .unwrap(),
            Some(b"data".to_vec())
        );
        assert_eq!(
            db.db.get_raw(&make_block_ranges_key(chain_id)).unwrap(),
            Some(serialize_block_ranges(new_ranges))
        );
        assert_eq!(
            db.db.get_raw(&CHAIN_IDS_KEY).unwrap(),
            Some(serialize_chain_ids(new_chain_ids))
        );
    }

    #[rstest::rstest]
    #[case::no_existing_ranges_for_chain_id(None, 2, 3, vec![])]
    #[case::existing_ranges_for_chain_id_delete_parts_of_multiple(Some(vec![0..=1, 3..=4, 6..=7]), 1, 6, vec![0..=0, 7..=7])]
    #[case::existing_ranges_for_chain_id_delete_middle(Some(vec![0..=5]), 2, 3, vec![0..=1, 4..=5])]
    fn kv_db_backed_block_db_delete_range_deletes_blocks_in_range_and_updates_metadata(
        #[case] existing_ranges: Option<Vec<BlockRange>>,
        #[case] start_block_number: u64,
        #[case] end_block_number: u64,
        #[case] expected_ranges: Vec<BlockRange>,
    ) {
        let chain_id: u64 = 11;

        let mut kv_db = MockKvDb::new();
        let mut kv_db_batch = MockKvDbBatch::new();
        kv_db
            .expect_get_raw()
            .with(eq(make_block_ranges_key(chain_id)))
            .return_once(move |_| Ok(existing_ranges.map(serialize_block_ranges)))
            .times(1);
        kv_db_batch
            .expect_delete_range_raw()
            .with(
                eq(make_block_key(chain_id, start_block_number)),
                eq(make_block_key(chain_id, end_block_number)),
            )
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(
                eq(make_block_ranges_key(chain_id)),
                eq(serialize_block_ranges(expected_ranges)),
            )
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_size()
            .return_once(move || UNIQUE_IDENTIFIER)
            .times(1);
        kv_db
            .expect_batch_raw()
            .return_once(move || kv_db_batch)
            .times(1);
        kv_db
            .expect_write_batch_raw()
            .withf(move |b| b.size() == UNIQUE_IDENTIFIER) // this is to ensure that the mock batch is the one that gets written
            .return_once(|_| Ok(()))
            .times(1);

        let mut db = KvDbBackedBlockDb { db: kv_db };
        db.delete_range(chain_id, start_block_number, end_block_number)
            .unwrap();
    }

    #[rstest::rstest]
    #[case::no_existing_ranges_for_chain_id(vec![], 2, 3, vec![])]
    #[case::existing_ranges_for_chain_id_delete_parts_of_multiple(vec![0..=1, 3..=4, 6..=7], 1, 6, vec![0..=0, 7..=7])]
    #[case::existing_ranges_for_chain_id_delete_middle(vec![0..=5], 2, 3, vec![0..=1, 4..=5])]
    fn rocks_block_db_delete_range_deletes_blocks_in_range_and_updates_metadata(
        #[case] existing_ranges: Vec<BlockRange>,
        #[case] start_block_number: u64,
        #[case] end_block_number: u64,
        #[case] expected_ranges: Vec<BlockRange>,
    ) {
        let chain_id: u64 = 11;
        let (_tmpdir, mut db) = create_rocks_block_db();

        for range in &existing_ranges {
            for block_num in range.clone() {
                db.put_bytes(chain_id, block_num, b"block").unwrap();
            }
        }

        db.delete_range(chain_id, start_block_number, end_block_number)
            .unwrap();

        for range in existing_ranges {
            for block_num in range {
                assert_eq!(
                    db.get_bytes(chain_id, block_num).unwrap().is_some(),
                    !(start_block_number..=end_block_number).contains(&block_num)
                );
            }
        }

        assert_eq!(
            db.get_ranges_of_chain_id(chain_id).unwrap(),
            expected_ranges
        );
    }

    #[test]
    fn kv_db_backed_block_db_delete_range_returns_error_if_start_is_greater_than_end() {
        let mut db = KvDbBackedBlockDb {
            db: MockKvDb::new(),
        };
        let result = db.delete_range(1, 10, 5);
        assert_eq!(
            result,
            Err(Error::StorageLayer(
                "invalid block range: start block number 10 is greater than end block number 5"
                    .to_owned()
            ))
        );
    }

    #[test]
    fn rocks_block_db_delete_range_returns_error_if_start_is_greater_than_end() {
        let (_tmpdir, mut db) = create_rocks_block_db();
        let result = db.delete_range(1, 10, 5);
        assert_eq!(
            result,
            Err(Error::StorageLayer(
                "invalid block range: start block number 10 is greater than end block number 5"
                    .to_owned()
            ))
        );
    }

    #[rstest::rstest]
    #[case::valid_raw_block(
        Box::from(proto::Block::from(some_block()).encode_to_vec()),
        Some((1, some_block())),
    )]
    #[case::invalid_raw_block(
        Box::from([0u8, 1, 2, 3].as_slice()),
        None,
    )]
    fn kv_db_backed_block_db_iterate_parses_blocks(
        #[case] raw_block: Box<[u8]>,
        #[case] expected_block: Option<(u64, Block)>,
    ) {
        let chain_id = 1;
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_iterate_raw()
            .return_once(move |_start, _dir| {
                Box::new(
                    [Ok((
                        Box::from(make_block_key(chain_id, 1).as_slice()),
                        raw_block,
                    ))]
                    .into_iter(),
                )
            })
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };

        let result = db.iterate(chain_id, 1, IterationDirection::Forward).next();

        match expected_block {
            Some(expected) => assert_eq!(result, Some(Ok(expected))),
            None => assert_matches!(result, Some(Err(Error::Protobuf(_)))),
        }
    }

    #[rstest::rstest]
    #[case::valid_raw_block(
        Box::from(proto::Block::from(some_block()).encode_to_vec()),
        Some((1, some_block())),
    )]
    #[case::invalid_raw_block(
        Box::from([0u8, 1, 2, 3].as_slice()),
        None,
    )]
    fn rocks_block_db_iterate_parses_blocks(
        #[case] raw_block: Box<[u8]>,
        #[case] expected_block: Option<(u64, Block)>,
    ) {
        let chain_id = 1;
        let (_tmpdir, db) = create_rocks_block_db();
        db.db
            .put_raw(&make_block_key(chain_id, 1), &raw_block)
            .unwrap();

        let result = db.iterate(chain_id, 1, IterationDirection::Forward).next();

        match expected_block {
            Some(expected) => assert_eq!(result, Some(Ok(expected))),
            None => assert_matches!(result, Some(Err(Error::Protobuf(_)))),
        }
    }

    type ByteVecTuple = (Vec<u8>, Vec<u8>);

    #[rstest::rstest]
    // case starting_at_block_number does not make sense for a mock backed DB because the mock can
    // be set up to return any sequence of items regardless of the start key
    // case until_no_more_blocks does not make sense for a mock backed DB because the mock stops
    // returning items depending on how it's set up, not based on the presence or absence of block
    // keys.
    #[case::until_chain_boundary(
        vec![
            Ok((make_block_key(1, 1).to_vec(), b"chain1-block1".to_vec())),
            Ok((make_block_key(2, 1).to_vec(), b"chain2-block1".to_vec())),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_metadata_key(
        vec![
            Ok((make_block_key(1, 1).to_vec(), b"chain1-block1".to_vec())),
            Ok((make_block_ranges_key(1).to_vec(), b"metadata".to_vec())),
            Ok((make_block_key(1, 2).to_vec(), b"chain1-block2".to_vec())),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_key_of_wrong_length(
        vec![
            Ok((make_block_key(1, 1).to_vec(), b"block1-1".to_vec())),
            Ok(([make_block_key(1, 1).as_slice(), &[0]].concat(), b"invalid".to_vec())),
            Ok((make_block_key(1, 2).to_vec(), b"block1-2".to_vec())),
        ],
        vec![
            Ok((1, Box::from(b"block1-1".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0]"
                    .to_owned(),
            )),
        ]
    )]
    #[case::until_key_with_wrong_prefix(
        vec![
            Ok((make_block_key(1, 1).to_vec(), b"block1-1".to_vec())),
            Ok((
                [[0x01u8].as_slice(), make_block_key(1, 2)[1..].as_ref()].concat(),
                b"invalid".to_vec(),
            )),
            Ok((make_block_key(1, 2).to_vec(), b"block1-2".to_vec())),
        ],
        vec![
            Ok((1, Box::from(b"block1-1".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2]"
                    .to_owned(),
            )),
        ]
    )]
    #[case::until_db_error(
        vec![
            Ok((make_block_key(1, 4).to_vec(), b"block1-4".to_vec())),
            Err(Error::StorageLayer("boom".to_owned())),
            Ok((make_block_key(1, 5).to_vec(), b"block1-5".to_vec())),
        ],
        vec![
            Ok((4, Box::from(b"block1-4".as_slice()))),
            Err(Error::StorageLayer("boom".to_owned())),
        ]
    )]
    fn kv_db_backed_block_db_iterate_bytes_iterates_over_all_valid_blocks_for_chain_id(
        #[case] raw_items: Vec<Result<ByteVecTuple, Error>>,
        #[case] expected: Vec<Result<IterBytesItem, Error>>,
    ) {
        let chain_id = 1;
        let start_block_number = 1;
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_iterate_raw()
            .with(
                eq(Box::<[u8]>::from(
                    make_block_key(chain_id, start_block_number).as_slice(),
                )),
                eq(IterationDirection::Forward),
            )
            .return_once(move |_start, _dir| {
                Box::new(
                    raw_items
                        .into_iter()
                        .map(|res| res.map(|(k, v)| (k.into_boxed_slice(), v.into_boxed_slice()))),
                )
            })
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };

        let blocks: Vec<_> = db
            .iterate_bytes(chain_id, start_block_number, IterationDirection::Forward)
            .collect();
        assert_eq!(blocks, expected);
    }

    #[rstest::rstest]
    #[case::starting_at_block_number(
        vec![
            (make_block_key(1, 0).to_vec(), b"block0".to_vec()),
            (make_block_key(1, 1).to_vec(), b"block1".to_vec()),
            (make_block_key(1, 2).to_vec(), b"block2".to_vec()),
        ],
        vec![
            Ok((1, Box::from(b"block1".as_slice()))),
            Ok((2, Box::from(b"block2".as_slice()))),
        ]
    )]
    #[case::until_no_more_blocks(
        vec![
            (make_block_key(1, 1).to_vec(), b"block1".to_vec()),
            (make_block_key(1, 3).to_vec(), b"block3".to_vec()),
        ],
        vec![
            Ok((1, Box::from(b"block1".as_slice()))),
            Ok((3, Box::from(b"block3".as_slice()))),
        ]
    )]
    #[case::until_chain_boundary(
        vec![
            (make_block_key(1, 1).to_vec(), b"chain1-block1".to_vec()),
            (make_block_key(2, 1).to_vec(), b"chain2-block1".to_vec()),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    // case until_metadata_key does not make sense for forward iteration because metadata keys are
    // always before block keys
    #[case::until_key_with_wrong_length(
        vec![
            (make_block_key(1, 1).to_vec(), b"block1-1".to_vec()),
            ([make_block_key(1, 1).as_slice(), &[0]].concat(), b"invalid".to_vec()),
            (make_block_key(1, 2).to_vec(), b"block1-2".to_vec()),
        ],
        vec![
            Ok((1, Box::from(b"block1-1".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0]"
                    .to_owned(),
            )),
        ]
    )]
    #[case::until_key_with_wrong_prefix(
        vec![
            (make_block_key(1, 1).to_vec(), b"block1-1".to_vec()),
            (
                [[0x03u8].as_slice(), make_block_key(1, 2)[1..].as_ref()].concat(),
                b"invalid".to_vec(),
            ),
        ],
        vec![
            Ok((1, Box::from(b"block1-1".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2]"
                    .to_owned(),
            )),
        ]
    )]
    fn rocks_block_db_iterate_bytes_iterates_over_all_valid_blocks_for_chain_id(
        #[case] raw_items: Vec<ByteVecTuple>,
        #[case] expected: Vec<Result<IterBytesItem, Error>>,
    ) {
        let chain_id = 1;
        let start_block_number = 1;
        let (_tmpdir, db) = create_rocks_block_db();
        for (key, value) in raw_items {
            db.db.put_raw(&key, &value).unwrap();
        }

        let result: Vec<_> = db
            .iterate_bytes(chain_id, start_block_number, IterationDirection::Forward)
            .collect();
        assert_eq!(result, expected);
    }

    #[rstest::rstest]
    // case starting_at_block_number does not make sense for a mock backed DB because the mock can
    // be set up to return any sequence of items regardless of the start key
    // case until_no_more_blocks does not make sense for a mock backed DB because the mock stops
    // returning items depending on how it's set up, not based on the presence or absence of block
    // keys.
    #[case::until_chain_boundary(
        vec![
            Ok((make_block_key(1, 1).to_vec(), b"chain1-block1".to_vec())),
            Ok((make_block_key(0, 1).to_vec(), b"chain0-block1".to_vec())),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_metadata_key(
        vec![
            Ok((make_block_key(1, 1).to_vec(), b"chain1-block1".to_vec())),
            Ok((make_block_ranges_key(1).to_vec(), b"metadata".to_vec())),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_key_with_wrong_length(
        vec![
            Ok((make_block_key(1, 2).to_vec(), b"block1-2".to_vec())),
            Ok(([make_block_key(1, 1).as_slice(), &[0]].concat(), b"invalid".to_vec())),
            Ok((make_block_key(1, 1).to_vec(), b"block1-1".to_vec())),
        ],
        vec![
            Ok((2, Box::from(b"block1-2".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0]"
                    .to_owned(),
            )),
        ]
    )]
    #[case::until_key_with_wrong_prefix(
        vec![
            Ok((make_block_key(1, 2).to_vec(), b"block1-2".to_vec())),
            Ok((
                [[0x01u8].as_slice(), make_block_key(1, 1)[1..].as_ref()].concat(),
                b"invalid".to_vec(),
            )),
            Ok((make_block_key(1, 1).to_vec(), b"block1-1".to_vec())),
        ],
        vec![
            Ok((2, Box::from(b"block1-2".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1]"
                    .to_owned(),
            )),
        ]
    )]
    fn kv_db_backed_block_db_iterate_bytes_reverse_iterates_over_all_valid_blocks_for_chain_id_in_reverse(
        #[case] raw_items: Vec<Result<ByteVecTuple, Error>>,
        #[case] expected: Vec<Result<IterBytesItem, Error>>,
    ) {
        let chain_id = 1;
        let start_block_number = 3;
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_iterate_raw()
            .with(
                eq(Box::<[u8]>::from(
                    make_block_key(chain_id, start_block_number).as_slice(),
                )),
                eq(IterationDirection::Reverse),
            )
            .return_once(move |_start, _dir| {
                Box::new(
                    raw_items
                        .into_iter()
                        .map(|res| res.map(|(k, v)| (k.into_boxed_slice(), v.into_boxed_slice()))),
                )
            })
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };

        let blocks: Vec<_> = db
            .iterate_bytes(chain_id, start_block_number, IterationDirection::Reverse)
            .collect();
        assert_eq!(blocks, expected);
    }

    #[rstest::rstest]
    #[case::starting_at_block_number(
        vec![
            (make_block_key(1, 2).to_vec(), b"block2".to_vec()),
            (make_block_key(1, 3).to_vec(), b"block3".to_vec()),
            (make_block_key(1, 4).to_vec(), b"block4".to_vec()),
        ],
        vec![
            Ok((3, Box::from(b"block3".as_slice()))),
            Ok((2, Box::from(b"block2".as_slice()))),
        ]
    )]
    #[case::until_no_more_blocks(
        vec![
            (make_block_key(1, 1).to_vec(), b"block1".to_vec()),
            (make_block_key(1, 3).to_vec(), b"block3".to_vec()),
        ],
        vec![
            Ok((3, Box::from(b"block3".as_slice()))),
            Ok((1, Box::from(b"block1".as_slice()))),
        ]
    )]
    #[case::until_chain_boundary(
        vec![
            (make_block_key(0, 1).to_vec(), b"chain0-block1".to_vec()),
            (make_block_key(1, 1).to_vec(), b"chain1-block1".to_vec()),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_metadata_key(
        vec![
            (make_block_key(1, 1).to_vec(), b"chain1-block1".to_vec()),
            (make_block_ranges_key(1).to_vec(), b"metadata".to_vec()),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_key_with_wrong_length(
        vec![
            (make_block_key(1, 1).to_vec(), b"block1-1".to_vec()),
            ([make_block_key(1, 1).as_slice(), &[0]].concat(), b"invalid".to_vec()),
            (make_block_key(1, 2).to_vec(), b"block1-2".to_vec()),
        ],
        vec![
            Ok((2, Box::from(b"block1-2".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0]"
                    .to_owned(),
            )),
        ]
    )]
    #[case::until_key_with_wrong_prefix(
        vec![
            (
                [[0x01u8].as_slice(), make_block_key(1, 2)[1..].as_ref()].concat(),
                b"invalid".to_vec(),
            ),
            (make_block_key(1, 2).to_vec(), b"block1-2".to_vec()),
        ],
        vec![
            Ok((2, Box::from(b"block1-2".as_slice()))),
            Err(Error::StorageLayer(
                "unexpected key [1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2]"
                    .to_owned(),
            )),
        ]
    )]
    fn rocks_block_db_iterate_bytes_reverse_iterates_over_all_valid_blocks_for_chain_id_in_reverse(
        #[case] raw_items: Vec<ByteVecTuple>,
        #[case] expected: Vec<Result<IterBytesItem, Error>>,
    ) {
        let chain_id = 1;
        let start_block_number = 3;
        let (_tmpdir, db) = create_rocks_block_db();
        for (key, value) in raw_items {
            db.db.put_raw(&key, &value).unwrap();
        }

        let result: Vec<_> = db
            .iterate_bytes(chain_id, start_block_number, IterationDirection::Reverse)
            .collect();
        assert_eq!(result, expected);
    }

    #[rstest::rstest]
    #[case::forward(IterationDirection::Forward)]
    #[case::reverse(IterationDirection::Reverse)]
    fn kv_db_backed_block_db_iterate_bytes_passes_start_key_and_direction_to_kv_db_iterate_raw(
        #[case] direction: IterationDirection,
    ) {
        let chain_id = 1;
        let start_block_number = 42;
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_iterate_raw()
            .with(
                eq(Box::<[u8]>::from(
                    make_block_key(chain_id, start_block_number).as_slice(),
                )),
                eq(direction),
            )
            .return_once(move |_start, _dir| Box::new(std::iter::empty()))
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        assert!(
            db.iterate_bytes(chain_id, start_block_number, direction)
                .next()
                .is_none()
        );
    }

    #[rstest::rstest]
    #[case::forward(IterationDirection::Forward, vec![0, 1, 2], 1, vec![1, 2])]
    #[case::reverse(IterationDirection::Reverse, vec![0, 1, 2], 1, vec![1, 0])]
    fn rocks_block_db_iterate_bytes_passes_start_key_and_direction(
        #[case] direction: IterationDirection,
        #[case] block_numbers: Vec<u64>,
        #[case] iter_start_block_number: u64,
        #[case] iter_block_numbers: Vec<u64>,
    ) {
        let chain_id = 1;
        let (_tmpdir, mut db) = create_rocks_block_db();
        for i in block_numbers {
            db.put(
                chain_id,
                Block {
                    number: i,
                    ..Block::default()
                },
            )
            .unwrap();
        }
        assert_eq!(
            db.iterate_bytes(chain_id, iter_start_block_number, direction)
                .map(|res| res.map(|(num, _)| num))
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            iter_block_numbers
        );
    }

    #[test]
    fn kv_db_backed_block_db_write_batch_writes_all_blocks_and_updates_metadata() {
        let existing_chain_ids = [1, 2];
        let existing_ranges_chain_2 = [1..=2];
        let existing_ranges_chain_3 = [];
        let new_blocks = [(2, 2, b"block"), (2, 3, b"block"), (3, 1, b"block")];
        let new_ranges_chain_2 = [1..=3];
        let new_ranges_chain_3 = [1..=1];
        let new_chain_ids = [1, 2, 3];

        let mut kv_db = MockKvDb::new();
        let mut kv_db_batch = MockKvDbBatch::new();
        kv_db
            .expect_get_raw()
            .with(eq(CHAIN_IDS_KEY))
            .return_once(move |_| Ok(Some(serialize_chain_ids(existing_chain_ids))))
            .times(1);
        kv_db
            .expect_get_raw()
            .with(eq(make_block_ranges_key(2)))
            .return_once(move |_| Ok(Some(serialize_block_ranges(existing_ranges_chain_2))))
            .times(1);
        kv_db
            .expect_get_raw()
            .with(eq(make_block_ranges_key(3)))
            .return_once(move |_| Ok(Some(serialize_block_ranges(existing_ranges_chain_3))))
            .times(1);
        for (chain_id, block_number, data) in new_blocks {
            kv_db_batch
                .expect_put_raw()
                .with(
                    eq(make_block_key(chain_id, block_number)),
                    eq(data.to_vec()),
                )
                .return_once(|_, _| ())
                .times(1);
        }
        kv_db_batch
            .expect_put_raw()
            .with(
                eq(make_block_ranges_key(2)),
                eq(serialize_block_ranges(new_ranges_chain_2)),
            )
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(
                eq(make_block_ranges_key(3)),
                eq(serialize_block_ranges(new_ranges_chain_3)),
            )
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(eq(CHAIN_IDS_KEY), eq(serialize_chain_ids(new_chain_ids)))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_size()
            .return_once(move || UNIQUE_IDENTIFIER)
            .times(1);
        kv_db
            .expect_write_batch_raw()
            .withf(move |b| b.size() == UNIQUE_IDENTIFIER) // this is to ensure that the mock batch is the one that gets written
            .return_once(|_| Ok(()))
            .times(1);

        let mut db = KvDbBackedBlockDb { db: kv_db };
        let mut batch = KvDbBatchWrapper {
            kv_batch: kv_db_batch,
            block_ranges: HashMap::default(),
        };
        for (block_chain_id, block_number, block_data) in new_blocks {
            batch.put_bytes(block_chain_id, block_number, block_data);
        }

        db.write_batch(batch).unwrap();
    }

    #[test]
    fn rocks_block_db_write_batch_writes_all_blocks_and_updates_metadata() {
        let (_tmpdir, mut db) = create_rocks_block_db();

        // Set up existing state via kv_db
        db.db
            .put_raw(&CHAIN_IDS_KEY, &serialize_chain_ids([1, 2]))
            .unwrap();
        db.db
            .put_raw(&make_block_ranges_key(2), &serialize_block_ranges([1..=2]))
            .unwrap();
        let existing_blocks = [
            (1, 0, b"existing"),
            (2, 1, b"existing"),
            (2, 2, b"existing"),
        ];
        for (cid, bn, data) in existing_blocks {
            db.db.put_raw(&make_block_key(cid, bn), data).unwrap();
        }

        let new_blocks = [(2, 2, b"block"), (2, 3, b"block"), (3, 1, b"block")];
        let mut batch = db.batch();
        for (chain_id, block_number, data) in new_blocks {
            batch.put_bytes(chain_id, block_number, data);
        }
        db.write_batch(batch).unwrap();

        // Verify data
        for (chain_id, block_number, data) in new_blocks {
            assert_eq!(
                db.db
                    .get_raw(&make_block_key(chain_id, block_number))
                    .unwrap(),
                Some(data.to_vec())
            );
        }
        // Verify metadata
        assert_eq!(
            db.db.get_raw(&CHAIN_IDS_KEY).unwrap(),
            Some(serialize_chain_ids([1, 2, 3]))
        );
        assert_eq!(
            db.db.get_raw(&make_block_ranges_key(2)).unwrap(),
            Some(serialize_block_ranges([1..=3]))
        );
        assert_eq!(
            db.db.get_raw(&make_block_ranges_key(3)).unwrap(),
            Some(serialize_block_ranges([1..=1]))
        );
    }

    #[test]
    fn kv_db_backed_block_db_write_batch_empty_batch_is_noop() {
        let kv_db = MockKvDb::new();
        let kv_db_batch = MockKvDbBatch::new();
        let mut db = KvDbBackedBlockDb { db: kv_db };
        let batch = KvDbBatchWrapper {
            kv_batch: kv_db_batch,
            block_ranges: HashMap::default(),
        };
        db.write_batch(batch).unwrap();
    }

    #[test]
    fn rocks_block_db_write_batch_empty_batch_is_noop() {
        let (_tmpdir, mut db) = create_rocks_block_db();
        let batch = db.batch();
        db.write_batch(batch).unwrap();
    }

    #[test]
    fn kv_db_backed_block_db_try_catch_up_with_primary_calls_try_catch_up_with_primary_on_kv_db() {
        let mut kv_db = MockKvDb::new();
        kv_db
            .expect_try_catch_up_with_primary()
            .return_once(|| Ok(()))
            .times(1);
        let db = KvDbBackedBlockDb { db: kv_db };
        db.try_catch_up_with_primary().unwrap();
    }

    #[test]
    fn rocks_block_db_try_catch_up_with_primary_pulls_in_changes_of_primary() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let write_rocks_db = RocksDb::create(tmpdir.path()).unwrap();
        let mut write_block_db = KvDbBackedBlockDb::create(write_rocks_db).unwrap();
        let read_rocks_db = RocksDb::open_for_reading(tmpdir.path()).unwrap();
        let read_block_db = KvDbBackedBlockDb::open(read_rocks_db).unwrap();

        write_block_db
            .put(
                1,
                Block {
                    number: 1,
                    ..Block::default()
                },
            )
            .unwrap();
        assert_eq!(read_block_db.get(1, 1).unwrap(), None);
        read_block_db.try_catch_up_with_primary().unwrap();
        assert_eq!(
            read_block_db.get(1, 1).unwrap(),
            Some(Block {
                number: 1,
                ..Block::default()
            })
        );
    }

    #[test]
    fn kv_db_batch_wrapper_put_bytes_adds_block_data_and_tracks_metadata() {
        let mut kv_db_batch = MockKvDbBatch::new();
        kv_db_batch
            .expect_put_raw()
            .with(eq(make_block_key(1, 1)), eq(b"block".to_vec()))
            .return_once(|_, _| ())
            .times(1);
        kv_db_batch
            .expect_put_raw()
            .with(eq(make_block_key(1, 2)), eq(b"block".to_vec()))
            .return_once(|_, _| ())
            .times(1);
        let mut batch = KvDbBatchWrapper {
            kv_batch: kv_db_batch,
            block_ranges: HashMap::default(),
        };
        batch.put_bytes(1, 1, b"block");
        batch.put_bytes(1, 2, b"block");
        assert_eq!(batch.block_ranges.get(&1), Some(&vec![1..=2]));
    }

    #[test]
    fn serialize_version_returns_8_byte_be_version() {
        assert_eq!(serialize_version(1), [0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[rstest::rstest]
    #[case::invalid_version(
        vec![0],
        Err(Error::StorageLayer("invalid block database version length: expected 8 bytes, got 1 bytes".to_owned()))
    )]
    #[case::valid_version(vec![0, 0, 0, 0, 0, 0, 0, 1], Ok(1))]
    fn deserialize_version_parses_8_byte_be_version(
        #[case] data: Vec<u8>,
        #[case] expected: Result<u64, Error>,
    ) {
        assert_eq!(deserialize_version(data), expected);
    }

    #[test]
    fn serialize_chain_ids_returns_concatenation_of_8_byte_be_chain_ids() {
        let chain_ids = [258, 259, 260];
        assert_eq!(
            serialize_chain_ids(chain_ids),
            [
                0, 0, 0, 0, 0, 0, 1, 2, // chain ID 1
                0, 0, 0, 0, 0, 0, 1, 3, // chain ID 2
                0, 0, 0, 0, 0, 0, 1, 4, // chain ID 3
            ]
        );
    }

    #[test]
    fn deserialize_chain_ids_parses_8_byte_be_chain_ids() {
        let data = [
            0, 0, 0, 0, 0, 0, 1, 2, // chain ID 1
            0, 0, 0, 0, 0, 0, 1, 3, // chain ID 2
            0, 0, 0, 0, 0, 0, 1, 4, // chain ID 3
        ];
        let result = deserialize_chain_ids(data).unwrap();
        assert_eq!(result, vec![258u64, 259u64, 260u64]);
    }

    #[test]
    fn deserialize_chain_ids_returns_error_if_data_length_is_not_multiple_of_8() {
        let data = [0, 1, 2];
        let result = deserialize_chain_ids(data);
        assert_eq!(
            result,
            Err(Error::StorageLayer(
                "invalid chain IDs length: data length 3 not a multiple of 8 bytes".to_owned()
            ))
        );
    }

    #[test]
    fn make_block_ranges_key_returns_10_byte_key_consisting_of_prefix_0x01_and_be_chain_id_and_suffix_0x00()
     {
        let chain_id = 258;
        let key = make_block_ranges_key(chain_id);
        assert_eq!(
            key,
            [
                1, // prefix
                0, 0, 0, 0, 0, 0, 1, 2, // chain ID
                0, // suffix
            ]
        );
    }

    #[test]
    fn serialize_block_ranges_returns_concatenation_of_16_byte_be_start_end_pairs() {
        let ranges = [258..=259, 260..=261];
        assert_eq!(
            serialize_block_ranges(ranges),
            [
                0, 0, 0, 0, 0, 0, 1, 2, // start of range 1
                0, 0, 0, 0, 0, 0, 1, 3, // end of range 1
                0, 0, 0, 0, 0, 0, 1, 4, // start of range 2
                0, 0, 0, 0, 0, 0, 1, 5, // end of range 2
            ]
        );
    }

    #[test]
    fn deserialize_block_ranges_parses_16_byte_be_start_end_pairs() {
        let data = [
            0, 0, 0, 0, 0, 0, 1, 2, // start of range 1
            0, 0, 0, 0, 0, 0, 1, 3, // end of range 1
            0, 0, 0, 0, 0, 0, 1, 4, // start of range 2
            0, 0, 0, 0, 0, 0, 1, 5, // end of range 2
        ];
        let result = deserialize_block_ranges(data).unwrap();
        assert_eq!(result, vec![258..=259, 260..=261]);
    }

    #[test]
    fn deserialize_block_ranges_returns_error_if_data_length_is_not_multiple_of_16() {
        let data = [0, 1, 2];
        let result = deserialize_block_ranges(data);
        assert_eq!(
            result,
            Err(Error::StorageLayer(
                "invalid block ranges length: data length 3 not a multiple of 16 bytes".to_owned()
            ))
        );
    }

    #[test]
    fn make_upgrade_heights_key_returns_10_byte_key_consisting_of_prefix_0x01_and_be_chain_id_and_suffix_0x01()
     {
        let chain_id = 258;
        let key = make_upgrade_heights_key(chain_id);
        assert_eq!(
            key,
            [
                1, // prefix
                0, 0, 0, 0, 0, 0, 1, 2, // chain ID
                1, // suffix
            ]
        );
    }

    #[test]
    fn make_corrections_key_returns_10_byte_key_consisting_of_prefix_0x01_and_be_chain_id_and_suffix_0x02()
     {
        let chain_id = 258;
        let key = make_corrections_key(chain_id);
        assert_eq!(
            key,
            [
                1, // prefix
                0, 0, 0, 0, 0, 0, 1, 2, // chain ID
                2, // suffix
            ]
        );
    }

    #[test]
    fn make_block_key_returns_17_byte_key_consisting_of_prefix_0x02_and_be_chain_id_and_be_block_number()
     {
        let chain_id = 258;
        let block_number = 259;
        assert_eq!(
            make_block_key(chain_id, block_number),
            [
                2, // prefix
                0, 0, 0, 0, 0, 0, 1, 2, // chain ID
                0, 0, 0, 0, 0, 0, 1, 3, // block number
            ]
        );
    }

    #[test]
    fn global_metadata_is_stored_before_chain_metadata() {
        let global_metadata_keys = [VERSION_KEY, CHAIN_IDS_KEY];
        let chain_metadata_keys = [
            make_block_ranges_key(0),
            make_upgrade_heights_key(0),
            make_corrections_key(0),
        ];
        for global_metadata_key in global_metadata_keys {
            for chain_metadata_key in chain_metadata_keys {
                assert!(global_metadata_key.as_slice() < chain_metadata_key.as_slice());
            }
        }
    }

    #[test]
    fn chain_metadata_is_stored_before_blocks() {
        let chain_metadata_keys = [
            make_block_ranges_key(u64::MAX),
            make_upgrade_heights_key(u64::MAX),
            make_corrections_key(u64::MAX),
        ];
        let block_data_key = make_block_key(u64::MIN, 0);
        for chain_metadata_key in chain_metadata_keys {
            assert!(chain_metadata_key.as_slice() < block_data_key.as_slice());
        }
    }

    const UNIQUE_IDENTIFIER: usize = 12345;

    /// Creates a new [KvDbBackedBlockDb] backed by a fresh [RocksDb] in a temporary directory.
    fn create_rocks_block_db() -> (TestDir, KvDbBackedBlockDb<RocksDb>) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let rocks = RocksDb::create(tmpdir.path()).unwrap();
        let db = KvDbBackedBlockDb::create(rocks).unwrap();
        (tmpdir, db)
    }

    // Returns a non-default block for testing purposes.
    fn some_block() -> Block {
        Block {
            number: 42,
            ..Default::default()
        }
    }
}
