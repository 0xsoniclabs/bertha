use std::ops::RangeInclusive;

mod app_dir;
mod cli;
pub mod cmd;
mod db;
mod error;
pub mod grpc;
mod utils;
pub use error::Error;
#[cfg(test)]
mod json_rpc;

pub type BlockRange = RangeInclusive<u64>;
