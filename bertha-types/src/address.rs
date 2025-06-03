use std::fmt;

use alloy_rlp::Encodable;
use serde::{Deserialize, Serialize};

use crate::types::{SerializableByteArray, parse_hex_error::ParseHexError};

/// Address is a 20-byte identifier used in Ethereum-compatible blockchains to identify smart
/// contracts and externally owned accounts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Address(SerializableByteArray<20>);

impl Address {
    /// Constructs a new Address from an hex string.
    /// The hex string can be prefixed with "0x" or not.
    pub fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        SerializableByteArray::<20>::try_from_hex(value).map(Self)
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        self.0.as_bytes()
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Encodable for Address {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.0.encode(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_be_constructed_from_hex_string() {
        let addr = Address::try_from_hex("0x00112233445566778899aabbccddeeff00112233").unwrap();
        assert_eq!(
            addr.to_string(),
            "0x00112233445566778899aabbccddeeff00112233"
        );
    }

    #[test]
    fn from_hex_returns_error_on_invalid_input() {
        let invalid_hex_string = "0xg";
        let addr = Address::try_from_hex(invalid_hex_string);
        assert!(addr.is_err());
    }

    #[test]
    fn can_be_serialized_using_rlp() {
        let address_string = "0x00112233445566778899aabbccddeeff00112233";

        // The expected RLP encoding of the address
        let res: [u8; 20] =
            const_hex::decode_to_array(address_string.trim_start_matches("0x")).unwrap();
        let mut ref_rlp = Vec::new();
        res.encode(&mut ref_rlp);

        // Construct the address and encode it
        let addr = Address::try_from_hex(address_string).unwrap();
        let mut buf = Vec::new();
        addr.encode(&mut buf);
        assert_eq!(buf, ref_rlp);
    }

    #[test]
    fn can_be_printed() {
        let addr = Address::try_from_hex("0xffeeccddeeff00112233445566778899aabbccdd").unwrap();
        assert_eq!(
            format!("{addr}"),
            "0xffeeccddeeff00112233445566778899aabbccdd"
        );
    }
}
