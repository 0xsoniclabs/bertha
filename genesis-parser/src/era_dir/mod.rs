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

use std::{
    ffi::OsStr,
    fs::{self, File},
    marker::PhantomData,
    path::{Path, PathBuf},
};

use bertha_types::Block;
use lighthouse_types::{ForkName, ForkVersionDecode, MainnetEthSpec, SignedBeaconBlock};
use reth_era::{common::file_ops::StreamReader, era::file::EraReader, era1::file::Era1Reader};

use crate::Error;

mod era;
mod era1;

/// An accessor to parsed blocks from a directory containing `.era1` and `.era` files.
pub struct EraDir<R: FileReader> {
    files: Vec<PathBuf>,
    chain_id: u64,
    _reader: PhantomData<R>,
}

impl<R: FileReader> EraDir<R> {
    /// Opens the directory at the given path and scans for `.era1` and `.era` files.
    pub fn open(path: impl AsRef<Path>, chain_id: u64) -> Result<Self, Error> {
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
            chain_id,
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

        self.files.into_iter().flat_map(move |path| {
            match R::read_file(path, self.chain_id) {
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

/// A trait for reading and parsing blocks from a specific file format.
pub trait FileReader {
    const EXTENSION: &'static str;

    fn read_file(
        path: impl AsRef<Path>,
        chain_id: u64,
    ) -> Result<impl Iterator<Item = Result<Block, Error>>, Error>;
}

pub struct Era1FileReader;

impl FileReader for Era1FileReader {
    const EXTENSION: &'static str = "era1";

    /// Reads and parses a single `.era1` file at the given path, returning an iterator over its
    /// blocks.
    fn read_file(
        path: impl AsRef<Path>,
        _chain_id: u64,
    ) -> Result<impl Iterator<Item = Result<Block, Error>>, Error> {
        let file = File::open(path.as_ref())?;
        let reader = Era1Reader::new(file);
        Ok(reader.iter().map(|result| {
            result
                .map_err(Error::E2S)
                .and_then(|block| era1::convert_block(&block))
        }))
    }
}

pub struct EraFileReader;

impl FileReader for EraFileReader {
    const EXTENSION: &'static str = "era";

    /// Reads and parses a single `.era` file at the given path, returning an iterator over its
    /// blocks.
    fn read_file(
        path: impl AsRef<Path>,
        chain_id: u64,
    ) -> Result<impl Iterator<Item = Result<Block, Error>>, Error> {
        let file = File::open(path.as_ref())?;
        let reader = EraReader::new(file);
        Ok(reader.iter().map(move |result| {
            let ssz_bytes = result?.decompress()?;
            // Determine the fork from the slot in the beacon block.
            // The slot is at a fixed offset in the SSZ: after the 4-byte offset for `message`
            // and 96-byte signature in SignedBeaconBlock, then at the start of BeaconBlock.
            let slot = decode_slot_from_signed_block(&ssz_bytes)?;
            let fork = try_get_beacon_fork(slot, chain_id)
                .ok_or_else(|| Error::Era(format!("unsupported fork for slot {slot}")))?;
            let beacon_block =
                SignedBeaconBlock::<MainnetEthSpec>::from_ssz_bytes_by_fork(&ssz_bytes, fork)
                    .map_err(|e| Error::Era(format!("SSZ decode error: {e:?}")))?;
            era::convert_block(beacon_block)
        }))
    }
}

/// Decodes the slot number from the raw SSZ bytes of a `SignedBeaconBlock`.
///
/// In the SSZ encoding of `SignedBeaconBlock`:
/// - Fixed section: [4-byte offset to `message`] [96-byte signature] = 100 bytes
/// - Variable section starts at byte 100 with `BeaconBlock`
/// - `BeaconBlock` fixed section starts with `slot` (u64 LE, 8 bytes)
fn decode_slot_from_signed_block(ssz: &[u8]) -> Result<u64, Error> {
    if ssz.len() < 108 {
        return Err(Error::Era(
            "SSZ too short for SignedBeaconBlock".to_string(),
        ));
    }
    let slot_bytes: [u8; 8] = ssz[100..108]
        .try_into()
        .map_err(|_| Error::Era("failed to read slot".to_string()))?;
    Ok(u64::from_le_bytes(slot_bytes))
}

/// Returns the fork name for a given slot and chain ID, based on the known fork activation slots
/// for mainnet, sepolia, hoodi, and holeski.
fn try_get_beacon_fork(slot_index: u64, chain_id: u64) -> Option<ForkName> {
    const MAINNET: u64 = 1;
    const SEPOLIA: u64 = 11155111;
    const HOLESKI: u64 = 17000;
    const HOODI: u64 = 560048;

    // go-ethereum/beacon/params/networks.go
    const MAINNET_BELLATRIX: u64 = 144896 * 32;
    const MAINNET_CAPELLA: u64 = 194048 * 32;
    const MAINNET_DENEB: u64 = 269568 * 32;
    const MAINNET_ELECTRA: u64 = 364032 * 32;
    const MAINNET_FULU: u64 = 411392 * 32;

    const SEPOLIA_BELLATRIX: u64 = 100 * 32;
    const SEPOLIA_CAPELLA: u64 = 56832 * 32;
    const SEPOLIA_DENEB: u64 = 132608 * 32;
    const SEPOLIA_ELECTRA: u64 = 222464 * 32;
    const SEPOLIA_FULU: u64 = 272640 * 32;

    const HOLESKI_CAPELLA: u64 = 256 * 32;
    const HOLESKI_DENEB: u64 = 29696 * 32;
    const HOLESKI_ELECTRA: u64 = 115968 * 32;
    const HOLESKI_FULU: u64 = 165120 * 32;

    const HOODI_ELECTRA: u64 = 2048 * 32;
    const HOODI_FULU: u64 = 50688 * 32;

    match chain_id {
        MAINNET => match slot_index {
            0..MAINNET_BELLATRIX => None, // pre-merge
            MAINNET_BELLATRIX..MAINNET_CAPELLA => Some(ForkName::Bellatrix),
            MAINNET_CAPELLA..MAINNET_DENEB => Some(ForkName::Capella),
            MAINNET_DENEB..MAINNET_ELECTRA => Some(ForkName::Deneb),
            MAINNET_ELECTRA..MAINNET_FULU => Some(ForkName::Electra),
            MAINNET_FULU.. => Some(ForkName::Fulu),
        },
        SEPOLIA => match slot_index {
            0..SEPOLIA_BELLATRIX => None, // pre-merge
            SEPOLIA_BELLATRIX..SEPOLIA_CAPELLA => Some(ForkName::Bellatrix),
            SEPOLIA_CAPELLA..SEPOLIA_DENEB => Some(ForkName::Capella),
            SEPOLIA_DENEB..SEPOLIA_ELECTRA => Some(ForkName::Deneb),
            SEPOLIA_ELECTRA..SEPOLIA_FULU => Some(ForkName::Electra),
            SEPOLIA_FULU.. => Some(ForkName::Fulu),
        },
        HOLESKI => match slot_index {
            0..HOLESKI_CAPELLA => Some(ForkName::Bellatrix),
            HOLESKI_CAPELLA..HOLESKI_DENEB => Some(ForkName::Capella),
            HOLESKI_DENEB..HOLESKI_ELECTRA => Some(ForkName::Deneb),
            HOLESKI_ELECTRA..HOLESKI_FULU => Some(ForkName::Electra),
            HOLESKI_FULU.. => Some(ForkName::Fulu),
        },
        HOODI => match slot_index {
            0..HOODI_ELECTRA => Some(ForkName::Deneb),
            HOODI_ELECTRA..HOODI_FULU => Some(ForkName::Electra),
            HOODI_FULU.. => Some(ForkName::Fulu),
        },
        _ => None,
    }
}
