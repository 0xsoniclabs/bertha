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

use std::num::IntErrorKind;

use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Error)]
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

    #[error("custom: {0}")]
    Custom(String),
}

impl From<bnum::errors::ParseIntError> for ParseHexError {
    fn from(value: bnum::errors::ParseIntError) -> Self {
        match value.kind() {
            IntErrorKind::InvalidDigit => Self::InvalidCharacter,
            _ => Self::IntError(*value.kind()),
        }
    }
}

impl From<std::num::ParseIntError> for ParseHexError {
    fn from(value: std::num::ParseIntError) -> Self {
        match value.kind() {
            IntErrorKind::InvalidDigit => Self::InvalidCharacter,
            _ => Self::IntError(*value.kind()),
        }
    }
}
