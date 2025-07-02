use std::path::Path;

use bertha_types::Block;
use prost::Message;
use rocksdb::WriteBatchWithTransaction;
use tempfile::TempDir;

use crate::{error::Error, proto};

impl From<rocksdb::Error> for Error {
    fn from(e: rocksdb::Error) -> Self {
        Error::StorageLayer(e.to_string())
    }
}

/// Default blockdb name
pub const BLOCK_DB_NAME: &str = ".blockdb";

/// A database that allows to store and and query [Block]s for multiple different blockchains.
/// Blocks are encoded as protobuf messages before being stored in the database.
/// As database operations may fail for various reasons, all methods return a [Result].
///
/// Currently, the block database uses 64-bit chain-IDs for convenience, as opposed to the 256-bit
/// used by Ethereum. This could be changed in the future if needed.
#[cfg_attr(test, mockall::automock)]
pub trait BlockDb {
    /// Key for storing the IDs of all chains in the database.
    /// The ranges for each chain-ID are stored using the chain-ID as key.
    /// Chain ID 0 is invalid.
    const CHAIN_IDS_KEY: u64 = 0;

    /// Retrieves the IDs of all chains stored in the database.
    fn get_chain_ids(&self) -> Result<Vec<u64>, Error> {
        Ok(self
            .get_metadata_raw(Self::CHAIN_IDS_KEY)?
            .unwrap_or_default())
    }

    /// Stores the IDs of all chains in the database.
    fn put_chain_ids(&mut self, chain_ids: &[u64]) -> Result<(), Error> {
        self.put_metadata_raw(Self::CHAIN_IDS_KEY, chain_ids)
    }

    /// Retrieves the stored ranges of blocks for the specified chain-ID.
    /// The start and end of each range are inclusive.
    fn get_ranges_of_chain_id(&self, chain_id: u64) -> Result<Vec<(u64, u64)>, Error> {
        let data = self.get_metadata_raw(chain_id)?.unwrap_or_default();
        if data.len() % 2 != 0 {
            return Err(Error::StorageLayer(format!(
                "invalid ranges for chain ID {}: data length {} not a multiple of 2",
                chain_id,
                data.len()
            )));
        }
        Ok(data
            .chunks_exact(2)
            // slices are guaranteed to be of length 2 so the index access will not fail
            .map(|chunk| (chunk[0], chunk[1]))
            .collect())
    }

    /// Stores the ranges of blocks for the specified chain-ID.
    /// The start and end of each range are inclusive.
    fn put_ranges_of_chain_id(
        &mut self,
        chain_id: u64,
        ranges: &[(u64, u64)],
    ) -> Result<(), Error> {
        let data: Vec<u64> = ranges
            .iter()
            .flat_map(|&(start, end)| [start, end])
            .collect();
        self.put_metadata_raw(chain_id, &data)
    }

    /// Adds a chain ID to the list of chain IDs stored in the database.
    fn add_chain_id_to_chain_ids(&mut self, chain_id: u64) -> Result<(), Error> {
        // assumption:
        // - ids are sorted
        // - ids are not duplicated
        let mut chain_ids = self.get_chain_ids()?;
        match chain_ids.binary_search(&chain_id) {
            // chain_id already exists, no need to add it
            Ok(_) => Ok(()),
            Err(idx) => {
                // chain_id does not exist, insert it at the correct position
                chain_ids.insert(idx, chain_id);
                self.put_chain_ids(&chain_ids)
            }
        }
    }

    /// Removes a chain ID from the list of chain IDs stored in the database.
    fn remove_chain_id_from_chain_ids(&mut self, chain_id: u64) -> Result<(), Error> {
        // assumption:
        // - ids are sorted
        // - ids are not duplicated
        let mut chain_ids = self.get_chain_ids()?;
        if let Ok(idx) = chain_ids.binary_search(&chain_id) {
            // chain_id exists, remove it
            chain_ids.remove(idx);
            self.put_chain_ids(&chain_ids)
        } else {
            // chain_id does not exist, no need to remove it
            Ok(())
        }
    }

