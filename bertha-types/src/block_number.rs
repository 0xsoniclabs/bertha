use std::fmt::Display;

use alloy_rlp::RlpEncodableWrapper;
use serde::{Deserialize, Serialize};

use crate::{SerializableU64, parse_hex_error::ParseHexError};

/// BlockNumber is a 64-bit unsigned integer used to represent block numbers in Ethereum-compatible
/// blockchains.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    RlpEncodableWrapper,
    PartialOrd,
    Ord,
)]
pub struct BlockNumber(SerializableU64);

impl BlockNumber {
    pub const ZERO: Self = Self(SerializableU64::ZERO);

    /// Constructs a new BlockNumber from a hex string.
    /// The hex string can be prefixed with "0x" or not.
    pub fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        SerializableU64::try_from_hex(value).map(Self)
    }

    /// Returns the hex representation of the BlockNumber.
    /// The returned string is prefixed with "0x".
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

impl<T> From<T> for BlockNumber
where
    SerializableU64: From<T>,
{
    fn from(value: T) -> Self {
        BlockNumber(SerializableU64::from(value))
    }
}

impl Display for BlockNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use alloy_rlp::Encodable;

    use crate::{BlockNumber, serializable_u64::SerializableU64};

    #[test]
    fn zero_initializes_block_number_to_zero() {
        let block_number = BlockNumber::ZERO;
        assert_eq!(block_number.0, SerializableU64::ZERO);
    }

    #[test]
    fn can_be_constructed_from_unsigned_integer() {
        let block_number = BlockNumber::from(10u64);
        assert_eq!(block_number.0, SerializableU64::from(10u64));
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let hex_string = "0xaff";
        let block_number =
            BlockNumber::try_from_hex(hex_string).expect("Failed to create BlockNumber");
        assert_eq!(block_number.to_hex(), "0xaff");
    }

    #[test]
    fn from_hex_returns_error_on_invalid_input() {
        // Invalid hex string
        let invalid_hex_string = "0xg";
        let result = BlockNumber::try_from_hex(invalid_hex_string);
        assert!(result.is_err());
    }

    #[test]
    fn to_hex_returns_correct_hex_representation() {
        let block_number = BlockNumber::from(10u64);
        assert_eq!(block_number.to_hex(), "0xa");
    }

    #[test]
    fn can_be_encoded_as_number_using_rlp() {
        let num = 10u64;
        // The expected RLP encoding of the number
        let mut ref_rlp = Vec::new();
        num.encode(&mut ref_rlp);
        // Construct the block number and encode it
        let block_number = BlockNumber::from(num);
        let mut buf = Vec::new();
        block_number.encode(&mut buf);

        assert_eq!(buf, ref_rlp);
    }

    #[test]
    fn format_display_uses_decimal_representation() {
        let block_number = BlockNumber::from(10u64);
        assert_eq!(format! {"{block_number}"}, "10");
    }
}
