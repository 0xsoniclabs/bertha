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

use const_hex::FromHexError;

use crate::parse_hex_error::ParseHexError;

/// A type which can be constructed from a hex string and converted to a hex string.
/// This is used primarily for JSON RPC because there all types are represented as hex strings.
pub trait HexConvert: Sized {
    /// Attempts to parse this type from the hex string.
    /// The can be prefixed with "0x" or not.
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError>;

    /// Converts the type to a hex string prefixed with "0x".
    fn to_hex(&self) -> String;
}

impl<const N: usize> HexConvert for [u8; N] {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        match const_hex::decode_to_array(value.trim_start_matches("0x")) {
            Ok(a) => Ok(a),
            Err(e) => match e {
                FromHexError::InvalidHexCharacter { .. } => Err(ParseHexError::InvalidCharacter),
                FromHexError::OddLength => Err(ParseHexError::OddLength),
                FromHexError::InvalidStringLength => Err(ParseHexError::FixedSizeMismatch(
                    N,
                    (value.trim_start_matches("0x").len()) / 2,
                )),
            },
        }
    }

    fn to_hex(&self) -> String {
        format!("0x{}", const_hex::encode(self))
    }
}

impl HexConvert for Vec<u8> {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        match const_hex::decode(value.trim_start_matches("0x")) {
            Ok(a) => Ok(a),
            Err(e) => match e {
                FromHexError::InvalidHexCharacter { .. } => Err(ParseHexError::InvalidCharacter),
                FromHexError::OddLength => Err(ParseHexError::OddLength),
                FromHexError::InvalidStringLength => unreachable!(),
            },
        }
    }

    fn to_hex(&self) -> String {
        format!("0x{}", const_hex::encode(self))
    }
}

impl HexConvert for u64 {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        u64::from_str_radix(value.trim_start_matches("0x"), 16).map_err(ParseHexError::from)
    }

    fn to_hex(&self) -> String {
        format!("0x{self:x}")
    }
}

impl HexConvert for u8 {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        u8::from_str_radix(value.trim_start_matches("0x"), 16).map_err(ParseHexError::from)
    }

    fn to_hex(&self) -> String {
        format!("0x{self:x}")
    }
}

#[cfg(test)]
mod tests {
    use std::num::IntErrorKind;

    use super::HexConvert;
    use crate::parse_hex_error::ParseHexError;

    #[test]
    fn byte_array_can_be_constructed_from_hex_string() {
        let a = <[u8; 2]>::try_from_hex("0x1234").unwrap();
        assert_eq!(a, [0x12, 0x34]);
        let a = <[u8; 0]>::try_from_hex("0x").unwrap();
        assert_eq!(a, [0x0; 0]);
        let a = <[u8; 0]>::try_from_hex("").unwrap();
        assert_eq!(a, [0x0; 0]);
    }

    #[test]
    fn byte_array_from_hex_string_checks_that_length_matches_exactly() {
        // empty array from empty hex string
        let arr = <[u8; 0]>::try_from_hex("0x").unwrap();
        assert_eq!(arr, <[u8; 0]>::default());
        // array of length one from empty hex string
        let err = <[u8; 1]>::try_from_hex("0x").unwrap_err();
        assert_eq!(
            err.to_string(),
            "hex string is required to have a length of exactly 1 bytes, but has 0 bytes"
        );
        // array of length one from hex string of length one
        let arr = <[u8; 1]>::try_from_hex("0x01").unwrap();
        assert_eq!(arr, [0x01]);
        // array of length one from hex string of length two
        let err = <[u8; 1]>::try_from_hex("0x0102").unwrap_err();
        assert_eq!(
            err.to_string(),
            "hex string is required to have a length of exactly 1 bytes, but has 2 bytes"
        );
    }

    #[test]
    fn byte_array_from_malformed_hex_string_produces_error() {
        let err = <[u8; 2]>::try_from_hex("xyzw").unwrap_err();
        assert_eq!(err, ParseHexError::InvalidCharacter);
        let err = <[u8; 2]>::try_from_hex("0xxyzw").unwrap_err();
        assert_eq!(err, ParseHexError::InvalidCharacter);
        let err = <[u8; 0]>::try_from_hex("0x1").unwrap_err();
        assert_eq!(err, ParseHexError::OddLength);
    }

    #[test]
    fn byte_array_can_be_converted_to_hex_string() {
        let a = [];
        assert_eq!(a.to_hex(), "0x");
        let a = [0x12, 0x34];
        assert_eq!(a.to_hex(), "0x1234");

        // String has fixed length
        let a = [0x0, 0x0];
        assert_eq!(a.to_hex(), "0x0000");
        let a = [0x0, 0x0, 0x0, 0x0];
        assert_eq!(a.to_hex(), "0x00000000");
    }

    #[rstest::rstest]
    #[case::empty("", &[])]
    #[case::only_prefix("0x", &[])]
    #[case::multi_byte("0x1234", &[0x12, 0x34])]
    fn byte_vec_can_be_constructed_from_hex_string(#[case] input: &str, #[case] expected: &[u8]) {
        let v = Vec::try_from_hex(input).unwrap();
        assert_eq!(v, expected);
    }

