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

mod block;
mod block_parser;
mod slice_reader;
pub mod test_utils;
mod transaction_receipt;
mod units;

use std::io::{self, BufRead, Read, Seek};

use bertha_types::Block;
use block_parser::BlockParser;

use crate::Error;

/// An accessor to parsed blocks from a genesis `.g` file.
pub struct GFile<R: BufRead + Seek> {
    chain_id: u64,
    blocks: BlockParser<R>,
}

impl<R: BufRead + Seek> GFile<R> {
    /// Parses the genesis file metadata and returns a [GFile] object.
    pub fn parse(mut reader: R) -> Result<Self, Error> {
        let meta = units::parse_metadata(&mut reader)?;
        let blocks = BlockParser::try_new(reader, &meta.units)?;
        Ok(Self {
            chain_id: meta.chain_id,
            blocks,
        })
    }

    /// Returns the chain ID of the genesis file.
    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// Returns and iterator over blocks in descending order (w.r.t. block number). Because the
    /// `.g` file is parsed lazily while consuming the iterator, the yielded items are of type
    /// `Result<Block, Error>` to be able to propagate errors during parsing. Once an error
    /// was returned, the iterator will not yield any more blocks.
    pub fn blocks(&mut self) -> impl Iterator<Item = Result<Block, Error>> + '_ {
        &mut self.blocks
    }
}

/// Reads exactly `N` bytes from the reader and returns them as an array.
fn read_bytes<const N: usize>(mut reader: impl Read) -> Result<[u8; N], io::Error> {
    let mut data = [0u8; N];
    reader.read_exact(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::g_file::test_utils::generate_test_genesis;

    #[test]
    fn parses_whole_genesis_file_and_yields_all_blocks() {
        let chain_id = 146;
        let blocks = 3;
        let buf = generate_test_genesis(chain_id, blocks, &[]);
        let mut genesis = GFile::parse(Cursor::new(buf)).unwrap();
        assert_eq!(genesis.chain_id(), chain_id);
        assert_eq!(genesis.blocks().count(), blocks);
    }
}
