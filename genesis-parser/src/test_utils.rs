use std::io::{Cursor, Read};

use alloy_rlp::Encodable;
use bertha_types::Hash;
use flate2::{Compression, bufread::GzEncoder};

use crate::{
    block::{FullBlock, IdxFullBlock},
    units::{GenesisHeader, HEADER, Unit, VERSION},
};

/// Returns a dummy genesis file.
pub fn generate_test_genesis(network_id: u64, num_blocks: usize) -> Vec<u8> {
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
    for i in 0..num_blocks {
        IdxFullBlock {
            block_number: (num_blocks - i - 1) as u64,
            block: FullBlock::default(),
        }
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
