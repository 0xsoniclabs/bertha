use bertha_types::Block;
use prost::Message;

use crate::{BlockRange, db::proto, error::Error, utils::ranges::RangesExt};

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
    fn put_chain_ids(&self, chain_ids: &[u64]) -> Result<(), Error> {
        self.put_metadata_raw(Self::CHAIN_IDS_KEY, chain_ids)
    }

    /// Retrieves the stored ranges of blocks for the specified chain-ID.
    /// The start and end of each range are inclusive.
    fn get_ranges_of_chain_id(&self, chain_id: u64) -> Result<Vec<BlockRange>, Error> {
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
            .map(|chunk| chunk[0]..=chunk[1])
            .collect())
    }

    /// Stores the ranges of blocks for the specified chain-ID.
    /// The start and end of each range are inclusive.
    fn put_ranges_of_chain_id(&self, chain_id: u64, ranges: &[BlockRange]) -> Result<(), Error> {
        let data: Vec<u64> = ranges
            .iter()
            .flat_map(|range| [*range.start(), *range.end()])
            .collect();
        self.put_metadata_raw(chain_id, &data)
    }

    /// Deletes the ranges of blocks for the specified chain-ID.
    fn delete_ranges_of_chain_id(&self, chain_id: u64) -> Result<(), Error> {
        self.delete_metadata(chain_id)
    }

    /// Adds a chain ID to the list of chain IDs stored in the database.
    fn add_chain_id_to_chain_ids(&self, chain_id: u64) -> Result<(), Error> {
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
    fn remove_chain_id_from_chain_ids(&self, chain_id: u64) -> Result<(), Error> {
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
    fn add_block_number_to_ranges(&self, chain_id: u64, block_number: u64) -> Result<(), Error> {
        self.add_chain_id_to_chain_ids(chain_id)?;
        let mut ranges = self.get_ranges_of_chain_id(chain_id)?;
        ranges.add_range(block_number..=block_number);
        self.put_ranges_of_chain_id(chain_id, &ranges)
    }

    /// Adds a block range to the ranges of block numbers stored in the database for the specified
    /// chain-ID. If this is the first block range for the chain ID, the chain ID is added to the
    /// list of chain IDs.
    fn add_range_to_ranges(&self, chain_id: u64, new: BlockRange) -> Result<(), Error> {
        self.add_chain_id_to_chain_ids(chain_id)?;
        let mut ranges = self.get_ranges_of_chain_id(chain_id)?;
        ranges.add_range(new);
        self.put_ranges_of_chain_id(chain_id, &ranges)
    }

    /// Removes a range of block numbers from the ranges of blocks stored in the database for the
    /// specified chain-ID. If this is the last remaining block for the chain ID, the chain ID is
    /// removed from the list of chain IDs.
    fn remove_range_from_ranges(&self, chain_id: u64, del: &BlockRange) -> Result<(), Error> {
        let mut ranges = self.get_ranges_of_chain_id(chain_id)?;
        ranges.subtract_range(del);
        if ranges.is_empty() {
            self.delete_ranges_of_chain_id(chain_id)?;
            self.remove_chain_id_from_chain_ids(chain_id)
        } else {
            self.put_ranges_of_chain_id(chain_id, &ranges)
        }
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
    fn put(&self, chain_id: u64, block: Block) -> Result<(), Error> {
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

    /// Deletes all blocks for the specified chain-ID in the range from `from_block` (inclusive) to
    /// `to_block` (inclusive).
    fn delete_range(&self, chain_id: u64, from_block: u64, to_block: u64) -> Result<(), Error>;

    /// Retrieves the raw metadata for the specified key.
    fn get_metadata_raw(&self, key: u64) -> Result<Option<Vec<u64>>, Error>;

    /// Stores the raw metadata for the specified key.
    fn put_metadata_raw(&self, key: u64, data: &[u64]) -> Result<(), Error>;

    /// Deletes the metadata for the specified key.
    fn delete_metadata(&self, key: u64) -> Result<(), Error>;

    /// Retrieves the raw protobuf-encoded data for the specified chain-ID and block number.
    /// Returns [None] if the block does not exist.
    fn get_raw(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error>;

    /// Stores the raw protobuf-encoded data for the specified chain-ID and block number.
    fn put_raw(&self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error>;

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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::BTreeMap};

    use super::*;
    use crate::db::test_utils::{make_meta_value, make_range_value};

    #[test]
    fn blockdb_get_chain_ids_queries_key_zero() {
        let db = StubDb::new();
        db.0.borrow_mut()
            .insert(0u64.to_be_bytes().to_vec(), make_meta_value([1u64, 2u64]));
        assert_eq!(db.get_chain_ids(), Ok(vec![1, 2]),);
    }

    #[test]
    fn blockdb_get_chain_ids_returns_empty_vec_if_no_chain_ids_stored() {
        let db = StubDb::new();
        assert_eq!(db.get_chain_ids(), Ok(Vec::new()));
    }

    #[test]
    fn blockdb_get_chain_ids_returns_error_if_data_length_is_invalid() {
        let db = StubDb::new();
        db.0.borrow_mut().insert(
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
        let db = StubDb::new();
        let ids = [0, 1];
        db.put_chain_ids(&ids).unwrap();
        assert_eq!(
            db.0.borrow().get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value(ids))
        );
    }

    #[test]
    fn blockdb_get_ranges_of_chain_id_converts_tuples() {
        let ranges = [0..=1, 2..=3, 4..=5];
        let db = StubDb::new();
        db.0.borrow_mut().insert(
            1u64.to_be_bytes().to_vec(),
            make_range_value(ranges.clone()),
        );
        assert_eq!(db.get_ranges_of_chain_id(1).unwrap(), ranges);
    }

    #[test]
    fn blockdb_get_ranges_of_chain_id_returns_error_if_data_length_is_invalid() {
        let db = StubDb::new();
        db.0.borrow_mut().insert(
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
        let ranges = [0..=1, 2..=3, 4..=5];
        let db = StubDb::new();
        db.put_ranges_of_chain_id(chain_id, &ranges).unwrap();
        assert_eq!(
            db.0.borrow().get(chain_id.to_be_bytes().as_slice()),
            Some(&make_range_value(ranges))
        );
    }

    #[test]
    fn blockdb_delete_ranges_of_chain_id_removes_all_ranges_for_chain_id() {
        let chain_id: u64 = 1;
        let ranges = [0..=1, 2..=3, 4..=5];
        let db = StubDb::new();
        db.0.borrow_mut()
            .insert(chain_id.to_be_bytes().to_vec(), make_range_value(ranges));
        db.delete_ranges_of_chain_id(chain_id).unwrap();
        assert_eq!(db.0.borrow().get(chain_id.to_be_bytes().as_slice()), None);
    }

    #[test]
    fn blockdb_add_chain_id_to_chain_ids_adds_chain_id_if_not_exists_and_keep_list_sorted() {
        let db = StubDb::new();
        db.0.borrow_mut()
            .insert(0u64.to_be_bytes().to_vec(), make_meta_value([1u64, 3u64]));

        // add non existing key
        let chain_id: u64 = 2;
        db.add_chain_id_to_chain_ids(chain_id).unwrap();
        assert_eq!(
            db.0.borrow().get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 2, 3]))
        );

        // add existing key
        let chain_id: u64 = 1;
        db.add_chain_id_to_chain_ids(chain_id).unwrap();
        assert_eq!(
            db.0.borrow().get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 2, 3]))
        );
    }

    #[test]
    fn blockdb_remove_chain_id_from_chain_ids_removes_chain_id_if_exists() {
        let db = StubDb::new();
        db.0.borrow_mut().insert(
            0u64.to_be_bytes().to_vec(),
            make_meta_value([1u64, 2u64, 3u64]),
        );

        // remove non-existing key
        db.remove_chain_id_from_chain_ids(4).unwrap();
        assert_eq!(
            db.0.borrow().get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 2, 3]))
        );

        // remove existing key
        db.remove_chain_id_from_chain_ids(2).unwrap();
        assert_eq!(
            db.0.borrow().get(0u64.to_be_bytes().as_slice()),
            Some(&make_meta_value([1, 3]))
        );
    }

    #[test]
    fn blockdb_add_block_number_to_ranges_adds_block_number_if_not_exists() {
        let chain_id: u64 = 1;
        let db = StubDb::new();

        // The corner cases of adding a block number to the ranges are tested in the
        // [crate::utils::ranges], here we only test that the changes are also written.
        let cases = [
            // add non-existing block number
            (vec![0..=1], 3, vec![0..=1, 3..=3]),
            // add existing block number
            (vec![0..=1], 0, vec![0..=1]),
        ];
        for (ranges, new_key, expected_ranges) in cases {
            db.0.borrow_mut()
                .insert(chain_id.to_be_bytes().to_vec(), make_range_value(ranges)); // reset value
            db.add_block_number_to_ranges(chain_id, new_key).unwrap();
            assert_eq!(
                db.0.borrow().get(chain_id.to_be_bytes().as_slice()),
                Some(&make_range_value(expected_ranges))
            );
        }
    }

    #[test]
    fn blockdb_remove_range_from_ranges_removes_range_if_exists() {
        let chain_id: u64 = 1;
        let db = StubDb::new();

        // The corner cases of removing a block number from the ranges are tested in the
        // [crate::utils::ranges], here we only test that the changes are also written.
        let cases = [
            // remove an existing range
            (vec![0..=1], 1..=1, Some(vec![0..=0])),
            // remove the last existing range
            (vec![0..=1], 0..=1, None),
            (vec![], 0..=1, None),
        ];
        for (ranges, del_range, expected_ranges) in cases {
            // set the chain id
            db.0.borrow_mut()
                .insert(0u64.to_be_bytes().to_vec(), chain_id.to_be_bytes().to_vec()); // reset value
            // set the initial ranges
            db.0.borrow_mut()
                .insert(chain_id.to_be_bytes().to_vec(), make_range_value(ranges)); // reset value
            db.remove_range_from_ranges(chain_id, &del_range).unwrap();
            if expected_ranges.is_some() {
                assert!(db.get_chain_ids().unwrap().contains(&chain_id));
            }
            assert_eq!(
                db.0.borrow().get(chain_id.to_be_bytes().as_slice()),
                expected_ranges.map(make_range_value).as_ref()
            );
        }
    }

    #[test]
    fn blockdb_get_converts_from_protobuf() {
        let block = Block::default();

        let chain_id = 1;
        let db = StubDb::new();
        db.0.borrow_mut().insert(
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
        let db = StubDb::new();
        db.0.borrow_mut().insert(
            make_data_key(chain_id, block_number),
            vec![0, 1, 2, 3], // invalid protobuf data
        );
        let result = db.get(chain_id, block_number);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Protobuf(_)));
    }

    #[test]
    fn blockdb_put_converts_to_protobuf() {
        let db = StubDb::new();
        let chain_id: u64 = 1;
        let block = Block::default();
        db.put(chain_id, block.clone()).unwrap();
        assert_eq!(
            proto::Block::decode(
                db.0.borrow()
                    .get(&make_data_key(chain_id, block.number))
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
        let db = StubDb::new();
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
        let db = StubDb::new();
        db.0.borrow_mut().insert(
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

    struct StubDb(RefCell<BTreeMap<Vec<u8>, Vec<u8>>>);

    impl StubDb {
        fn new() -> Self {
            Self(RefCell::new(BTreeMap::new()))
        }
    }

    impl BlockDb for StubDb {
        fn get_metadata_raw(&self, key: u64) -> Result<Option<Vec<u64>>, Error> {
            let value = self.0.borrow().get(key.to_be_bytes().as_slice()).cloned();
            let Some(value) = value else {
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

        fn put_metadata_raw(&self, key: u64, data: &[u64]) -> Result<(), Error> {
            let key = key.to_be_bytes().to_vec();
            let value = data.iter().flat_map(|v| v.to_be_bytes()).collect();
            self.0.borrow_mut().insert(key, value);
            Ok(())
        }

        fn delete_metadata(&self, key: u64) -> Result<(), Error> {
            self.0.borrow_mut().remove(key.to_be_bytes().as_slice());
            Ok(())
        }

        fn get_raw(&self, chain_id: u64, block_number: u64) -> Result<Option<Vec<u8>>, Error> {
            let key = make_data_key(chain_id, block_number);
            Ok(self.0.borrow().get(&key).cloned())
        }

        fn put_raw(&self, chain_id: u64, block_number: u64, data: &[u8]) -> Result<(), Error> {
            let key = make_data_key(chain_id, block_number);
            let value = data.to_vec();
            self.0.borrow_mut().insert(key, value);
            Ok(())
        }

        fn iterate_raw(
            &self,
            chain_id: u64,
            from: u64,
        ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
            let key = make_data_key(chain_id, from);
            self.0
                .borrow()
                .range(key..)
                .map(|(k, v)| {
                    let key = u64::from_be_bytes(
                        k[8..16]
                            .try_into()
                            .map_err(|_| Error::StorageLayer("invalid key length".to_owned()))?,
                    );
                    Ok((key, v.clone().into_boxed_slice()))
                })
                // this is needed to be able to release the lock
                .collect::<Vec<_>>()
                .into_iter()
        }

        fn iterate_reverse_raw(
            &self,
            chain_id: u64,
            from: u64,
        ) -> impl Iterator<Item = Result<(u64, Box<[u8]>), Error>> {
            let key = make_data_key(chain_id, from);
            self.0
                .borrow()
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
                // this is needed to be able to release the lock
                .collect::<Vec<_>>()
                .into_iter()
        }

        fn delete_range(
            &self,
            _chain_id: u64,
            _from_block: u64,
            _to_block: u64,
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
}
