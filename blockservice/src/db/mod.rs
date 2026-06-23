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

mod blockdb;
pub mod proto;
mod rocksdb;

pub use blockdb::{BlockDb, BlockDbBatch, IterationDirection, KvDbBackedBlockDb};
// By not exposing the KvDb outside of tests, access to the underlying key-value database is
// only possible through the BlockDb interface.
#[cfg(test)]
pub use blockdb::{
    //TODO make private
    CHAIN_IDS_KEY,
    KvDb,
    MockBlockDb,
    make_block_ranges_key,
    serialize_block_ranges,
    serialize_chain_ids,
};
pub use rocksdb::RocksDb;

pub type RocksBlockDb = KvDbBackedBlockDb<RocksDb>;