    /// Adds a block number to the ranges of block numbers stored in the database for the specified
    /// chain-ID. If this is the first block for the chain ID, the chain ID is added to the list
    /// of chain IDs.
    fn add_block_number_to_ranges(
        &mut self,
        chain_id: u64,
        block_number: u64,
    ) -> Result<(), Error> {
        // assumption:
        // - ranges are valid (start <= end)
        // - ranges are non-overlapping
        // - ranges are sorted

        self.add_chain_id_to_chain_ids(chain_id)?;

        let mut ranges = self.get_ranges_of_chain_id(chain_id)?;

        // iterate over index to allow insertion
        for i in 0..ranges.len() {
            let (start, end) = ranges[i];
            // the block number is before the current range and not adjacent to it
            if block_number + 1 < start {
                ranges.insert(i, (block_number, block_number));
                return self.put_ranges_of_chain_id(chain_id, &ranges);
            }
            // the block number is adjacent to the start of the current range
            else if block_number + 1 == start {
                ranges[i].0 = block_number; // extend the start of the range to include the block number
                // no need to check for merge with previous range, because this would have been
                // handled by extending the end of the previous range
                return self.put_ranges_of_chain_id(chain_id, &ranges);
            }
            // the block number is within the current range
            else if start <= block_number && block_number <= end {
                return Ok(());
            }
            // the block number is adjacent to the end of the current range
            else if block_number == end + 1 {
                ranges[i].1 = block_number; // extend the end of the range to include the block number
                // check if we can merge with next range
                if i + 1 < ranges.len() && ranges[i + 1].0 == block_number + 1 {
                    ranges[i].1 = ranges[i + 1].1; // extend the end of the current range to include the next range
                    ranges.remove(i + 1); // remove the next range
                }
                return self.put_ranges_of_chain_id(chain_id, &ranges);
            }
        }
        // block number is greater than all existing ranges
        ranges.push((block_number, block_number)); // add new range for block number
        self.put_ranges_of_chain_id(chain_id, &ranges)
    }

    /// Removes a range of block numbers from the ranges of blocks stored in the database for the
    /// specified chain-ID. If this is the last remaining block for the chain ID, the chain ID is
    /// removed from the list of chain IDs.
    fn remove_range_from_ranges(
        &mut self,
        chain_id: u64,
        del_start: u64,
        del_end: u64,
    ) -> Result<(), Error> {
        // assumption:
        // - ranges are valid (start <= end)
        // - ranges are non-overlapping
        // - ranges are sorted

        self.remove_chain_id_from_chain_ids(chain_id)?;

        let mut ranges = self.get_ranges_of_chain_id(chain_id)?;
        if ranges.is_empty() {
            return Ok(());
        }

        let mut i = 0;
        while i < ranges.len() {
            let (start, end) = ranges[i];
            // The deletion range is before all following ranges
            if del_end < start {
                break;
            }
            // No overlap
            else if end < del_start || del_end < start {
                i += 1;
                continue;
            }
            // Full overlap: remove the range
            else if del_start <= start && end <= del_end {
                ranges.remove(i);
                continue;
            }
            // Overlap at end of existing range: trim right
            else if start < del_start && end <= del_end {
                ranges[i].1 = del_start - 1;
                i += 1;
                continue;
            }
            // Overlap at start of existing range: trim left
            else if del_start <= start && del_end < end {
                ranges[i].0 = del_end + 1;
                break;
            }
            // Middle overlap: split into two ranges
            else if start < del_start && del_end < end {
                let right = (del_end + 1, end);
                ranges[i].1 = del_start - 1;
                ranges.insert(i + 1, right);
                break;
            }
        }
        self.put_ranges_of_chain_id(chain_id, &ranges)
    }

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

