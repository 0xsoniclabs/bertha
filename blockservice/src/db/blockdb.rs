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
#[cfg_attr(test, mockall::automock(type Batch = MockBlockDbBatch;))]
pub trait BlockDb {
    type Batch: BlockDbBatch;

    /// Retrieves the IDs of all chains stored in the database.
    fn get_chain_ids(&self) -> Result<Vec<u64>, Error>;

    /// Retrieves the stored ranges of blocks for the specified chain-ID.
    /// The start and end of each range are inclusive.
    fn get_ranges_of_chain_id(&self, chain_id: u64) -> Result<Vec<BlockRange>, Error>;

    /// Retrieves a block for the specified chain-ID and block number.
    /// Returns [None] if the block does not exist.
    fn get(&self, chain_id: u64, block_number: u64) -> Result<Option<Block>, Error>;

    /// Retrieves the raw protobuf-encoded bytes of a block for the specified chain-ID and block
    /// number. Returns [None] if the block does not exist.
    fn get_bytes(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error>;

    /// Stores a block for the specified chain-ID and updates the metadata.
    /// The block number is obtained from the block itself.
    fn put(&self, chain_id: u64, block: Block) -> Result<(), Error>;

    /// Stores a block for the specified chain-ID and block number and updates the metadata.
    /// The data is expected to be a protobuf-encoded block.
    fn put_bytes(&self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error>;

    /// Deletes blocks in the specified block number range (inclusive) for the specified chain-ID
    /// and updates the metadata.
    fn delete_range(
        &self,
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
    fn write_batch(&self, batch: Self::Batch) -> Result<(), Error>;
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
    /// Creates a new [BlockDb] backed by the given [KvDb] implementation.
    pub fn new(db: D) -> Self {
        Self { db }
    }

    /// Provides access to the underlying [KvDb] for test setup/verification.
    #[cfg(test)]
    pub(crate) fn kv_db(&self) -> &D {
        // TODO remove this method
        &self.db
    }

    /// Executes a function that operates on a batch, then writes the batch atomically.
    fn execute_in_batch<R>(&self, f: impl FnOnce(&mut D::Batch) -> R) -> Result<R, Error> {
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

    fn put(&self, chain_id: u64, block: Block) -> Result<(), Error> {
        let number = block.number;
        let data = proto::Block::from(block).encode_to_vec();
        self.put_bytes(chain_id, number, &data)
    }

    fn put_bytes(&self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
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
        &self,
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
                    if key.len() == 8 {
                        // metadata, no more data for this chain id
                        return None;
                    }
                    if key.len() != 16 {
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

    fn write_batch(&self, mut batch: Self::Batch) -> Result<(), Error> {
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
}

/// Key for storing the IDs of all chains in the database.
pub const CHAIN_IDS_KEY: [u8; 8] = 0u64.to_be_bytes();

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
pub fn make_block_ranges_key(chain_id: u64) -> [u8; 8] {
    chain_id.to_be_bytes()
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

/// Returns the key for storing a block for the given chain ID and block number.
pub fn make_block_key(chain_id: u64, block_number: u64) -> [u8; 16] {
    let mut key = [0u8; 16];
    key[0..8].copy_from_slice(&chain_id.to_be_bytes());
    key[8..16].copy_from_slice(&block_number.to_be_bytes());
    key
}

#[cfg(test)]
mod tests {
    use mockall::predicate::eq;

    use super::*;

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
        let db = KvDbBackedBlockDb::new(kv_db);
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
        let db = KvDbBackedBlockDb::new(kv_db);
        assert_eq!(db.get_ranges_of_chain_id(chain_id), expected);
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
        let db = KvDbBackedBlockDb::new(kv_db);
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
        let db = KvDbBackedBlockDb::new(kv_db);
        let result = db.get(chain_id, block_number);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));
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
        let db = KvDbBackedBlockDb::new(kv_db);
        let received = db.get_bytes(chain_id, block_number).unwrap().unwrap();
        assert_eq!(received, raw_data);
    }

    #[test]
    fn kv_db_backed_block_db_get_bytes_returns_none_for_missing_block() {
        let mut kv_db = MockKvDb::new();
        kv_db.expect_get_raw().return_once(|_| Ok(None)).times(1);
        let db = KvDbBackedBlockDb::new(kv_db);
        assert_eq!(db.get_bytes(1, 42).unwrap(), None);
    }

    #[test]
    fn kv_db_backed_block_db_put_stores_encodes_block_as_protobuf_using_number_from_block_as_key() {
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

        let db = KvDbBackedBlockDb::new(kv_db);
        db.put(chain_id, block).unwrap();
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

        let db = KvDbBackedBlockDb::new(kv_db);
        db.put_bytes(chain_id, block_number, b"data").unwrap();
    }

    #[rstest::rstest]
    #[case::no_existing_ranges_for_chain_id(
        None,
        2,
        3,
        vec![],
    )]
    #[case::existing_ranges_for_chain_id_delete_middle(
        Some(vec![0..=5]),
        2,
        3,
        vec![0..=1, 4..=5],
    )]
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

        let db = KvDbBackedBlockDb::new(kv_db);
        db.delete_range(chain_id, start_block_number, end_block_number)
            .unwrap();
    }

