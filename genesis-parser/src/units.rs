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
    cmp,
    collections::HashMap,
    io::{BufRead, ErrorKind, Read, Seek, SeekFrom},
};

use alloy_rlp::{Decodable, RlpDecodable, RlpEncodable};
use bertha_types::Hash;

use crate::{
    error::{Error, GFileError},
    read_bytes,
};

/// Metadata for part of the genesis file that contains data of a specific type (e.g. blocks).
// Source: sonic/opera/genesisstore/disk.go (Unit)
#[derive(Debug, Clone, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub struct Unit {
    pub unit_name: String,
    pub header: GenesisHeader,
}

/// A header that contains metadata about the chain this genesis file belongs to.
// Source: sonic/opera/genesis/types.go (Header)
#[derive(Debug, Clone, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub struct GenesisHeader {
    pub genesis_id: Hash,
    pub network_id: u64,
    pub network_name: String,
}

/// A description of where the data of a unit can be found within the genesis file, and how large it
/// is (compressed and uncompressed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitDescriptor {
    pub offset: usize,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

/// A container which contains all metadata needed to parse blocks from the genesis file.
pub struct GenesisMetadata {
    pub chain_id: u64,
    pub units: HashMap<String, UnitDescriptor>,
}

pub const HEADER: [u8; 4] = [0x64, 0x1b, 0x00, 0xac];
pub const VERSION: [u8; 4] = [0x00, 0x02, 0x00, 0x01];

/// Checks that the next bytes in the reader match the expected genesis file header and version.
// Source: sonic/opera/genesisstore/disk.go (checkFileHeader)
fn read_file_header(mut reader: impl Read) -> Result<(), Error> {
    match read_bytes(&mut reader) {
        Ok(HEADER) => (),
        Ok(header) => {
            return Err(Error::GFile(GFileError::InvalidHeader {
                got: header,
                expected: HEADER,
            }));
        }
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => {
            return Err(Error::GFile(GFileError::HeaderMissing));
        }
        Err(err) => {
            return Err(Error::Io(err));
        }
    };

    match read_bytes(&mut reader) {
        Ok(VERSION) => (),
        Ok(version) => {
            return Err(Error::GFile(GFileError::InvalidFileVersion {
                got: version,
                expected: VERSION,
            }));
        }
        Err(err) if err.kind() == ErrorKind::UnexpectedEof => {
            return Err(Error::GFile(GFileError::HeaderMissing));
        }
        Err(err) => {
            return Err(Error::Io(err));
        }
    };

    Ok(())
}

// Parses a [Unit] from the reader by first materializing enough bytes into a buffer
// and then decoding the [Unit] from that buffer. This is necessary because alloy_rlp
// does not support decoding directly from a reader, but requires a slice of bytes.
// Afterwards the underlying reader is seeked to the position right after the [Unit].
fn parse_unit(mut reader: impl Read + Seek, reader_len: usize) -> Result<Unit, Error> {
    // must be large enough to hold the entire encoded [Unit]
    let mut unit_buffer = [0u8; 1024];
    let unit_buf_len = cmp::min(
        unit_buffer.len(),
        reader_len - reader.stream_position()? as usize,
    );
    // because alloy_rlp can not decode from readers but requires slices, we have to materialize
    // enough bytes of the reader into a buffer and pass that buffer to the decoder
    let unit_buffer = &mut unit_buffer[..unit_buf_len];
    reader.read_exact(unit_buffer)?;
    let mut unit_buffer = &*unit_buffer; // convert a mutable slice to an immutable one
    let unit = Unit::decode(&mut unit_buffer)?;
    // now seek backward the number of bytes that have not been consumed when decoding the [Unit]
    reader.seek_relative(-(unit_buffer.len() as i64))?;
    Ok(unit)
}

