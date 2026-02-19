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

use alloy_rlp::{Decodable, Encodable};
use serde::{Deserialize, Serialize};

use crate::{HexConvert, parse_hex_error::ParseHexError};

/// A wrapper type to encode and decode an optional value as a RLP nil (empty string).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RlpNil<T>(pub Option<T>);

impl<T: Decodable> Decodable for RlpNil<T> {
    fn decode(from: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        if from.starts_with(&[0x80]) {
            *from = &from[1..];
            Ok(RlpNil(None))
        } else {
            Ok(RlpNil(Some(T::decode(from)?)))
        }
    }
}

impl<T: Encodable> Encodable for RlpNil<T> {
    fn length(&self) -> usize {
        match &self.0 {
            Some(value) => value.length(),
            None => 1, // nil is encoded as the empty string (single byte 0x80)
        }
    }

    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        match &self.0 {
            Some(value) => value.encode(out),
            None => out.put_bytes(alloy_rlp::EMPTY_STRING_CODE, 1),
        }
    }
}

impl<T> RlpNil<T> {
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

/// A wrapper type to encode and decode [`Vec<u8>`] as a RLP string and not as a RLP list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RlpString(pub Vec<u8>);

impl Encodable for RlpString {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.0.as_slice().encode(out);
    }
}

impl Decodable for RlpString {
    fn decode(rlp: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        Ok(Self(alloy_rlp::Header::decode_bytes(rlp, false)?.to_vec()))
    }
}

impl HexConvert for RlpString {
    fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        Vec::try_from_hex(value).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use alloy_rlp::{Decodable, Encodable};

    use super::*;

    #[test]
    fn rlpnil_encodes_nil_to_empty_string_code() {
        let nil: RlpNil<u64> = RlpNil(None);
        let mut out = Vec::new();
        nil.encode(&mut out);
        assert_eq!(out, vec![alloy_rlp::EMPTY_STRING_CODE]);
    }

    #[test]
    fn rlpnil_encodes_value_to_its_normal_encoding() {
        let nil = RlpNil(Some(u64::MAX));
        let mut out = Vec::new();
        nil.encode(&mut out);
        let mut normal = Vec::new();
        u64::MAX.encode(&mut normal);
        assert_eq!(out, normal);
    }

    #[test]
    fn rlpnil_decodes_nil_from_empty_string_code() {
        let encoded = vec![alloy_rlp::EMPTY_STRING_CODE];
        let decoded: RlpNil<u64> = RlpNil::decode(&mut encoded.as_slice()).unwrap();
        assert_eq!(decoded, RlpNil(None));
    }

    #[test]
    fn rlpnil_decodes_value_from_normal_encoding() {
        let mut encoded = Vec::new();
        u64::MAX.encode(&mut encoded);
        let mut encoded_slice = encoded.as_slice();
        let decoded: RlpNil<u64> = RlpNil::decode(&mut encoded_slice).unwrap();
        assert_eq!(decoded, RlpNil(Some(u64::MAX)));
    }

    #[test]
    fn rlpstring_encodes_vec_u8_as_slice_u8() {
        let s = RlpString(vec![0x01, 0x02, 0x03]);
        let mut out = Vec::new();
        s.encode(&mut out);
        let mut expected = Vec::new();
        [0x01, 0x02, 0x03].as_slice().encode(&mut expected);
        assert_eq!(out, expected);
    }

    #[test]
    fn rlpstring_decodes_vec_u8_as_slice_u8() {
        let slice = [0x01, 0x02, 0x03].as_slice();
        let mut encoded = Vec::new();
        slice.encode(&mut encoded);
        let decoded = RlpString::decode(&mut encoded.as_slice()).unwrap();
        assert_eq!(decoded.0, slice);
    }

    #[test]
    fn rlpstring_to_hex_returns_hex_encoding_of_bytes() {
        let s = RlpString(vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(s.to_hex(), "0xdeadbeef");
    }

    #[test]
    fn rlpstring_try_from_hex_parses_vec_u8() {
        let s = RlpString::try_from_hex("0x010203").unwrap();
        assert_eq!(s.0, vec![0x01, 0x02, 0x03]);
    }
}
