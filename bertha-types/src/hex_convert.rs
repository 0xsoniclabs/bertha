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

    #[test]
    fn byte_vec_can_be_constructed_from_hex_string() {
        let v = Vec::try_from_hex("0x1234").unwrap();
        assert_eq!(v, [0x12, 0x34]);
        let v = Vec::try_from_hex("0x").unwrap();
        assert_eq!(v, [0x0; 0]);
        let v = Vec::try_from_hex("").unwrap();
        assert_eq!(v, [0x0; 0]);
    }

    #[test]
    fn bytes_vec_from_malformed_hex_string_produces_error() {
        let err = Vec::try_from_hex("xyzw").unwrap_err();
        assert_eq!(err, ParseHexError::InvalidCharacter);
        let err = Vec::try_from_hex("0xxyzw").unwrap_err();
        assert_eq!(err, ParseHexError::InvalidCharacter);
        let err = Vec::try_from_hex("0x1").unwrap_err();
        assert_eq!(err, ParseHexError::OddLength);
    }

    #[test]
    fn bytes_vec_can_be_converted_to_hex_string() {
        let v = vec![0x12, 0x34];
        assert_eq!(v.to_hex(), "0x1234");

        // String has fixed length
        let v = vec![0x0, 0x0];
        assert_eq!(v.to_hex(), "0x0000");
        let v = vec![0x0, 0x0, 0x0, 0x0];
        assert_eq!(v.to_hex(), "0x00000000");
    }

    #[test]
    fn u64_can_be_constructed_from_hex_string() {
        // Even-length hex string
        let n = u64::try_from_hex("0x00").unwrap();
        assert_eq!(n, 0u64);

        // Odd-length hex string
        let n = u64::try_from_hex("0x0").unwrap();
        assert_eq!(n, 0u64);

        // Without 0x prefix
        let n = u64::try_from_hex("10").unwrap();
        assert_eq!(n, 16u64);

        // u64::MAX
        let n = u64::try_from_hex("0xffffffffffffffff").unwrap();
        assert_eq!(n, u64::MAX);
    }

    #[test]
    fn u64_from_malformed_hex_string_produces_error() {
        let n = u64::try_from_hex("0x").unwrap_err();
        assert_eq!(n, ParseHexError::IntError(IntErrorKind::Empty));
        assert_eq!(
            n.to_string(),
            "hex string cannot be represented as a number of the target type: IntErrorKind::Empty"
        );

        let n = u64::try_from_hex("xyz").unwrap_err();
        assert_eq!(n, ParseHexError::InvalidCharacter);
        assert_eq!(n.to_string(), "hex string contains invalid character(s)");

        let n = u64::try_from_hex("0xxyz").unwrap_err();
        assert_eq!(n, ParseHexError::InvalidCharacter);
        assert_eq!(n.to_string(), "hex string contains invalid character(s)");

        let n = u64::try_from_hex(
            const_hex::encode((u128::from(u64::MAX) + 1u128).to_be_bytes()).as_str(),
        )
        .unwrap_err();
        assert_eq!(
            n,
            crate::parse_hex_error::ParseHexError::IntError(IntErrorKind::PosOverflow)
        );
    }

    #[test]
    fn u64_can_be_converted_to_hex_string() {
        assert_eq!(u64::MIN.to_hex(), "0x0");
        assert_eq!(10u64.to_hex(), "0xa");
        assert_eq!(16u64.to_hex(), "0x10");
        assert_eq!(255u64.to_hex(), "0xff");
        assert_eq!(256u64.to_hex(), "0x100");
        assert_eq!(u64::MAX.to_hex(), "0xffffffffffffffff");
    }

    #[test]
    fn u8_can_be_constructed_from_hex_string() {
        // Even-length hex string
        let n = u8::try_from_hex("0x00").unwrap();
        assert_eq!(n, 0u8);

        // Odd-length hex string
        let n = u8::try_from_hex("0x0").unwrap();
        assert_eq!(n, 0u8);

        // Without 0x prefix
        let n = u8::try_from_hex("10").unwrap();
        assert_eq!(n, 16u8);

        // u8::MAX
        let n = u8::try_from_hex("0xff").unwrap();
        assert_eq!(n, u8::MAX);
    }

    #[test]
    fn u8_from_malformed_hex_string_produces_error() {
        let n = u8::try_from_hex("0x").unwrap_err();
        assert_eq!(n, ParseHexError::IntError(IntErrorKind::Empty));
        assert_eq!(
            n.to_string(),
            "hex string cannot be represented as a number of the target type: IntErrorKind::Empty"
        );

        let n = u8::try_from_hex("xyz").unwrap_err();
        assert_eq!(n, ParseHexError::InvalidCharacter);
        assert_eq!(n.to_string(), "hex string contains invalid character(s)");

        let n = u8::try_from_hex("0xxyz").unwrap_err();
        assert_eq!(n, ParseHexError::InvalidCharacter);
        assert_eq!(n.to_string(), "hex string contains invalid character(s)");

        let n =
            u8::try_from_hex(const_hex::encode((u16::from(u8::MAX) + 1u16).to_be_bytes()).as_str())
                .unwrap_err();
        assert_eq!(
            n,
            crate::parse_hex_error::ParseHexError::IntError(IntErrorKind::PosOverflow)
        );
    }

    #[test]
    fn u8_can_be_converted_to_hex_string() {
        assert_eq!(u8::MIN.to_hex(), "0x0");
        assert_eq!(10u8.to_hex(), "0xa");
        assert_eq!(16u8.to_hex(), "0x10");
        assert_eq!(u8::MAX.to_hex(), "0xff");
    }
}
