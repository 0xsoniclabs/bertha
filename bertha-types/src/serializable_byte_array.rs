use std::fmt;

use alloy_rlp::{RlpDecodableWrapper, RlpEncodableWrapper};
use const_hex::FromHexError;
use serde::{Deserialize, Serialize};

use crate::types::parse_hex_error::ParseHexError;

/// Fixed-size binary data that can be de-/serialized from and to hex strings, using a fixed-length
/// encoding. Used for opaque fields that are only required for computing block hashes.
#[derive(
    PartialEq, Eq, PartialOrd, Ord, Debug, Clone, RlpDecodableWrapper, RlpEncodableWrapper,
)]
pub struct SerializableByteArray<const N: usize>([u8; N]);

impl<const N: usize> SerializableByteArray<N> {
    pub fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        let array: [u8; N] = match const_hex::decode_to_array(value.trim_start_matches("0x")) {
            Ok(b) => Ok(b),
            Err(e) => match e {
                FromHexError::InvalidHexCharacter { .. } => Err(ParseHexError::InvalidCharacter),
                FromHexError::OddLength => Err(ParseHexError::OddLength),
                FromHexError::InvalidStringLength => Err(ParseHexError::FixedSizeMismatch(
                    N,
                    (value.trim_start_matches("0x").len()) / 2,
                )),
            },
        }?;

        Ok(Self(array))
    }

    pub fn to_hex(&self) -> String {
        format!("0x{}", const_hex::encode(self.0))
    }

    pub fn as_bytes(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> AsRef<[u8]> for SerializableByteArray<N> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<const N: usize> From<[u8; N]> for SerializableByteArray<N> {
    fn from(value: [u8; N]) -> Self {
        Self(value)
    }
}

impl<const N: usize> fmt::Display for SerializableByteArray<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl<const N: usize> Serialize for SerializableByteArray<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de, const N: usize> Deserialize<'de> for SerializableByteArray<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex_str: &str = Deserialize::deserialize(deserializer)?;
        Self::try_from_hex(hex_str).map_err(serde::de::Error::custom)
    }
}

impl<const N: usize> Default for SerializableByteArray<N> {
    fn default() -> Self {
        Self([0; N])
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_be_constructed_from_byte_array() {
        let b = SerializableByteArray::from([0x12, 0x34]);
        assert_eq!(b.0, [0x12, 0x34]);
    }

    fn can_be_default_initialized() {
        let b: SerializableByteArray<2> = SerializableByteArray::default();
        assert_eq!(b.0, [0x0, 0x0]);
        let b: SerializableByteArray<4> = SerializableByteArray::default();
        assert_eq!(b.0, [0x0, 0x0, 0x0, 0x0]);
        let b: SerializableByteArray<0> = SerializableByteArray::default();
        assert_eq!(b.0, [] as [u8; 0]);
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let b = SerializableByteArray::<2>::try_from_hex("0x1234").unwrap();
        assert_eq!(b.0, [0x12, 0x34]);
        let b = SerializableByteArray::<0>::try_from_hex("0x").unwrap();
        assert_eq!(b.0, [0x0; 0]);
    }

    #[test]
    fn hex_string_length_has_to_match_exactly() {
        let err = SerializableByteArray::<2>::try_from_hex("0x12").unwrap_err();
        assert_eq!(
            err.to_string(),
            "hex string is required to have a length of exactly 2 bytes, but has 1 bytes"
        );
        let err = SerializableByteArray::<4>::try_from_hex("0x123456789a").unwrap_err();
        assert_eq!(
            err.to_string(),
            "hex string is required to have a length of exactly 4 bytes, but has 5 bytes"
        );
    }

    #[test]
    fn malformed_hex_string_produces_error() {
        let err = SerializableByteArray::<2>::try_from_hex("xyzw").unwrap_err();
        assert_eq!(err, ParseHexError::InvalidCharacter);
        let err = SerializableByteArray::<0>::try_from_hex("0x1").unwrap_err();
        assert_eq!(err, ParseHexError::OddLength);
    }

    #[test]
    fn can_be_converted_to_hex_string() {
        let b: SerializableByteArray<2> = SerializableByteArray([0x12, 0x34]);
        assert_eq!(b.to_hex(), "0x1234");

        // String has fixed length
        let b: SerializableByteArray<2> = SerializableByteArray([0x0, 0x0]);
        assert_eq!(b.to_hex(), "0x0000");
        let b: SerializableByteArray<4> = SerializableByteArray([0x0, 0x0, 0x0, 0x0]);
        assert_eq!(b.to_hex(), "0x00000000");
    }

    #[test]
    fn as_bytes_returns_reference_to_underlying_byte_array() {
        let b: SerializableByteArray<2> = SerializableByteArray([0x12, 0x34]);
        assert_eq!(b.as_bytes(), &[0x12, 0x34]);

        // Fixed length
        let b: SerializableByteArray<4> = SerializableByteArray([0x0, 0x0, 0x0, 0x0]);
        assert_eq!(b.as_bytes(), &[0x0, 0x0, 0x0, 0x0]);

        // Empty array
        let b: SerializableByteArray<0> = SerializableByteArray([]);
        let empty_array: [u8; 0] = [];
        assert_eq!(b.as_bytes(), &empty_array);
    }

    #[test]
    fn can_be_serialized_to_json() {
        let b: SerializableByteArray<2> = SerializableByteArray([0x12, 0x34]);
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "\"0x1234\"");

        // String has fixed length
        let b: SerializableByteArray<2> = SerializableByteArray([0x0, 0x0]);
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "\"0x0000\"");
    }

    #[test]
    fn can_be_deserialized_from_json() {
        let json = "\"0x1234\"";
        let b: SerializableByteArray<2> = serde_json::from_str(json).unwrap();
        assert_eq!(b.0, [0x12, 0x34]);

        // Length mismatch causes error
        let json = "\"0x00\"";
        let e = serde_json::from_str::<SerializableByteArray<4>>(json).unwrap_err();
        assert_eq!(
            e.to_string(),
            "hex string is required to have a length of exactly 4 bytes, but has 1 bytes"
        );

        // Invalid JSON causes error
        let json = ";$";
        let e = serde_json::from_str::<SerializableByteArray<2>>(json).unwrap_err();
        assert_eq!(e.to_string(), "expected value at line 1 column 1");
    }

    #[test]
    fn can_be_serialized_to_rlp() {
        let b = SerializableByteArray([0x12, 0x34]);
        let rlp = alloy_rlp::encode(b);
        assert_eq!(rlp, const_hex::decode("821234").unwrap());

        // Encoding has fixed length
        let b: SerializableByteArray<4> = SerializableByteArray([0; 4]);
        let rlp = alloy_rlp::encode(b);
        assert_eq!(rlp, const_hex::decode("8400000000").unwrap());
    }

    #[test]
    fn can_be_printed() {
        let b: SerializableByteArray<4> = SerializableByteArray([0x00, 0x00, 0x00, 0x01]);
        assert_eq!(format!("{b}"), "0x00000001");
    }
}
