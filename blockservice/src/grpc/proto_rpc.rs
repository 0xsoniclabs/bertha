#![allow(warnings)]
tonic::include_proto!("rpc");

use std::ops::RangeInclusive;

impl From<crate::BlockRange> for BlockRange {
    fn from(range: crate::BlockRange) -> Self {
        BlockRange {
            from: *range.start(),
            to: *range.end(),
        }
    }
}

impl From<BlockRange> for crate::BlockRange {
    fn from(range: BlockRange) -> Self {
        range.from..=range.to
    }
}
