mod blockdb;
pub mod proto;
mod rocksblockdb;

pub use blockdb::BlockDb;
#[cfg(test)]
pub use blockdb::MockBlockDb;
pub use rocksblockdb::RocksBlockDb;

#[cfg(test)]
mod test_utils {
    pub fn make_meta_value(value: impl IntoIterator<Item = u64>) -> Vec<u8> {
        value.into_iter().flat_map(u64::to_be_bytes).collect()
    }

    pub fn make_range_value(ranges: impl IntoIterator<Item = (u64, u64)>) -> Vec<u8> {
        make_meta_value(ranges.into_iter().flat_map(|(start, end)| [start, end]))
    }
}
