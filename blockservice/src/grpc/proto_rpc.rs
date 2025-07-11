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
