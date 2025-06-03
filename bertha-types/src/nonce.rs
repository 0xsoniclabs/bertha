use std::fmt;

use alloy_rlp::{RlpDecodableWrapper, RlpEncodableWrapper};
use serde::{Deserialize, Serialize};

use crate::types::{SerializableU64, parse_hex_error::ParseHexError};

/// Nonce is a 64-bit unsigned integer used to represent the nonce of a transaction in
/// Ethereum-compatible blockchains. The nonce is a unique identifier for each transaction sent from
/// an address.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    RlpDecodableWrapper,
    RlpEncodableWrapper,
)]
pub struct Nonce(SerializableU64);

impl Nonce {
    pub(crate) fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        SerializableU64::try_from_hex(value).map(Self)
    }

    fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

impl<T> From<T> for Nonce
where
    SerializableU64: From<T>,
{
    fn from(value: T) -> Self {
        Nonce(SerializableU64::from(value))
    }
}

#[allow(clippy::from_over_into)]
impl Into<u64> for Nonce {
    fn into(self) -> u64 {
        self.0.into()
    }
}

impl fmt::Display for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use alloy_rlp::Encodable;

    use super::*;

    #[test]
    fn can_be_constructed_from_unsigned_integer() {
        let nonce = Nonce::from(10u64);
        assert_eq!(nonce.0, SerializableU64::from(10u64));
    }

    #[test]
    fn can_be_converted_to_u64() {
        let n = Nonce::try_from_hex("0xffffffffffffffff").unwrap();
        let i: u64 = n.into();
        assert_eq!(i, u64::MAX);
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let hex_string = "0xaff";
        let nonce = Nonce::try_from_hex(hex_string).expect("Failed to create Nonce");
        assert_eq!(nonce.to_hex(), "0xaff");
    }

    #[test]
    fn from_hex_returns_error_on_invalid_input() {
        // Invalid hex string
        let invalid_hex_string = "0xg";
        let result = Nonce::try_from_hex(invalid_hex_string);
        assert!(result.is_err());
    }

    #[test]
    fn to_hex_returns_correct_hex_representation() {
        let nonce = Nonce::from(10u64);
        assert_eq!(nonce.to_hex(), "0xa");
    }

    #[test]
    fn can_be_encoded_as_byte_vector_using_rlp() {
        let num = 10u64;
        // The expected RLP encoding of the nonce
        let mut ref_rlp = Vec::new();
        num.encode(&mut ref_rlp);
        // Construct the nonce and encode it
        let nonce = Nonce::from(num);
        let mut buf = Vec::new();
        nonce.encode(&mut buf);
        assert_eq!(buf, ref_rlp);
    }

    #[test]
    fn format_display_uses_decimal_representation() {
        let n = Nonce::try_from_hex("0x000000000000000f").unwrap();
        assert_eq!(format!("{n}"), "15");
    }
}
