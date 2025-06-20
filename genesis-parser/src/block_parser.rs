use std::{
    collections::HashMap,
    io::{self, BufRead, Read, Seek, SeekFrom, Take},
    num::NonZeroUsize,
};

use alloy_rlp::Decodable;
use bertha_types::Hash;
use flate2::bufread::GzDecoder;

use crate::{
    Block, Error, GenesisError, block::IdxFullBlock, read_bytes, slice_reader::SliceReader,
    units::UnitDescriptor,
};

/// An iterator that parses blocks from a compressed genesis file lazily while they are consumed.
/// It yields [Result<Block, Error>] to be able to propagate errors during parsing.
/// Once an error was returned, the iterator will not yield any more blocks.
pub struct BlockParser<R: BufRead> {
    slice_reader: SliceReader<GzDecoder<Take<R>>>,
    error: bool,
}

impl<R: BufRead + Seek> BlockParser<R> {
    // Source: sonic/opera/genesisstore/disk.go (FilesHashMaxMemUsage)
    const MAX_MEM_USAGE: usize = 256 * 1024 * 1024; // 256 MiB

    pub fn try_new(mut reader: R, units: &HashMap<String, UnitDescriptor>) -> Result<Self, Error> {
        // In theory, the genesis file can contain multiple block units (named `brs`, `brs_1`,
        // `brs_2`, ...). However, in practice only `brs` is used. Because it simplifies the code,
        // we only support a single block unit named `brs` here. In case there are multiple
        // block units, we will only use the first one and print a warning.
        // Source: sonic/opera/genesisstore/store_genesis.go ((Blocks)ForEach)
        if units.keys().filter(|key| key.starts_with("brs_")).count() > 0 {
            println!("WARNING: file contains multiple block units, but only the first one is used");
        }
        let blocks_unit = units
            .get("brs")
            .ok_or(Error::Genesis(GenesisError::BlocksUnitMissing))?;

        // Now seek to the start of the blocks unit (offset) and decompress `compressed_size` bytes.
        // Source: sonic/opera/genesisstore/disk.go (OpenGenesisStore -> ReaderProvider)
        reader.seek(SeekFrom::Start(blocks_unit.offset as u64))?;
        let compressed_blocks_reader = reader.take(blocks_unit.compressed_size);

        let mut blocks_reader = GzDecoder::new(compressed_blocks_reader);

        // The payload starts with the piece size, the size and the hashes.
        // Source: sonic/opera/genesisstore/fileshash/reader_file.go (init)
        let piece_size = u32::from_be_bytes(read_bytes(&mut blocks_reader)?) as usize;
        let size = u64::from_be_bytes(read_bytes(&mut blocks_reader)?) as usize;

        let num_hashes = size.div_ceil(piece_size);

        // The Go code holds all hashes and a single `piece` in memory
        // For what ever reason it calculates the total size as
        // `piece_size + hashes_num * 128` and not as `piece_size + hashes_num * 32`.
        // For consistency we do the same check here, although we don't use the hashes at all.
        // This also means that we have the full MAX_MEM_USAGE of 256 MiB available for the buffer
        // in SliceReader.
        if piece_size + num_hashes * 128 > Self::MAX_MEM_USAGE {
            return Err(Error::Genesis(GenesisError::PieceSizeTooLarge {
                got: piece_size,
                max: Self::MAX_MEM_USAGE,
            }));
        }

        // skip hashes
        io::copy(
            &mut blocks_reader
                .by_ref()
                .take((num_hashes * size_of::<Hash>()) as u64),
            &mut io::sink(),
        )?;

        // everything else that is left in the reader are the encoded blocks
        Ok(Self {
            slice_reader: SliceReader::new(
                blocks_reader,
                Self::MAX_MEM_USAGE,
                NonZeroUsize::new(Self::MAX_MEM_USAGE / 2).unwrap(), // This is > 0
            ),
            error: false,
        })
    }
}

