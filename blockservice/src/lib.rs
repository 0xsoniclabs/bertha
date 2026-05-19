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

use std::ops::RangeInclusive;

pub mod app_dir;
pub mod cli;
pub mod cmd;
pub mod config;
mod db;
mod error;
pub mod grpc;
mod utils;
pub use error::Error;
mod json_rpc;

#[cfg(test)]
pub(crate) mod test_templates;

pub type BlockRange = RangeInclusive<u64>;