    #[test]
    fn kv_db_backed_block_db_delete_range_returns_error_if_start_is_greater_than_end() {
        let db = KvDbBackedBlockDb::new(MockKvDb::new());
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
        let db = KvDbBackedBlockDb::new(kv_db);

        let result = db.iterate(chain_id, 1, IterationDirection::Forward).next();

        match expected_block {
            Some(expected) => assert_eq!(result, Some(Ok(expected))),
            None => assert!(matches!(result, Some(Err(Error::Protobuf(_))))),
        }
    }

    #[rstest::rstest]
    #[case::until_no_more_blocks(
        vec![
            Ok((
                make_block_key(1, 1).to_vec().into_boxed_slice(),
                Box::from(b"block1".as_slice()),
            )),
            Ok((
                make_block_key(1, 3).to_vec().into_boxed_slice(),
                Box::from(b"block3".as_slice()),
            )),
        ],
        vec![
            Ok((1, Box::from(b"block1".as_slice()))),
            Ok((3, Box::from(b"block3".as_slice()))),
        ]
    )]
    #[case::until_chain_boundary(
        vec![
            Ok((
                make_block_key(1, 1).to_vec().into_boxed_slice(),
                Box::from(b"chain1-block1".as_slice()),
            )),
            Ok((
                make_block_key(2, 1).to_vec().into_boxed_slice(),
                Box::from(b"chain2-block1".as_slice()),
            )),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_metadata_key(
        vec![
            Ok((
                make_block_key(1, 1).to_vec().into_boxed_slice(),
                Box::from(b"chain1-block1".as_slice()),
            )),
            Ok((
                make_block_ranges_key(1).to_vec().into_boxed_slice(),
                Box::from(b"metadata".as_slice()),
            )),
            Ok((
                make_block_key(1, 2).to_vec().into_boxed_slice(),
                Box::from(b"chain1-block2".as_slice()),
            )),
        ],
        vec![Ok((1, Box::from(b"chain1-block1".as_slice())))]
    )]
    #[case::until_invalid_key(
        vec![
            Ok((
                make_block_key(1, 1).to_vec().into_boxed_slice(),
                Box::from(b"block1-1".as_slice()),
            )),
            Ok((
                vec![0u8; 17].into_boxed_slice(),
                Box::from(b"invalid".as_slice()),
            )),
            Ok((
                make_block_key(1, 2).to_vec().into_boxed_slice(),
                Box::from(b"block1-2".as_slice()),
            )),
        ],
        vec![
            Ok((1, Box::from(b"block1-1".as_slice()))),
            Err(Error::StorageLayer("unexpected key length 17".to_owned())),
        ]
    )]
    #[case::until_db_error(
        vec![
            Ok((
                make_block_key(1, 4).to_vec().into_boxed_slice(),
                Box::from(b"block1-4".as_slice()),
            )),
            Err(Error::StorageLayer("boom".to_owned())),
            Ok((
                make_block_key(1, 5).to_vec().into_boxed_slice(),
                Box::from(b"block1-5".as_slice()),
            )),
        ],
        vec![
            Ok((4, Box::from(b"block1-4".as_slice()))),
            Err(Error::StorageLayer("boom".to_owned())),
        ]
    )]
    fn kv_db_backed_block_db_iterate_bytes_iterates_over_all_valid_blocks_for_chain_id(
        #[case] raw_items: Vec<Result<RawEntry, Error>>,
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
            .return_once(move |_start, _dir| Box::new(raw_items.into_iter()))
            .times(1);
        let db = KvDbBackedBlockDb::new(kv_db);

        let blocks: Vec<_> = db
            .iterate_bytes(chain_id, start_block_number, IterationDirection::Forward)
            .collect();
        assert_eq!(blocks, expected);
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
        let db = KvDbBackedBlockDb::new(kv_db);
        assert!(
            db.iterate_bytes(chain_id, start_block_number, direction)
                .next()
                .is_none()
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

        let db = KvDbBackedBlockDb::new(kv_db);
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
    fn kv_db_backed_block_db_write_batch_empty_batch_is_noop() {
        let kv_db = MockKvDb::new();
        let kv_db_batch = MockKvDbBatch::new();
        let db = KvDbBackedBlockDb::new(kv_db);
        let batch = KvDbBatchWrapper {
            kv_batch: kv_db_batch,
            block_ranges: HashMap::default(),
        };
        db.write_batch(batch).unwrap();
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
    fn make_block_ranges_key_returns_8_byte_be_chain_id() {
        let chain_id = 258;
        let key = make_block_ranges_key(chain_id);
        assert_eq!(key, [0, 0, 0, 0, 0, 0, 1, 2]);
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
    fn make_block_key_returns_8_byte_be_chain_id_concat_8_byte_be_block_number() {
        let chain_id = 258;
        let block_number = 259;
        assert_eq!(
            make_block_key(chain_id, block_number),
            [
                0, 0, 0, 0, 0, 0, 1, 2, // chain ID
                0, 0, 0, 0, 0, 0, 1, 3, // block number
            ]
        );
    }

    const UNIQUE_IDENTIFIER: usize = 12345;

    // Returns a non-default block for testing purposes.
    fn some_block() -> Block {
        Block {
            number: 42,
            ..Default::default()
        }
    }
}