/// Parses the genesis file and returns the metadata (chain id and unit descriptors).
// Source: sonic/opera/genesisstore/disk.go (OpenGenesisStore)
pub fn parse_metadata(mut reader: impl BufRead + Seek) -> Result<GenesisMetadata, Error> {
    let mut header = None;
    let mut units = HashMap::new();

    // Note: Seek::stream_len() is not available in stable Rust yet
    let len = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;

    loop {
        // Note: ReadBuf::has_data_left() is not available in stable Rust yet
        if reader.stream_position()? >= len {
            break;
        }

        read_file_header(&mut reader)?;

        let unit = parse_unit(&mut reader, len as usize)?;

        match &header {
            Some(h) => {
                if *h != unit.header {
                    return Err(Error::GFile(GFileError::HeaderMismatch));
                }
            }
            None => header = Some(unit.header.clone()),
        }

        // skip hash
        reader.seek_relative(size_of::<Hash>() as i64)?;

        let compressed_size = u64::from_be_bytes(read_bytes(&mut reader)?);
        let uncompressed_size = u64::from_be_bytes(read_bytes(&mut reader)?);

        units.insert(
            unit.unit_name,
            UnitDescriptor {
                offset: reader.stream_position()? as usize,
                compressed_size,
                uncompressed_size,
            },
        );

        reader.seek_relative(compressed_size as i64)?;
    }

    let header = header.ok_or(Error::GFile(GFileError::HeaderMissing))?;

    Ok(GenesisMetadata {
        chain_id: header.network_id,
        units,
    })
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, iter};

    use alloy_rlp::Encodable;
    use bertha_types::Hash;

    use crate::{
        Error, GFileError,
        units::{GenesisHeader, HEADER, Unit, VERSION, parse_metadata, read_file_header},
    };

    #[test]
    fn check_file_header_succeeds_with_valid_header() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&HEADER);
        buf.extend_from_slice(&VERSION);
        assert!(read_file_header(buf.as_slice()).is_ok());
    }

    #[rstest::rstest]
    #[case::header_missing(Vec::new(), GFileError::HeaderMissing)]
    #[case::invalid_header(vec![0; 4], GFileError::InvalidHeader { got: [0; 4], expected: HEADER })]
    #[case::version_missing(HEADER.to_vec(), GFileError::HeaderMissing)]
    #[case::invalid_version(
        {let mut b = HEADER.to_vec(); b.extend_from_slice(&[0; 4]); b},
        GFileError::InvalidFileVersion { got: [0; 4], expected: VERSION }
    )]
    fn check_file_header_fails_with_invalid_header(
        #[case] buf: Vec<u8>,
        #[case] expected_error: GFileError,
    ) {
        assert!(matches!(
            read_file_header(buf.as_slice()).unwrap_err(),
            Error::GFile(err) if err == expected_error,
        ));
    }

    #[test]
    fn parse_metadata_parses_multiple_units_successfully() {
        const COMPRESSED_SIZE: u64 = 1000;
        const UNCOMPRESSED_SIZE: u64 = 1000;
        let header = GenesisHeader {
            genesis_id: [0u8; 32],
            network_id: 1,
            network_name: "test_network".to_string(),
        };

        let mut buf = Vec::new();
        for i in 0..3 {
            // write the header and version
            buf.extend_from_slice(&HEADER);
            buf.extend_from_slice(&VERSION);
            // write unit
            let unit = Unit {
                unit_name: format!("test_unit{i}"),
                header: header.clone(),
            };
            unit.encode(&mut buf);
            // write hash
            buf.extend_from_slice(&Hash::default());
            // write compressed size
            buf.extend_from_slice(&COMPRESSED_SIZE.to_be_bytes());
            // write uncompressed size
            buf.extend_from_slice(&UNCOMPRESSED_SIZE.to_be_bytes());
            // add dummy data to fill the compressed size
            buf.extend(iter::repeat_n(0, COMPRESSED_SIZE as usize));
        }

        let meta = parse_metadata(Cursor::new(buf)).unwrap();
        assert_eq!(meta.chain_id, header.network_id);

        let mut units: Vec<_> = meta.units.into_iter().collect();
        units.sort_by(|(unit_name1, _), (unit_name2, _)| unit_name1.cmp(unit_name2));
        for (i, (unit_name, descriptor)) in units.into_iter().enumerate() {
            assert_eq!(*unit_name, format!("test_unit{i}"));
            assert_eq!(descriptor.compressed_size, COMPRESSED_SIZE);
            assert_eq!(descriptor.uncompressed_size, UNCOMPRESSED_SIZE);
        }
    }

    #[test]
    fn parse_metadata_fails_if_headers_mismatch() {
        const COMPRESSED_SIZE: u64 = 1000;
        const UNCOMPRESSED_SIZE: u64 = 1000;
        let mut header = GenesisHeader {
            genesis_id: [0u8; 32],
            network_id: 1,
            network_name: "test_network".to_string(),
        };

        let mut buf = Vec::new();
        for i in 0..3 {
            // write the header and version
            buf.extend_from_slice(&HEADER);
            buf.extend_from_slice(&VERSION);
            // modify the header
            header.network_id = i;
            // write unit
            let unit = Unit {
                unit_name: format!("test_unit{i}"),
                header: header.clone(),
            };
            unit.encode(&mut buf);
            // write hash
            buf.extend_from_slice(&Hash::default());
            // write compressed size
            buf.extend_from_slice(&COMPRESSED_SIZE.to_be_bytes());
            // write uncompressed size
            buf.extend_from_slice(&UNCOMPRESSED_SIZE.to_be_bytes());
            // add dummy data to fill the compressed size
            buf.extend(iter::repeat_n(0, COMPRESSED_SIZE as usize));
        }

        let meta = parse_metadata(Cursor::new(buf));
        assert!(matches!(
            meta,
            Err(Error::GFile(GFileError::HeaderMismatch))
        ));
    }
}
