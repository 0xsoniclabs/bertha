#![allow(warnings)]
tonic::include_proto!("rpc");

impl From<(u64, u64)> for BlockRange {
    fn from(range: (u64, u64)) -> Self {
        BlockRange {
            from: range.0,
            to: range.1,
        }
    }
}

/// Helper function to convert an iterator of `(u64, u64)` tuples into a `Vec<BlockRange>`.
#[cfg(test)]
pub fn to_block_range_vec(ranges: impl IntoIterator<Item = (u64, u64)>) -> Vec<BlockRange> {
    ranges.into_iter().map(BlockRange::from).collect()
}
