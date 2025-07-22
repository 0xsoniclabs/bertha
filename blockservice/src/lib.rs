use std::ops::RangeInclusive;

mod app_dir;
pub mod cli;
pub mod cmd;
mod db;
mod error;
pub mod grpc;
mod utils;
pub use error::Error;
mod json_rpc;

pub type BlockRange = RangeInclusive<u64>;
