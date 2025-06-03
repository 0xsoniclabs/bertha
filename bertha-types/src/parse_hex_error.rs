use std::num::IntErrorKind;

use thiserror::Error;

#[derive(PartialEq, Eq, Error, Debug)]
pub enum ParseHexError {
    #[error("hex string contains invalid character(s)")]
    InvalidCharacter,

    #[error("hex string cannot be represented as a number of the target type: IntErrorKind::{0:?}")]
    IntError(IntErrorKind),

    // Not all types emit this error: It is allowed to represent u256 using odd-length hex
    // strings, whereas Blob requires an exact, even number of digits. This follows the
    // conventions used by the JSON RPC API.
    #[error("hex string has an odd number of digits, which is not supported by the target type")]
    OddLength,

    #[error("hex string is required to have a length of exactly {0} bytes, but has {1} bytes")]
    FixedSizeMismatch(usize, usize),
}

impl From<bnum::errors::ParseIntError> for ParseHexError {
    fn from(value: bnum::errors::ParseIntError) -> Self {
        match value.kind() {
            IntErrorKind::InvalidDigit => Self::InvalidCharacter,
            _ => Self::IntError(value.kind().clone()),
        }
    }
}

impl From<std::num::ParseIntError> for ParseHexError {
    fn from(value: std::num::ParseIntError) -> Self {
        match value.kind() {
            IntErrorKind::InvalidDigit => Self::InvalidCharacter,
            _ => Self::IntError(value.kind().clone()),
        }
    }
}
