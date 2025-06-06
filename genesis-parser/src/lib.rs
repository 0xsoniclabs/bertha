use std::io::{self, BufRead, Read, Seek};

use bertha_types::Block;

pub use crate::error::{Error, GenesisError};
use crate::{block_parser::BlockParser, units::parse_metadata};

mod block;
mod block_parser;
mod error;
mod slice_reader;
mod transaction_receipt;
mod units;

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
    use std::io::{Cursor, Read};

    use alloy_rlp::Encodable;
    use bertha_types::{Block, Hash};
    use flate2::{Compression, bufread::GzEncoder};

    use crate::{
        Genesis,
        block::IdxFullBlock,
        units::{GenesisHeader, HEADER, Unit, VERSION},
    };

    #[test]
    fn parses_whole_genesis_file_and_yields_all_blocks() {
        const PIECE_SIZE: u32 = 1000;
        const SIZE: u64 = 10000;

        // this buffer will hold the piece size, size, hashes and blocks
        // it gets compressed and then added to `buffer``
        let mut unit_data = Vec::new();

        // add piece_size, size and hashes
        unit_data.extend_from_slice(&PIECE_SIZE.to_be_bytes());
        unit_data.extend_from_slice(&SIZE.to_be_bytes());
        for _ in 0..(SIZE as usize / PIECE_SIZE as usize) {
            unit_data.extend_from_slice(&[0u8; 32]); // dummy hashes
        }

        // add multiple encoded blocks
        for _ in 0..3 {
            IdxFullBlock::from(Block {
                extra_data: vec![0; 12],
                ..Block::default()
            })
            .encode(&mut unit_data);
        }
        let uncompressed_size = unit_data.len();
        let mut compressed_unit_data = Vec::new();
        let compressed_size = GzEncoder::new(Cursor::new(&mut unit_data), Compression::fast())
            .read_to_end(&mut compressed_unit_data)
            .unwrap();

        let header = GenesisHeader {
            genesis_id: [0u8; 32],
            network_id: 1,
            network_name: "test_network".to_string(),
        };

        let mut buf = Vec::new();
        // write the header and version
        buf.extend_from_slice(&HEADER);
        buf.extend_from_slice(&VERSION);
        // write unit
        let unit = Unit {
            unit_name: "brs".to_owned(),
            header: header.clone(),
        };
        unit.encode(&mut buf);
        // write hash
        buf.extend_from_slice(&Hash::default());
        // write compressed size
        buf.extend_from_slice(&compressed_size.to_be_bytes());
        // write uncompressed size
        buf.extend_from_slice(&uncompressed_size.to_be_bytes());
        // add compressed blocks unit data
        buf.extend(compressed_unit_data);

        let mut genesis = Genesis::parse(Cursor::new(buf)).unwrap();
        assert_eq!(genesis.chain_id(), header.network_id);
        assert_eq!(genesis.blocks().count(), 3);
    }
}
