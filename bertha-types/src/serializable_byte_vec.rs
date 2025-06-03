use std::fmt;

use alloy_rlp::{Encodable, RlpDecodableWrapper};
use const_hex::FromHexError;
use serde::{Deserialize, Serialize};

use crate::parse_hex_error::ParseHexError;

/// Variable-size binary data that can be de-/serialized from and to hex strings, using a
/// fixed-length encoding.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, RlpDecodableWrapper)]
pub struct SerializableByteVec(Vec<u8>);

impl SerializableByteVec {
    pub fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        let vec = match const_hex::decode(value.trim_start_matches("0x")) {
            Ok(v) => Ok(v),
            Err(e) => match e {
                FromHexError::InvalidHexCharacter { .. } => Err(ParseHexError::InvalidCharacter),
                FromHexError::OddLength => Err(ParseHexError::OddLength),
                FromHexError::InvalidStringLength => unreachable!(),
            },
        }?;

        Ok(Self(vec))
    }

    pub fn to_hex(&self) -> String {
        format!("0x{}", const_hex::encode(&self.0))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl From<&[u8]> for SerializableByteVec {
    fn from(value: &[u8]) -> Self {
        Self(value.to_vec())
    }
}

impl fmt::Display for SerializableByteVec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl Serialize for SerializableByteVec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for SerializableByteVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex_str: &str = Deserialize::deserialize(deserializer)?;
        Self::try_from_hex(hex_str).map_err(serde::de::Error::custom)
    }
}

impl Encodable for SerializableByteVec {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.0.as_slice().encode(out);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_be_constructed_from_byte_slice() {
        let b = SerializableByteVec::from([0x12, 0x34].as_slice());
        assert_eq!(b.0, [0x12, 0x34]);
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let b = SerializableByteVec::try_from_hex("0x1234").unwrap();
        assert_eq!(b.0, [0x12, 0x34]);
        // String can be any length
        let b = SerializableByteVec::try_from_hex("0x123456789a").unwrap();
        assert_eq!(b.0, [0x12, 0x34, 0x56, 0x78, 0x9a]);
        let b = SerializableByteVec::try_from_hex("0x").unwrap();
        assert_eq!(b.0, [0x0; 0]);
    }

    #[test]
    fn malformed_hex_string_produces_error() {
        let err = SerializableByteVec::try_from_hex("xyzw").unwrap_err();
        assert_eq!(err, ParseHexError::InvalidCharacter);
        let err = SerializableByteVec::try_from_hex("0x1").unwrap_err();
        assert_eq!(err, ParseHexError::OddLength);
    }

    #[test]
    fn can_be_converted_to_hex_string() {
        let b: SerializableByteVec = SerializableByteVec([0x12, 0x34].to_vec());
        assert_eq!(b.to_hex(), "0x1234");

        // String has fixed length
        let b: SerializableByteVec = SerializableByteVec([0x0, 0x0].to_vec());
        assert_eq!(b.to_hex(), "0x0000");
        let b: SerializableByteVec = SerializableByteVec([0x0, 0x0, 0x0, 0x0].to_vec());
        assert_eq!(b.to_hex(), "0x00000000");
    }

    #[test]
    fn as_bytes_returns_reference_to_underlying_byte_slice() {
        let b: SerializableByteVec = SerializableByteVec([0x12, 0x34].to_vec());
        assert_eq!(b.as_bytes(), &[0x12, 0x34]);

        // Fixed length
        let b: SerializableByteVec = SerializableByteVec([0x0, 0x0, 0x0, 0x0].to_vec());
        assert_eq!(b.as_bytes(), &[0x0, 0x0, 0x0, 0x0]);

        // Empty array
        let b: SerializableByteVec = SerializableByteVec([].to_vec());
        let empty_array: [u8; 0] = [];
        assert_eq!(b.as_bytes(), &empty_array);
    }

    #[test]
    fn can_be_serialized_to_json() {
        let b: SerializableByteVec = SerializableByteVec([0x12, 0x34].to_vec());
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "\"0x1234\"");

        // String has fixed length
        let b: SerializableByteVec = SerializableByteVec([0x0, 0x0].to_vec());
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "\"0x0000\"");
    }

    #[test]
    fn can_be_deserialized_from_json() {
        let json = "\"0x1234\"";
        let b: SerializableByteVec = serde_json::from_str(json).unwrap();
        assert_eq!(b.0, [0x12, 0x34]);

        // Invalid JSON causes error
        let json = ";$";
        let e = serde_json::from_str::<SerializableByteVec>(json).unwrap_err();
        assert_eq!(e.to_string(), "expected value at line 1 column 1");
    }

    #[test]
    fn can_be_serialized_to_rlp() {
        let b = SerializableByteVec([0x12, 0x34].to_vec());
        let rlp = alloy_rlp::encode(b);
        assert_eq!(rlp, const_hex::decode("821234").unwrap());

        // Encoding has fixed length
        let b: SerializableByteVec = SerializableByteVec([0; 4].to_vec());
        let rlp = alloy_rlp::encode(b);
        assert_eq!(rlp, const_hex::decode("8400000000").unwrap());
    }

    #[test]
    fn can_be_printed() {
        let b: SerializableByteVec = SerializableByteVec([0x00, 0x00, 0x00, 0x01].to_vec());
        assert_eq!(format!("{b}"), "0x00000001");
    }
}