    /// Iterates over all block numbers (extracted from the keys) and blocks for the specified
    /// chain-ID starting from the given block number. The sequence is ordered by
    /// block number and may contain gaps for missing blocks.
    fn iterate_with_block_number(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<(u64, Block), Error>> {
        self.iterate_raw(chain_id, from).map(|result| {
            result.and_then(|(block_number, data)| {
                let block = proto::Block::decode(data.as_ref()).map_err(Error::Protobuf)?;
                Ok((block_number, Block::try_from(block)?))
            })
        })
    }

    /// Like [BlockDb::iterate], but iterates in reverse order.
    fn iterate_reverse(
        &self,
        chain_id: u64,
        from: u64,
    ) -> impl Iterator<Item = Result<Block, Error>> {
        self.iterate_reverse_raw(chain_id, from).map(|result| {
            result.and_then(|(_, data)| {
                let block = proto::Block::decode(data.as_ref()).map_err(Error::Protobuf)?;
                Block::try_from(block)
            })
        })
    }

    /// Deletes all blocks for the specified chain-ID in the range from `from_block` (defaults to 0;
    /// inclusive) to `to_block` (defaults to u64::MAX; inclusive).
    fn delete_range(
        &mut self,
        chain_id: u64,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> Result<(), Error>;

    /// Retrieves the raw metadata for the specified key.
    fn get_metadata_raw(&self, key: u64) -> Result<Option<Vec<u64>>, Error>;

    /// Stores the raw metadata for the specified key.
    fn put_metadata_raw(&mut self, key: u64, data: &[u64]) -> Result<(), Error>;

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
    ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> + Send;

    /// Like `iterate_raw`, but iterates in reverse order.
    fn iterate_reverse_raw(
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

    fn put_metadata_raw(&mut self, key: u64, data: &[u64]) -> Result<(), Error> {
        self.db
            .put(
                key.to_be_bytes(),
                data.iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<u8>>(),
            )
            .map_err(|e| Error::StorageLayer(e.to_string()))
    }

    fn get_raw(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error> {
        self.db
            .get(Self::make_key(chain_id, block_number))
            .map_err(|e| Error::StorageLayer(e.to_string()))
    }

    fn put_raw(&mut self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
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
        &mut self,
        chain_id: u64,
        from_block: Option<u64>,
        to_block: Option<u64>,
    ) -> Result<(), Error> {
        let from_block = from_block.unwrap_or(0);
        let to_block = to_block.unwrap_or(u64::MAX);
        let from = Self::make_key(chain_id, from_block);
        let to = Self::make_key(chain_id, to_block);

        let mut batch = WriteBatchWithTransaction::<false>::default();
        batch.delete_range(from, to);
        self.db.write(batch)?;

        self.remove_range_from_ranges(chain_id, from_block, to_block)
    }
}

#[cfg(test)]
pub mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::proto;

    #[test]
    fn blockdb_get_chain_ids_queries_key_zero() {
        let mut db = StubDb::new();
        db.0.insert(0u64.to_be_bytes().to_vec(), make_meta_value([1u64, 2u64]));
        assert_eq!(db.get_chain_ids(), Ok(vec![1, 2]),);
    }

    #[test]
    fn blockdb_get_chain_ids_returns_empty_vec_if_no_chain_ids_stored() {
        let db = StubDb::new();
        assert_eq!(db.get_chain_ids(), Ok(Vec::new()));
    }

    #[test]
    fn blockdb_get_chain_ids_returns_error_if_data_length_is_invalid() {
        let mut db = StubDb::new();
        db.0.insert(
            0u64.to_be_bytes().to_vec(),
            vec![0], // not a multiple of 8 bytes
        );
        assert_eq!(
            db.get_chain_ids(),
            Err(Error::StorageLayer(
                "invalid metadata length: data length 1 not a multiple of 8 bytes".to_owned()
            ))
        );
    }

    #[test]
    fn blockdb_put_chain_ids_writes_chain_ids_to_key_zero() {
        let mut db = StubDb::new();
        let ids = [0, 1];
        db.put_chain_ids(&ids).unwrap();
        assert_eq!(
            db.0.get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value(ids))
        );
    }

    #[test]
    fn blockdb_get_ranges_of_chain_id_converts_tuples() {
        let ranges = [(0, 1), (2, 3), (4, 5)];
        let mut db = StubDb::new();
        db.0.insert(1u64.to_be_bytes().to_vec(), make_range_value(ranges));
        assert_eq!(db.get_ranges_of_chain_id(1).unwrap(), ranges);
    }

    #[test]
    fn blockdb_get_ranges_of_chain_id_returns_error_if_data_length_is_invalid() {
        let mut db = StubDb::new();
        db.0.insert(
            1u64.to_be_bytes().to_vec(),
            // not a multiple of 2 8 bytes chunks
            [0].into_iter().flat_map(u64::to_be_bytes).collect(),
        );
        assert_eq!(
            db.get_ranges_of_chain_id(1),
            Err(Error::StorageLayer(
                "invalid ranges for chain ID 1: data length 1 not a multiple of 2".to_owned()
            ))
        );
    }

    #[test]
    fn blockdb_put_ranges_of_chain_id_converts_tuples() {
        let chain_id: u64 = 1;
        let ranges = [(0, 1), (2, 3), (4, 5)];
        let mut db = StubDb::new();
        db.put_ranges_of_chain_id(chain_id, &ranges).unwrap();
        assert_eq!(
            db.0.get(chain_id.to_be_bytes().as_slice()),
            Some(&make_range_value(ranges))
        );
    }

    #[test]
    fn blockdb_add_chain_id_to_chain_ids_adds_chain_id_if_not_exists_and_keep_list_sorted() {
        let mut db = StubDb::new();
        db.0.insert(0u64.to_be_bytes().to_vec(), make_meta_value([1u64, 3u64]));

        // add non existing key
        let chain_id: u64 = 2;
        db.add_chain_id_to_chain_ids(chain_id).unwrap();
        assert_eq!(
            db.0.get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 2, 3]))
        );