impl<R: BufRead> Iterator for BlockParser<R> {
    type Item = Result<Block, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.error {
            return None;
        }
        // In Go, a hand written buffered reader of size `piece_size` is used.
        // This works because the rlp lib in Go can read from streams.
        // alloy_rlp can not read from streams but needs a slice of bytes.
        // Because encoded blocks can span across piece boundaries, we used the `SliceReader` which
        // always holds *enough* bytes in its buffer to decode a full block (unlike a BufReader
        // which only refills the buffer when it is empty).
        // Source: sonic/opera/genesisstore/fileshash/reader_file.go (readFromPiece, readNewPiece)
        match self.slice_reader.process_with(IdxFullBlock::decode) {
            Ok(Some(block)) => Some(
                Block::try_from(block).map_err(|msg| Error::Rlp(alloy_rlp::Error::Custom(msg))),
            ),
            Ok(None) => None,
            Err(err) => {
                self.error = true;
                Some(Err(err))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, iter, vec};

    use alloy_rlp::Encodable;
    use flate2::{Compression, bufread::GzEncoder};

    use super::*;

    #[test]
    fn parses_blocks_from_reader() {
        const OFFSET: usize = 1234;

        const PIECE_SIZE: u32 = 1000;
        const SIZE: u64 = 10000;

        let mut buffer = Vec::new();

        // add some dummy data before the blocks unit
        buffer.extend(iter::repeat_n(0, OFFSET));

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
            IdxFullBlock::default().encode(&mut unit_data);
        }
        let uncompressed_size = unit_data.len();
        let compressed_size = GzEncoder::new(Cursor::new(&mut unit_data), Compression::fast())
            .read_to_end(&mut buffer)
            .unwrap();

        // add some dummy data after the blocks unit
        buffer.extend(iter::repeat_n(0, 123));

        let mut blocks_iter = BlockParser::try_new(
            Cursor::new(buffer),
            &HashMap::from([(
                "brs".to_string(),
                UnitDescriptor {
                    offset: OFFSET,
                    compressed_size: compressed_size as u64,
                    uncompressed_size: uncompressed_size as u64,
                },
            )]),
        )
        .unwrap();

        for _ in 0..3 {
            assert_eq!(blocks_iter.next().unwrap().unwrap(), Block::default_sonic());
        }
        assert!(blocks_iter.next().is_none());
    }

    #[test]
    fn checks_max_mem_usage() {
        const PIECE_SIZE: u32 = BlockParser::<Cursor<Vec<u8>>>::MAX_MEM_USAGE as u32 + 1;
        const SIZE: u64 = PIECE_SIZE as u64 * 2;

        let mut buffer = Vec::new();

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
            IdxFullBlock::default().encode(&mut unit_data);
        }
        let uncompressed_size = unit_data.len();
        let compressed_size = GzEncoder::new(Cursor::new(&mut unit_data), Compression::fast())
            .read_to_end(&mut buffer)
            .unwrap();

        let blocks_iter = BlockParser::try_new(
            Cursor::new(buffer),
            &HashMap::from([(
                "brs".to_string(),
                UnitDescriptor {
                    offset: 0,
                    compressed_size: compressed_size as u64,
                    uncompressed_size: uncompressed_size as u64,
                },
            )]),
        );

        assert!(matches!(
            blocks_iter,
            Err(Error::Genesis(GenesisError::PieceSizeTooLarge {
                got: _,
                max: BlockParser::<Cursor<Vec<u8>>>::MAX_MEM_USAGE,
            }))
        ));
    }

    #[test]
    fn stops_iteration_after_error() {
        const PIECE_SIZE: u32 = 1000;
        const SIZE: u64 = 10000;

        let mut buffer = Vec::new();

        // this buffer will hold the piece size, size, hashes and blocks
        // it gets compressed and then added to `buffer``
        let mut unit_data = Vec::new();

        // add piece_size, size and hashes
        unit_data.extend_from_slice(&PIECE_SIZE.to_be_bytes());
        unit_data.extend_from_slice(&SIZE.to_be_bytes());
        for _ in 0..(SIZE as usize / PIECE_SIZE as usize) {
            unit_data.extend_from_slice(&[0u8; 32]); // dummy hashes
        }

        // add a valid block
        let before_len = unit_data.len();
        IdxFullBlock::default().encode(&mut unit_data);
        // add an invalid block
        let len = unit_data.len();
        unit_data.extend_from_slice(&vec![0; len - before_len]);
        // add a valid block
        IdxFullBlock::default().encode(&mut unit_data);

        let uncompressed_size = unit_data.len();
        let compressed_size = GzEncoder::new(Cursor::new(&mut unit_data), Compression::fast())
            .read_to_end(&mut buffer)
            .unwrap();

        let mut blocks_iter = BlockParser::try_new(
            Cursor::new(buffer),
            &HashMap::from([(
                "brs".to_string(),
                UnitDescriptor {
                    offset: 0,
                    compressed_size: compressed_size as u64,
                    uncompressed_size: uncompressed_size as u64,
                },
            )]),
        )
        .unwrap();

        // first block is valid
        assert_eq!(blocks_iter.next().unwrap().unwrap(), Block::default_sonic());
        // second block is invalid
        assert!(matches!(blocks_iter.next(), Some(Err(Error::Rlp(_)))));
        // third block is not processed because of the error
        assert!(blocks_iter.next().is_none());
    }
}
