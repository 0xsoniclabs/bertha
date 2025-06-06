use std::{
    fmt::{self, Display, Formatter},
    num::IntErrorKind,
    ops::{Add, Sub},
};

use alloy_rlp::{Decodable, Encodable};
use bnum::types::U256 as BnumU256;

use super::parse_hex_error::ParseHexError;
use crate::HexConvert;

/// Unsigned integer type that can be de-/serialized from and to hex strings, using a
/// variable-length encoding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U256(BnumU256);

impl U256 {
    pub const ZERO: Self = U256(BnumU256::ZERO);
    pub const MAX: Self = U256(BnumU256::MAX);
}

impl HexConvert for U256 {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        BnumU256::from_str_radix(value.trim_start_matches("0x"), 16)
            .map(Self)
            .map_err(Into::<ParseHexError>::into)
    }

    fn to_hex(&self) -> String {
        format!("0x{:x}", self.0)
    }
}

impl Display for U256 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<I> From<I> for U256
where
    BnumU256: From<I>,
{
    fn from(value: I) -> Self {
        U256(BnumU256::from(value))
    }
}

// We implement Into instead of From to avoid conflict with
// blanket implementation above.
#[allow(clippy::from_over_into)]
impl Into<BnumU256> for U256 {
    fn into(self) -> BnumU256 {
        self.0
    }
}

impl Add for U256 {
    type Output = Result<U256, IntErrorKind>;

    fn add(self, rhs: Self) -> Self::Output {
        self.0
            .checked_add(rhs.0)
            .map(U256)
            .ok_or(IntErrorKind::PosOverflow)
    }
}

impl Sub for U256 {
    type Output = Result<U256, IntErrorKind>;

    fn sub(self, rhs: Self) -> Self::Output {
        self.0
            .checked_sub(rhs.0)
            .map(U256)
            .ok_or(IntErrorKind::NegOverflow)
    }
}

impl Encodable for U256 {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        let s = self.0.to_radix_be(256);
        if s.len() == 1 && s[0] == 0 {
            // Special handling for zero: Encoding a single-element array of zero results in "0x00",
            // whereas encoding the value 0 results in "0x". We need the latter to
            // produce the correct block hash.
            0u64.encode(out);
        } else {
            s.as_slice().encode(out);
        }
    }
}

impl Decodable for U256 {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let bytes = alloy_rlp::Header::decode_bytes(buf, false)?;
        Ok(Self(BnumU256::from_be_slice(bytes).ok_or(
            alloy_rlp::Error::Custom("Failed to decode U256"),
        )?))
    }
}

#[cfg(test)]
mod test {
    use std::num::IntErrorKind;

    use super::*;

    #[test]
    fn can_be_constructed_from_bnum_type() {
        let x = U256::from(BnumU256::from(123u8));
        assert_eq!(x.0, BnumU256::from(123u8));
    }

    #[test]
    fn can_be_constructed_from_unsigned_integer_types() {
        let x = U256::from(1u8);
        assert_eq!(x, U256::from(BnumU256::from(1u8)));
        assert_eq!(x.to_string(), "1");
        let x = U256::from(2u16);
        assert_eq!(x, U256::from(BnumU256::from(2u16)));
        assert_eq!(x.to_string(), "2");
        let x = U256::from(3u32);
        assert_eq!(x, U256::from(BnumU256::from(3u32)));
        assert_eq!(x.to_string(), "3");
        let x = U256::from(4u64);
        assert_eq!(x, U256::from(BnumU256::from(4u64)));
        assert_eq!(x.to_string(), "4");
        let x = U256::from(5u128);
        assert_eq!(x, U256::from(BnumU256::from(5u128)));
        assert_eq!(x.to_string(), "5");
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let x = U256::try_from_hex("0xdeadbeef").unwrap();
        assert_eq!(x.0, BnumU256::from(3735928559u64));

        // Even-length hex string
        let x = U256::try_from_hex("0x00").unwrap();
        assert_eq!(x.0, BnumU256::ZERO);

        // Odd-length hex string
        let x = U256::try_from_hex("0x0").unwrap();
        assert_eq!(x.0, BnumU256::ZERO);

        // Without 0x prefix
        let x = U256::try_from_hex("10").unwrap();
        assert_eq!(x.0, BnumU256::from(16u8));
    }

    #[test]
    fn malformed_hex_string_produces_error() {
        let x = U256::try_from_hex("0x");
        assert_eq!(x, Err(ParseHexError::IntError(IntErrorKind::Empty)));
        assert_eq!(
            x.unwrap_err().to_string(),
            "hex string cannot be represented as a number of the target type: IntErrorKind::Empty"
        );

        let x = U256::try_from_hex("xyz");
        assert_eq!(x, Err(ParseHexError::InvalidCharacter));
        assert_eq!(
            x.unwrap_err().to_string(),
            "hex string contains invalid character(s)"
        );

        let x = U256::try_from_hex(
            "0x10000000000000000000000000000000000000000000000000000000000000000",
        );
        assert_eq!(x, Err(ParseHexError::IntError(IntErrorKind::PosOverflow)));
    }

    #[test]
    fn can_be_converted_to_underlying_bnum_type() {
        let x = U256::from(123u8);
        let y: BnumU256 = x.into();
        assert_eq!(y, BnumU256::from(123u8));
    }

    #[test]
    fn can_be_converted_to_hex_string() {
        let x = U256::from(256u16);
        let hex = x.to_hex();
        assert_eq!(hex, "0x100");
    }

    #[test]
    fn format_display_uses_decimal_representation() {
        let x = U256::from(123u8);
        assert_eq!(format!("{x}"), "123");
    }

    #[test]
    fn can_be_added() {
        let x = U256::from(1u8);
        let y = U256::from(2u8);
        let z = x + y;
        assert_eq!(z.unwrap().0, BnumU256::from(3u8));
    }

    #[test]
    fn add_produces_an_error_on_overflow() {
        let x = U256::MAX;
        let y = U256::from(1u8);
        let z = x + y;
        assert_eq!(z, Err(IntErrorKind::PosOverflow));
    }

    #[test]
    fn can_be_subtracted() {
        let x = U256::from(3u8);
        let y = U256::from(2u8);
        let z = x - y;
        assert_eq!(z.unwrap().0, BnumU256::from(1u8));
    }

    #[test]
    fn sub_produces_an_error_on_underflow() {
        let x = U256::from(1u8);
        let y = U256::from(2u8);
        let z = x - y;
        assert_eq!(z, Err(IntErrorKind::NegOverflow));
    }

    #[test]
    fn can_be_serialized_to_rlp() {
        let x = U256::try_from_hex("0xdeadbeef").unwrap();
        let rlp = alloy_rlp::encode(x);
        assert_eq!(rlp, const_hex::decode("84deadbeef").unwrap());

        let x = U256::from(0u8);
        let rlp = alloy_rlp::encode(x);
        assert_eq!(rlp, const_hex::decode("80").unwrap());
    }

    #[test]
    fn can_be_deserialized_from_rlp() {
        assert_eq!(
            U256::decode(&mut [0x80].as_slice()).unwrap(),
            U256::from(0u64)
        );
        assert_eq!(
            U256::decode(&mut [0x01].as_slice()).unwrap(),
            U256::from(1u64)
        );
        assert_eq!(
            U256::decode(&mut [0x84, 0xde, 0xad, 0xbe, 0xef].as_slice()).unwrap(),
            U256::from(3735928559u64)
        );
    }
}