        // add existing key
        let chain_id: u64 = 1;
        db.add_chain_id_to_chain_ids(chain_id).unwrap();
        assert_eq!(
            db.0.get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 2, 3]))
        );
    }

    #[test]
    fn blockdb_remove_chain_id_from_chain_ids_removes_chain_id_if_exists() {
        let mut db = StubDb::new();
        db.0.insert(
            0u64.to_be_bytes().to_vec(),
            make_meta_value([1u64, 2u64, 3u64]),
        );

        // remove non-existing key
        db.remove_chain_id_from_chain_ids(4).unwrap();
        assert_eq!(
            db.0.get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 2, 3]))
        );

        // remove existing key
        db.remove_chain_id_from_chain_ids(2).unwrap();
        assert_eq!(
            db.0.get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 3]))
        );
    }

    #[test]
    fn blockdb_add_block_number_to_ranges_adds_block_number_if_not_exists_and_keeps_ranges_disjunct_and_sorted()
     {
        let chain_id: u64 = 1;
        let init_ranges = [(3, 4), (9, 10), (12, 13)];
        let mut db = StubDb::new();

        let cases = [
            // add non-existing block number before existing ranges
            (0, vec![(0, 0), (3, 4), (9, 10), (12, 13)]),
            // add non-existing block number between existing ranges
            (6, vec![(3, 4), (6, 6), (9, 10), (12, 13)]),
            // add non-existing block number after existing ranges
            (15, vec![(3, 4), (9, 10), (12, 13), (15, 15)]),
            // add non-existing block number adjacent to start of existing range
            (2, vec![(2, 4), (9, 10), (12, 13)]),
            // add non-existing block number adjacent to end of existing range
            (5, vec![(3, 5), (9, 10), (12, 13)]),
            // add non-existing block number adjacent to end of one range and start of another
            (11, vec![(3, 4), (9, 13)]),
            // add existing block number
            (3, vec![(3, 4), (9, 10), (12, 13)]),
        ];
        for (new_key, expected_ranges) in cases {
            db.0.insert(
                chain_id.to_be_bytes().to_vec(),
                make_range_value(init_ranges),
            ); // reset value
            db.add_block_number_to_ranges(chain_id, new_key).unwrap();
            assert_eq!(
                db.0.get(chain_id.to_be_bytes().as_slice()),
                Some(&make_range_value(expected_ranges))
            );
        }
    }

    #[test]
    fn blockdb_remove_range_from_ranges_removes_range_if_exists() {
        let chain_id: u64 = 1;
        let init_ranges = [(3, 4), (9, 10), (12, 13)];
        let mut db = StubDb::new();

        let cases = [
            // remove start of existing range
            ((3, 3), vec![(4, 4), (9, 10), (12, 13)]),
            // remove end of existing range
            ((4, 4), vec![(3, 3), (9, 10), (12, 13)]),
            // remove full existing range
            ((3, 4), vec![(9, 10), (12, 13)]),
            // remove range that spans parts of multiple existing ranges
            ((4, 9), vec![(3, 3), (10, 10), (12, 13)]),
            // remove range that spans multiple existing ranges
            ((3, 13), vec![]),
            // remove non-existing range before first existing ranges
            ((0, 1), vec![(3, 4), (9, 10), (12, 13)]),
            // remove non-existing range after first existing range
            ((6, 6), vec![(3, 4), (9, 10), (12, 13)]),
        ];
        for (del_range, expected_ranges) in cases {
            db.0.insert(
                chain_id.to_be_bytes().to_vec(),
                make_range_value(init_ranges),
            ); // reset value
            db.remove_range_from_ranges(chain_id, del_range.0, del_range.1)
                .unwrap();
            assert_eq!(
                db.0.get(chain_id.to_be_bytes().as_slice()),
                Some(&make_range_value(expected_ranges))
            );
        }
    }

    #[test]
    fn blockdb_get_converts_from_protobuf() {
        let block = Block::default();

        let chain_id = 1;
        let mut db = StubDb::new();
        db.0.insert(
            make_data_key(chain_id, block.number),
            proto::Block::from(block.clone()).encode_to_vec(),
        );
        let received = db.get(chain_id, block.number).unwrap().unwrap();
        assert_eq!(received, block);
    }

    #[test]
    fn blockdb_get_returns_error_for_invalid_protobuf() {
        let chain_id = 1;
        let block_number = 0;
        let mut db = StubDb::new();
        db.0.insert(
            make_data_key(chain_id, block_number),
            vec![0, 1, 2, 3], // invalid protobuf data
        );
        let result = db.get(chain_id, block_number);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));
    }

    #[test]
    fn blockdb_put_converts_to_protobuf() {
        let mut db = StubDb::new();
        let chain_id: u64 = 1;
        let block = Block::default();
        db.put(chain_id, block.clone()).unwrap();
        assert_eq!(
            proto::Block::decode(
                db.0.get(&make_data_key(chain_id, block.number))
                    .unwrap()
                    .as_slice()
            )
            .unwrap(),
            block.into()
        );
    }

    #[test]
    fn blockdb_iterate_converts_from_protobuf() {
        let chain_id = 1;
        let mut db = StubDb::new();
        let block = Block {
            number: 123,
            ..Block::default()
        };
        db.put(chain_id, block.clone()).unwrap();

        // Forward
        let mut iter = db.iterate(chain_id, 0);
        let received = iter.next().unwrap().unwrap();
        assert_eq!(received, block);

        // Reverse
        let mut iter = db.iterate_reverse(chain_id, 0);
        let received = iter.next().unwrap().unwrap();
        assert_eq!(received, block);

        // With Block Number
        let mut iter = db.iterate_with_block_number(chain_id, 0);
        let received = iter.next().unwrap().unwrap().1;
        assert_eq!(received, block);
    }

    #[test]
    fn blockdb_iterate_returns_error_for_invalid_protobuf() {
        let chain_id = 1;
        let block_number = 0;
        let mut db = StubDb::new();
        db.0.insert(
            make_data_key(chain_id, block_number),
            vec![0, 1, 2, 3], // invalid protobuf data
        );

        // Forward
        let mut iter = db.iterate(chain_id, block_number);
        let result = iter.next().unwrap();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));

        // Reverse
        let mut iter = db.iterate_reverse(chain_id, block_number);
        let result = iter.next().unwrap();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));

        // With Block Number
        let mut iter = db.iterate_with_block_number(chain_id, block_number);
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
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();
        let key = 1u64;
        let data = [1u64; 2];

        db.put_metadata_raw(key, &data).unwrap();

        let result = db.db.get(key.to_be_bytes()).unwrap();

        assert_eq!(result, Some(make_meta_value(data)));
    }

    #[test]
    fn blockrocksdb_put_raw_adds_range_and_chain_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();

        let chain_id = 146;
        let block_number = 123;
        db.put_raw(chain_id, block_number, b"block").unwrap();

        assert_eq!(
            db.db.get(0u64.to_be_bytes().as_slice()),
            Ok(Some(make_meta_value([chain_id])))
        );
        assert_eq!(
            db.db.get(chain_id.to_be_bytes().as_slice()),
            Ok(Some(make_range_value([(block_number, block_number)])))
        );
    }

    #[test]
    fn blockrocksdb_delete_range_removes_range_and_chain_id() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();

        let chain_id = 146;
        let block_number = 123;
        db.put_chain_ids(&[chain_id]).unwrap();
        db.put_ranges_of_chain_id(chain_id, &[(block_number, block_number)])
            .unwrap();
        db.delete_range(chain_id, Some(block_number), Some(block_number))
            .unwrap();

        assert_eq!(db.db.get(0u64.to_be_bytes().as_slice()), Ok(Some(vec![])));
        assert_eq!(
            db.db.get(chain_id.to_be_bytes().as_slice()),
            Ok(Some(vec![]))
        );
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

        // query non existing key
        let result = db.get_raw(chain_id, 100).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn rocksblockdb_iterate_raw_returns_blocks_for_single_chain_in_order() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();

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
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();

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
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();
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
        assert_eq!(db.get_raw(chain_id, 4).unwrap(), Some(b"block 4".to_vec())); // range end is exclusive
        assert_eq!(db.get_raw(chain_id, 5).unwrap(), Some(b"block 5".to_vec()));
    }

    #[test]
    fn rocksblockdb_delete_range_succeeds_if_no_blocks_in_range() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();

        // delete range when no blocks exist
        assert!(db.delete_range(1, Some(1), Some(5)).is_ok());
    }

    #[test]
    fn rocksblockdb_delete_range_returns_error_if_start_greater_than_end() {
        let tmpdir = tempfile::tempdir().unwrap();
        let mut db = RocksBlockDb::create(tmpdir.path()).unwrap();
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

    struct StubDb(BTreeMap<Vec<u8>, Vec<u8>>);

    impl StubDb {
        fn new() -> Self {
            Self(BTreeMap::new())
        }
    }

    impl BlockDb for StubDb {
        fn get_metadata_raw(&self, key: u64) -> Result<Option<Vec<u64>>, Error> {
            let Some(value) = self.0.get(key.to_be_bytes().as_slice()) else {
                return Ok(None);
            };

            if value.len() % 8 != 0 {
                return Err(Error::StorageLayer(format!(
                    "invalid metadata length: data length {} not a multiple of 8 bytes",
                    value.len()
                )));
            }
            Ok(Some(
                value
                    .chunks_exact(8)
                    .map(|chunk| u64::from_be_bytes(chunk.try_into().unwrap()))
                    .collect(),
            ))
        }

        fn put_metadata_raw(&mut self, key: u64, data: &[u64]) -> Result<(), Error> {
            let key = key.to_be_bytes().to_vec();
            let value = data.iter().flat_map(|v| v.to_be_bytes()).collect();
            self.0.insert(key, value);
            Ok(())
        }

        fn get_raw(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error> {
            let key = make_data_key(chain_id, block_number);
            Ok(self.0.get(&key).cloned())
        }

        fn put_raw(&mut self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
            let key = make_data_key(chain_id, block_number);
            let value = data.to_vec();
            self.0.insert(key, value);
            Ok(())
        }

        fn iterate_raw(
            &self,
            chain_id: u64,
            from: u64,
        ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
            let key = make_data_key(chain_id, from);
            self.0.range(key..).map(|(k, v)| {
                let key = u64::from_be_bytes(
                    k[8..16]
                        .try_into()
                        .map_err(|_| Error::StorageLayer("invalid key length".to_owned()))?,
                );
                Ok((key, v.clone().into_boxed_slice()))
            })
        }

        fn iterate_reverse_raw(
            &self,
            chain_id: u64,
            from: u64,
        ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
            let key = make_data_key(chain_id, from);
            self.0
                .range(key..)
                .map(|(k, v)| {
                    let key = u64::from_be_bytes(
                        k[8..16]
                            .try_into()
                            .map_err(|_| Error::StorageLayer("invalid key length".to_owned()))?,
                    );
                    Ok((key, v.clone().into_boxed_slice()))
                })
                .rev()
        }

        fn delete_range(
            &mut self,
            _chain_id: u64,
            _from_block: Option<u64>,
            _to_block: Option<u64>,
        ) -> Result<(), Error> {
            unimplemented!() // there is no delete_range_raw method so we test it directly on RocksBlockDb
        }
    }

    fn make_data_key(chain_id: u64, block_number: u64) -> Vec<u8> {
        [chain_id, block_number]
            .into_iter()
            .flat_map(u64::to_be_bytes)
            .collect()
    }

    fn make_meta_value(value: impl IntoIterator<Item = u64>) -> Vec<u8> {
        value.into_iter().flat_map(u64::to_be_bytes).collect()
    }

    fn make_range_value(ranges: impl IntoIterator<Item = (u64, u64)>) -> Vec<u8> {
        make_meta_value(ranges.into_iter().flat_map(|(start, end)| [start, end]))
    }
}