    #[rstest::rstest]
    #[case::odd_length("0x1", ParseHexError::OddLength)]
    #[case::invalid_chars_without_prefix("xy", ParseHexError::InvalidCharacter)]
    #[case::invalid_chars_with_prefix("0xxy", ParseHexError::InvalidCharacter)]
    fn bytes_vec_from_malformed_hex_string_produces_error(
        #[case] input: &str,
        #[case] expected_err: ParseHexError,
    ) {
        let err = Vec::try_from_hex(input).unwrap_err();
        assert_eq!(err, expected_err);
    }

    #[rstest::rstest]
    #[case::empty(vec![], "0x")]
    #[case::non_empty(vec![0x12, 0x34], "0x1234")]
    #[case::zeros_two_bytes(vec![0x0, 0x0], "0x0000")]
    #[case::zeros_four_bytes(vec![0x0, 0x0, 0x0, 0x0], "0x00000000")]
    fn bytes_vec_can_be_converted_to_hex_string(#[case] v: Vec<u8>, #[case] expected: &str) {
        assert_eq!(v.to_hex(), expected);
    }

    #[rstest::rstest]
    #[case::even_length("0x00", 0u64)]
    #[case::odd_length("0x0", 0u64)]
    #[case::without_prefix("10", 16u64)]
    #[case::max("0xffffffffffffffff", u64::MAX)]
    fn u64_can_be_constructed_from_hex_string(#[case] input: &str, #[case] expected: u64) {
        let n = u64::try_from_hex(input).unwrap();
        assert_eq!(n, expected);
    }

    #[rstest::rstest]
    #[case::empty(
        "0x",
        ParseHexError::IntError(IntErrorKind::Empty),
        Some(
            "hex string cannot be represented as a number of the target type: IntErrorKind::Empty"
        )
    )]
    #[case::invalid_chars_with_prefix(
        "0xxyz",
        ParseHexError::InvalidCharacter,
        Some("hex string contains invalid character(s)")
    )]
    #[case::invalid_chars_without_prefix(
        "xyz",
        ParseHexError::InvalidCharacter,
        Some("hex string contains invalid character(s)")
    )]
    #[case::overflow(
        "10000000000000000",
        ParseHexError::IntError(IntErrorKind::PosOverflow),
        None
    )]
    fn u64_from_malformed_hex_string_produces_error(
        #[case] input: &str,
        #[case] expected_err: ParseHexError,
        #[case] expected_msg: Option<&str>,
    ) {
        let err = u64::try_from_hex(input).unwrap_err();
        assert_eq!(err, expected_err);
        if let Some(msg) = expected_msg {
            assert_eq!(err.to_string(), msg);
        }
    }

    #[rstest::rstest]
    #[case::min(u64::MIN, "0x0")]
    #[case::ten(10u64, "0xa")]
    #[case::sixteen(16u64, "0x10")]
    #[case::two_fifty_five(255u64, "0xff")]
    #[case::two_fifty_six(256u64, "0x100")]
    #[case::max(u64::MAX, "0xffffffffffffffff")]
    fn u64_can_be_converted_to_hex_string(#[case] n: u64, #[case] expected: &str) {
        assert_eq!(n.to_hex(), expected);
    }

    #[rstest::rstest]
    #[case::even_length("0x00", 0u8)]
    #[case::odd_length("0x0", 0u8)]
    #[case::without_prefix("10", 16u8)]
    #[case::max("0xff", u8::MAX)]
    fn u8_can_be_constructed_from_hex_string(#[case] input: &str, #[case] expected: u8) {
        let n = u8::try_from_hex(input).unwrap();
        assert_eq!(n, expected);
    }

    #[rstest::rstest]
    #[case::empty(
        "0x",
        ParseHexError::IntError(IntErrorKind::Empty),
        Some(
            "hex string cannot be represented as a number of the target type: IntErrorKind::Empty"
        )
    )]
    #[case::invalid_chars_without_prefix(
        "xy",
        ParseHexError::InvalidCharacter,
        Some("hex string contains invalid character(s)")
    )]
    #[case::invalid_chars_with_prefix(
        "0xxy",
        ParseHexError::InvalidCharacter,
        Some("hex string contains invalid character(s)")
    )]
    #[case::overflow("100", ParseHexError::IntError(IntErrorKind::PosOverflow), None)]
    fn u8_from_malformed_hex_string_produces_error(
        #[case] input: &str,
        #[case] expected_err: ParseHexError,
        #[case] expected_msg: Option<&str>,
    ) {
        let n = u8::try_from_hex(input).unwrap_err();
        assert_eq!(n, expected_err);
        if let Some(msg) = expected_msg {
            assert_eq!(n.to_string(), msg);
        }
    }

    #[rstest::rstest]
    #[case::min(u8::MIN, "0x0")]
    #[case::ten(10u8, "0xa")]
    #[case::sixteen(16u8, "0x10")]
    #[case::max(u8::MAX, "0xff")]
    fn u8_can_be_converted_to_hex_string(#[case] n: u8, #[case] expected: &str) {
        assert_eq!(n.to_hex(), expected);
    }
}
