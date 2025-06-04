use std::{
    fmt,
    num::IntErrorKind,
    ops::{Add, Sub},
};

use alloy_rlp::{RlpDecodableWrapper, RlpEncodableWrapper};
use serde::{Deserialize, Serialize};

use crate::{U256, parse_hex_error::ParseHexError};

/// Wei is an unsigned 256 bit integer that represents amount of Wei, the smallest denomination of
/// Ether in the Ethereum blockchain.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    RlpDecodableWrapper,
    RlpEncodableWrapper,
)]
pub struct Wei(U256);

impl Wei {
    pub const ZERO: Self = Self(U256::ZERO);

    pub fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        U256::try_from_hex(value).map(Self)
    }

    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

impl<T> From<T> for Wei
where
    U256: From<T>,
{
    fn from(value: T) -> Self {
        Wei(U256::from(value))
    }
}

impl fmt::Display for Wei {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for Wei {
    type Output = Result<Wei, IntErrorKind>;

    fn add(self, rhs: Self) -> Self::Output {
        (self.0 + rhs.0).map(Wei)
    }
}

impl Sub for Wei {
    type Output = Result<Wei, IntErrorKind>;

    fn sub(self, rhs: Self) -> Self::Output {
        (self.0 - rhs.0).map(Wei)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_initializes_balance_to_zero() {
        let balance = Wei::ZERO;
        assert_eq!(balance.0, U256::from(0u32));
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let hex_string = "0xaff";
        let balance = Wei::try_from_hex(hex_string).expect("Failed to create Wei");

        assert_eq!(balance.to_hex(), "0xaff");
    }

    #[test]
    fn from_hex_returns_error_on_invalid_input() {
        // Invalid hex string
        let invalid_hex_string = "0xg";
        let result = Wei::try_from_hex(invalid_hex_string);
        assert!(result.is_err());
    }

    #[test]
    fn can_be_constructed_from_unsigned_integer() {
        let balance = Wei::from(10u32);
        assert_eq!(balance.0, U256::from(10u32));
    }

    #[test]
    fn balances_can_be_added() {
        let balance_1 = Wei::from(10u32);
        let balance_2 = Wei::from(10u32);
        let res = (balance_1 + balance_2).unwrap();
        assert_eq!(res.0, U256::from(20u32));
    }

    #[test]
    fn add_produces_an_error_on_overflow() {
        let balance_1 = Wei::from(U256::MAX);
        let balance_2 = Wei::from(1u32);
        let res = balance_1 + balance_2;
        assert_eq!(res, Err(IntErrorKind::PosOverflow));
    }

    #[test]
    fn sub_two_balances() {
        let balance_1 = Wei::from(20u32);
        let balance_2 = Wei::from(10u32);
        let res = (balance_1 - balance_2).unwrap();
        assert_eq!(res.0, U256::from(10u32));
    }

    #[test]
    fn sub_produces_an_error_on_underflow() {
        let balance_1 = Wei::from(1u32);
        let balance_2 = Wei::from(2u32);
        let res = balance_1 - balance_2;
        assert_eq!(res, Err(IntErrorKind::NegOverflow));
    }

    #[test]
    fn format_display_uses_decimal_representation() {
        let balance = Wei::from(10u32);
        assert_eq!(format!("{balance}",), "10");
    }
}
