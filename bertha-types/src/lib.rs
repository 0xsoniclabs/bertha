// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

mod as_hex;
mod block;
mod block_header;
mod eip_2718_utils;
mod error;
mod hex_convert;
mod known_hashes;
mod log;
mod parse_hex_error;
mod receipts;
mod rlp_utils;
#[cfg(test)]
mod test_data;
mod transaction;
mod u256;
mod withdrawal;

pub use as_hex::AsHex;
pub use block::{Block, OmmerHeader};
pub use block_header::BlockHeader;
pub use eip_2718_utils::{EIP2718Unmarshallable, Eip2718Marshallable, compute_root_hash, verify};
pub use error::VerificationError;
pub use hex_convert::HexConvert;
pub use known_hashes::*;
pub use log::Log;
pub use receipts::{
    PostStateOrStatus, RECEIPT_STATUS_FAILED_RLP, RECEIPT_STATUS_SUCCESS_RLP, TransactionReceipt,
};
pub use transaction::{AccessListEntry, SetCodeAuthorization, Transaction, TransactionType};
pub use u256::U256;
pub use withdrawal::Withdrawal;

pub type Bloom = [u8; 256];
pub type Hash = [u8; 32];
pub type Address = [u8; 20];

pub use rlp_utils::*;
