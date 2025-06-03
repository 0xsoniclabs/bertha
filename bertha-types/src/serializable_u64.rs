use std::fmt::Display;

use alloy_rlp::{RlpDecodableWrapper, RlpEncodableWrapper};
use serde::{Deserialize, Serialize};

use crate::parse_hex_error::ParseHexError;

/// u64 that can be serialized to and deserialized from a hex string.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Copy,
    RlpDecodableWrapper,
    RlpEncodableWrapper,
)]
pub struct SerializableU64(u64);

impl SerializableU64 {
    pub const ZERO: Self = Self(0u64);

    /// Constructs a new SerializableU64 from a hex string.
    /// The hex string can be prefixed with "0x" or not.
    pub fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        u64::from_str_radix(value.trim_start_matches("0x"), 16)
            .map(Self)
            .map_err(Into::<ParseHexError>::into)
    }

    /// Returns the hex representation of the SerializableU64.
    /// The returned string is prefixed with "0x".
    pub fn to_hex(self) -> String {
        format!("0x{:x}", self.0)
    }
}

impl<T> From<T> for SerializableU64
where
    u64: From<T>,
{
    fn from(value: T) -> Self {
        Self(value.into())
    }
}

#[allow(clippy::from_over_into)]
impl Into<u64> for SerializableU64 {
    fn into(self) -> u64 {
        self.0
    }
}

impl Serialize for SerializableU64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_hex().as_str())
    }
}

impl<'de> Deserialize<'de> for SerializableU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex_str: &str = Deserialize::deserialize(deserializer)?;
        Self::try_from_hex(hex_str).map_err(serde::de::Error::custom)
    }
}

impl Display for SerializableU64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use std::num::IntErrorKind;

    use super::*;

    #[test]
    fn can_be_constructed_from_hex_string() {
        let serializable_u64 = SerializableU64::try_from_hex("0x123456789abcdef0").unwrap();
        assert_eq!(
            serializable_u64.0.to_be_bytes(),
            [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]
        );

        // Even-length hex string
        let x = SerializableU64::try_from_hex("0x00").unwrap();
        assert_eq!(x.0, 0u64);

        // Odd-length hex string
        let x = SerializableU64::try_from_hex("0x0").unwrap();
        assert_eq!(x.0, 0u64);

        // Without 0x prefix
        let x = SerializableU64::try_from_hex("10").unwrap();
        assert_eq!(x.0, 16u64);
    }

    #[test]
    fn malformed_hex_string_produces_error() {
        let e = SerializableU64::try_from_hex("0x").unwrap_err();
        assert_eq!(e, ParseHexError::IntError(IntErrorKind::Empty));
        assert_eq!(
            e.to_string(),
            "hex string cannot be represented as a number of the target type: IntErrorKind::Empty"
        );

        let e = SerializableU64::try_from_hex("xyz").unwrap_err();
        assert_eq!(e, ParseHexError::InvalidCharacter);
        assert_eq!(e.to_string(), "hex string contains invalid character(s)");

        let e = SerializableU64::try_from_hex(
            const_hex::encode((u128::from(u64::MAX) + 1u128).to_be_bytes()).as_str(),
        )
        .unwrap_err();
        assert_eq!(
            e,
            crate::parse_hex_error::ParseHexError::IntError(IntErrorKind::PosOverflow)
        );
    }

    #[test]
    fn can_be_converted_to_u64() {
        let n = SerializableU64::try_from_hex("0xffffffffffffffff").unwrap();
        let i: u64 = n.into();
        assert_eq!(i, u64::MAX);
    }

    #[test]
    fn can_be_serialized_to_json() {
        let serializable_u64 = SerializableU64::try_from_hex("0x123456789abcdef0").unwrap();
        let json = serde_json::to_string(&serializable_u64).unwrap();
        assert_eq!(json, "\"0x123456789abcdef0\"");
    }

    #[test]
    fn can_be_deserialized_from_json() {
        let json = "\"0x123456789abcdef0\"";
        let serializable_u64: SerializableU64 = serde_json::from_str(json).unwrap();
        assert_eq!(
            serializable_u64.0,
            u64::from_str_radix("123456789abcdef0", 16).unwrap()
        );
    }

    #[test]
    fn format_display_uses_decimal_representation() {
        let n = SerializableU64::try_from_hex("0x000000000000000f").unwrap();
        assert_eq!(format!("{n}"), "15");
    }
}
