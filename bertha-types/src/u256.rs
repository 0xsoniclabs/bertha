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
    fmt::{self, Display, Formatter},
    num::IntErrorKind,
    ops::{Add, Sub},
};

use alloy_rlp::{Decodable, Encodable};
use bnum::{
    cast::{As, CastFrom},
    types::U256 as BnumU256,
};

use super::parse_hex_error::ParseHexError;
use crate::HexConvert;

/// Unsigned integer type that can be de-/serialized from and to hex strings, using a
/// variable-length encoding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct U256(BnumU256);

impl U256 {
    pub const ZERO: Self = U256(BnumU256::MIN);
    pub const MAX: Self = U256(BnumU256::MAX);

    /// Constructs a [U256] from a byte array in big-endian order.
    pub fn from_be_bytes(bytes: [u8; 32]) -> Self {
        Self(BnumU256::from_be_bytes(bytes))
    }

    /// Constructs a [U256] from a byte array in little-endian order.
    pub fn from_le_bytes(bytes: [u8; 32]) -> Self {
        Self(BnumU256::from_le_bytes(bytes))
    }

    /// Returns the big-endian representation of the number as a byte array.
    pub fn to_be_bytes(&self) -> [u8; 32] {
        self.0.to_be_bytes()
    }

    /// Converts the number to a [u64] using only the least significant 8 bytes.
    pub fn to_least_significant_u64(self) -> u64 {
        self.0.as_()
    }
}

impl HexConvert for U256 {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        BnumU256::from_str_radix(value.trim_start_matches("0x"), 16)
            .map(Self)
            .map_err(ParseHexError::from)
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
    BnumU256: CastFrom<I>,
{
    fn from(value: I) -> Self {
        U256(BnumU256::cast_from(value))
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
        if *self == U256::ZERO {
            // Special handling for zero: Encoding a single-element array of zero results in "0x00",
            // whereas encoding the value 0 results in "0x". We need the latter to
            // produce the correct block hash.
            0u64.encode(out);
        } else {
            let bytes = self.to_be_bytes();
            let mut s = bytes.as_slice();
            // Strip leading zeros to get minimal big-endian representation.
            while s.len() > 1 && s[0] == 0 {
                s = &s[1..];
            }
            s.encode(out);
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
        let x = U256::from(BnumU256::cast_from(123u8));
        assert_eq!(x.0, BnumU256::cast_from(123u8));
    }

    #[test]
    fn can_be_constructed_from_unsigned_integer_types() {
        let x = U256::from(1u8);
        assert_eq!(x, U256::from(BnumU256::cast_from(1u8)));
        assert_eq!(x.to_string(), "1");
        let x = U256::from(2u16);
        assert_eq!(x, U256::from(BnumU256::cast_from(2u16)));
        assert_eq!(x.to_string(), "2");
        let x = U256::from(3u32);
        assert_eq!(x, U256::from(BnumU256::cast_from(3u32)));
        assert_eq!(x.to_string(), "3");
        let x = U256::from(4u64);
        assert_eq!(x, U256::from(BnumU256::cast_from(4u64)));
        assert_eq!(x.to_string(), "4");
        let x = U256::from(5u128);
        assert_eq!(x, U256::from(BnumU256::cast_from(5u128)));
        assert_eq!(x.to_string(), "5");
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let x = U256::try_from_hex("0xdeadbeef").unwrap();
        assert_eq!(x.0, BnumU256::cast_from(3735928559u64));

        // Even-length hex string
        let x = U256::try_from_hex("0x00").unwrap();
        assert_eq!(x.0, BnumU256::cast_from(0u64));

        // Odd-length hex string
        let x = U256::try_from_hex("0x0").unwrap();
        assert_eq!(x.0, BnumU256::cast_from(0u64));

        // Without 0x prefix
        let x = U256::try_from_hex("10").unwrap();
        assert_eq!(x.0, BnumU256::cast_from(16u64));
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
        assert_eq!(y, BnumU256::cast_from(123u8));
    }

    #[test]
    fn can_be_converted_to_hex_string() {
        let x = U256::from(256u16);
        let hex = x.to_hex();
        assert_eq!(hex, "0x100");
    }

    #[test]
    fn can_be_converted_to_and_from_be_bytes() {
        let x = U256::ZERO;
        let bytes = x.to_be_bytes();
        assert_eq!(bytes, [0; 32]);
        assert_eq!(U256::from_be_bytes(bytes), x);

        let x = U256::from(256u64);
        let bytes = x.to_be_bytes();
        assert_eq!(
            bytes,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 1, 0
            ]
        );
        assert_eq!(U256::from_be_bytes(bytes), x);

        let x = U256::from(u64::MAX);
        let bytes = x.to_be_bytes();
        assert_eq!(
            bytes,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255,
                255, 255, 255, 255, 255, 255
            ]
        );
        assert_eq!(U256::from_be_bytes(bytes), x);

        let x = U256::MAX;
        let bytes = x.to_be_bytes();
        assert_eq!(
            bytes,
            [
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255
            ]
        );
        assert_eq!(U256::from_be_bytes(bytes), x);
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
        assert_eq!(z.unwrap().0, BnumU256::cast_from(3u8));
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
        assert_eq!(z.unwrap().0, BnumU256::cast_from(1u8));
    }

    #[test]
    fn sub_produces_an_error_on_underflow() {
        let x = U256::from(1u8);
        let y = U256::from(2u8);
        let z = x - y;
        assert_eq!(z, Err(IntErrorKind::NegOverflow));
    }

    #[test]
    fn to_least_significant_u64_converts_and_truncates_if_necessary() {
        let cases = [
            (U256::from(u64::MIN), u64::MIN),
            (U256::from(1), 1),
            (U256::from(u64::MAX - 1), u64::MAX - 1),
            (U256::from(u64::MAX), u64::MAX),
            (U256::from(u64::MAX).add(U256::from(1u64)).unwrap(), 0),
            (U256::from(u64::MAX).add(U256::from(2u64)).unwrap(), 1),
            (
                U256::from(u64::MAX).add(U256::from(u64::MAX - 1)).unwrap(),
                u64::MAX - 2,
            ),
            (
                U256::from(u64::MAX).add(U256::from(u64::MAX)).unwrap(),
                u64::MAX - 1,
            ),
        ];
        for (u256, expected) in cases {
            assert_eq!(u256.to_least_significant_u64(), expected);
        }
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
