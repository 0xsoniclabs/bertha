use std::{fmt::Display, str::FromStr};

use serde::Serialize;

use crate::types::{BlockNumber, Hash};

/// An unique identifier of a block in the blockchain.
/// It can be either the latest confirmed block, a block number or a block hash.
/// When parsed from a string, latest is represented as `latest`,
/// block number as decimal number, and block hash as hex string prefixed with `0x`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockIdentifier {
    Latest,
    #[serde(untagged)]
    Number(BlockNumber),
    #[serde(untagged)]
    Hash(Hash),
}

impl Display for BlockIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockIdentifier::Latest => write!(f, "latest"),
            BlockIdentifier::Number(number) => write!(f, "{number}"),
            BlockIdentifier::Hash(hash) => write!(f, "{hash}"),
        }
    }
}

impl FromStr for BlockIdentifier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "latest" {
            Ok(BlockIdentifier::Latest)
        } else if s.starts_with("0x") {
            Ok(BlockIdentifier::Hash(
                Hash::try_from_hex(s).map_err(|r| format!("Invalid block hash: {s}\n{r}"))?,
            ))
        } else {
            Ok(BlockIdentifier::Number(BlockNumber::from(
                u64::from_str(s).map_err(|r| format!("Invalid block number: {s}\n{r}"))?,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_returns_tag_or_dec_number_or_hex_hash() {
        assert_eq!(BlockIdentifier::Latest.to_string(), "latest");

        assert_eq!(BlockIdentifier::Number(10_u8.into()).to_string(), "10");

        assert_eq!(
            BlockIdentifier::Hash(
                Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000001"
                )
                .unwrap()
            )
            .to_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn from_str_parses_tag_or_dec_number_or_hex_hash() {
        assert_eq!(
            BlockIdentifier::from_str("latest").unwrap(),
            BlockIdentifier::Latest
        );
        assert_eq!(
            BlockIdentifier::from_str("10").unwrap(),
            BlockIdentifier::Number(10_u8.into())
        );
        assert_eq!(
            BlockIdentifier::from_str(
                "0x0000000000000000000000000000000000000000000000000000000000000001"
            )
            .unwrap(),
            BlockIdentifier::Hash(
                Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000001"
                )
                .unwrap()
            )
        );
    }

    #[test]
    fn json_serialization_returns_tag_or_hex_number_or_hex_hash() {
        assert_eq!(
            serde_json::to_string(&BlockIdentifier::Latest).unwrap(),
            "\"latest\""
        );

        assert_eq!(
            serde_json::to_string(&BlockIdentifier::Number(10_u8.into())).unwrap(),
            "\"0xa\""
        );

        assert_eq!(
            serde_json::to_string(&BlockIdentifier::Hash(
                Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000001"
                )
                .unwrap()
            ))
            .unwrap(),
            "\"0x0000000000000000000000000000000000000000000000000000000000000001\""
        );
    }
}
