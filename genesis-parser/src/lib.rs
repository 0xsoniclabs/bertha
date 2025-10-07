use std::{
    ffi::OsStr,
    fs,
    io::{self, BufRead, Read, Seek},
    marker::PhantomData,
    path::{Path, PathBuf},
};

use bertha_types::Block;
use e2store::{era::Era, era1::Era1};

pub use crate::error::{Error, GFileError};
use crate::{block_parser::BlockParser, units::parse_metadata};

mod block;
mod block_parser;
mod era;
mod era1;
mod error;
mod slice_reader;
// the module can not be `#[cfg(test)]` because then it can not be used in tests of other crates,
// but by making it `#[doc(hidden)]` it is not included in the public API documentation
#[doc(hidden)]
pub mod test_utils;
mod transaction_receipt;
mod units;

/// An accessor to parsed blocks from a genesis `.g` file.
pub struct GFile<R: BufRead + Seek> {
    chain_id: u64,
    blocks: BlockParser<R>,
}

impl<R: BufRead + Seek> GFile<R> {
    /// Parses the genesis file metadata and returns a [GFile] object.
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

/// An accessor to parsed blocks from a directory containing `.era1` and `.era` files.
pub struct EraDir<R: FileReader> {
    files: Vec<PathBuf>,
    _reader: PhantomData<R>,
}

impl<R: FileReader> EraDir<R> {
    /// Opens the directory at the given path and scans for `.era1` and `.era` files.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut files = Vec::new();

        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let path = entry.path();
                if path.extension().and_then(OsStr::to_str) == Some(R::EXTENSION) {
                    files.push(path);
                }
            }
        }

        Ok(Self {
            files,
            _reader: PhantomData,
        })
    }

    /// Returns and iterator over blocks in descending order (w.r.t. block number). Because the
    /// `.era1` and `.era` files are parsed lazily while consuming the iterator, the yielded items
    /// are of type `Result<Block, Error>` to be able to propagate errors during parsing. Once
    /// an error was returned, the iterator will not yield any more blocks.
    pub fn blocks(mut self) -> impl Iterator<Item = Result<Block, Error>> {
        // `.era` file naming convention: <config-name>-<era-number>-<short-historical-root>.era
        //     - config-name is the CONFIG_NAME field of the runtime configuration (mainnet, prater,
        //       ropsten, sepolia, etc)
        //     - era-number is the number of the first era stored in the file - for example, the
        //       genesis era file has number 0 - as a 5-digit 0-filled decimal integer
        //     - short-historical-root is the first 4 bytes of the last historical root in the last
        //       state in the era file, lower-case hex-encoded (8 characters), except the genesis
        //       era which instead uses the genesis_validators_root field from the genesis state
        // The files in the directory are expected to be for the same configuration, so sorting by
        // the file name in reverse order, sorts the files according to their era number.
        self.files.sort_by(|a, b| b.cmp(a)); // sort in reverse

        self.files.into_iter().flat_map(|path| {
            match R::read_file(path) {
                Ok(blocks) => {
                    let mut blocks: Vec<_> = blocks.collect();
                    // The blocks in `.era1` and `.era` files are in ascending order, so reverse
                    // them to get descending order.
                    blocks.reverse();
                    blocks
                }
                Err(err) => vec![Err(err)],
            }
        })
    }
}

pub trait FileReader {
    const EXTENSION: &'static str;

    fn read_file(
        path: impl AsRef<Path>,
    ) -> Result<impl Iterator<Item = Result<Block, Error>>, Error>;
}

pub struct Era1Reader;

impl FileReader for Era1Reader {
    const EXTENSION: &'static str = "era1";

    /// Reads and parses a single `.era1` file at the given path, returning an iterator over its
    /// blocks.
    fn read_file(
        path: impl AsRef<Path>,
    ) -> Result<impl Iterator<Item = Result<Block, Error>>, Error> {
        let data = fs::read(path.as_ref())?;
        Ok(Era1::iter_tuples(&data)
            .map_err(|err| Error::Era(err.to_string()))?
            .map(era1::convert_block)
            .map(Ok))
    }
}

pub struct EraReader;

impl FileReader for EraReader {
    const EXTENSION: &'static str = "era";

    /// Reads and parses a single `.era` file at the given path, returning an iterator over its
    /// blocks.
    fn read_file(
        path: impl AsRef<Path>,
    ) -> Result<impl Iterator<Item = Result<Block, Error>>, Error> {
        let data = fs::read(path.as_ref())?;

        let blocks = Era::deserialize_blocks(&data)
            .map_err(|err| Error::Era(err.to_string()))?
            .into_iter()
            .map(era::convert_block);
        Ok(blocks)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::test_utils::generate_test_genesis;

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
