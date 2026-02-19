// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

mod blockdb;
pub mod proto;
mod rocksblockdb;

pub use blockdb::BlockDb;
#[cfg(test)]
pub use blockdb::MockBlockDb;
pub use rocksblockdb::{BlockBatch, RocksBlockDb};

#[cfg(test)]
mod test_utils {
    use crate::BlockRange;

    pub fn make_meta_value(value: impl IntoIterator<Item = u64>) -> Vec<u8> {
        value.into_iter().flat_map(u64::to_be_bytes).collect()
    }

    pub fn make_range_value(ranges: impl IntoIterator<Item = BlockRange>) -> Vec<u8> {
        make_meta_value(
            ranges
                .into_iter()
                .flat_map(|range| [*range.start(), *range.end()]),
        )
    }
}
