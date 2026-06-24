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

use std::io::{Cursor, Read};

use alloy_rlp::Encodable;
use bertha_types::{Block, Hash};
use flate2::{Compression, bufread::GzEncoder};

use crate::g_file::{
    block::IdxFullBlock,
    units::{GenesisHeader, HEADER, Unit, VERSION},
};

/// Returns a dummy genesis file with the specified number of blocks.
/// Additionally it adds the specified extra blocks at the end in case they are provided.
/// This can be used to generate invalid data.
pub fn generate_test_genesis(
    network_id: u64,
    num_blocks: usize,
    extra_blocks: &[Block],
) -> Vec<u8> {
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

    let mut prev_hash = Hash::default();
    let mut all_blocks = Vec::new();
    for block_number in 0..num_blocks as u64 {
        let block = Block {
            number: block_number,
            parent_hash: prev_hash,
            ..Block::default_sonic()
        };
        prev_hash = block.to_header().compute_hash();
        all_blocks.push(block);
    }
    all_blocks.extend_from_slice(extra_blocks);
    for block in all_blocks.into_iter().rev() {
        IdxFullBlock::try_from(block)
            .unwrap()
            .encode(&mut unit_data);
    }

    let uncompressed_size = unit_data.len();
    let mut compressed_unit_data = Vec::new();
    let compressed_size = GzEncoder::new(Cursor::new(&mut unit_data), Compression::fast())
        .read_to_end(&mut compressed_unit_data)
        .unwrap();

    let header = GenesisHeader {
        genesis_id: [0u8; 32],
        network_id,
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

    buf
}
