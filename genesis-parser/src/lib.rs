use std::io::{self, BufRead, Read, Seek};

use bertha_types::Block;

pub use crate::error::{Error, GenesisError};
use crate::{block_parser::BlockParser, units::parse_metadata};

mod block;
mod block_parser;
mod error;
mod slice_reader;
// the module can not be `#[cfg(test)]` because then it can not be used in tests of other crates,
// but by making it `#[doc(hidden)]` it is not included in the public API documentation
#[doc(hidden)]
pub mod test_utils;
mod transaction_receipt;
mod units;

/// An accessor to parsed blocks from a genesis file.
pub struct Genesis<R: BufRead + Seek> {
    chain_id: u64,
    blocks: BlockParser<R>,
}

impl<R: BufRead + Seek> Genesis<R> {
    /// Parses the genesis file metadata and returns a [Genesis] object.
    pub fn parse(mut reader: R) -> Result<Self, Error> {
        let meta = parse_metadata(&mut reader)?;
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

    /// Returns and iterator over blocks. Because the genesis file is parsed lazily while consuming
    /// the iterator, the yielded items are of type `Result<Block, Error>` to be able to
    /// propagate errors during parsing. Once an error was returned, the iterator will not yield
    /// any more blocks.
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

    use crate::{Genesis, test_utils::generate_test_genesis};

    #[test]
    fn parses_whole_genesis_file_and_yields_all_blocks() {
        let chain_id = 146;
        let blocks = 3;
        let buf = generate_test_genesis(chain_id, blocks, &[]);
        let mut genesis = Genesis::parse(Cursor::new(buf)).unwrap();
        assert_eq!(genesis.chain_id(), chain_id);
        assert_eq!(genesis.blocks().count(), blocks);
    }
}
