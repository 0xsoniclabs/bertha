use alloy_rlp::{BufMut, Encodable, Header};
use serde::{Deserialize, Serialize};

use crate::{
    AsHex, Bloom, Eip2718Marshallable, Hash, HexConvert, Log, RlpString, TransactionType,
    parse_hex_error::ParseHexError,
};

/// Receipt for a transaction.
/// The receipt provides information about the execution of the transaction like the amount of gas
/// that was used or the emitted logs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(from = "JsonRpcTransactionReceipt", into = "JsonRpcTransactionReceipt")]
pub struct TransactionReceipt {
    pub transaction_type: TransactionType,
    pub post_state_or_status: PostStateOrStatus,
    pub cumulative_gas_used: u64,
    pub logs: Vec<Log>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum PostStateOrStatus {
    PostState(Hash),
    Status(u64),
}

impl Default for PostStateOrStatus {
    fn default() -> Self {
        PostStateOrStatus::Status(0)
    }
}

impl HexConvert for PostStateOrStatus {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        // The custom implementation is needed because the status is encoded as a hex string of odd
        // length.
        match value {
            "0x0" => Ok(PostStateOrStatus::Status(0)),
            "0x1" => Ok(PostStateOrStatus::Status(1)),
            _ => <[u8; 32]>::try_from_hex(value).map(PostStateOrStatus::PostState),
        }
    }

    fn to_hex(&self) -> String {
        match self {
            PostStateOrStatus::PostState(root) => root.to_hex(),
            PostStateOrStatus::Status(status) => status.to_hex(),
        }
    }
}

impl TransactionReceipt {
    pub fn logs_bloom(&self) -> Bloom {
        let mut bloom = ethbloom::Bloom([0; 256]);
        for log in &self.logs {
            bloom.accrue(ethbloom::Input::Raw(&log.address));
            for topic in &log.topics {
                bloom.accrue(ethbloom::Input::Raw(topic));
            }
        }
        bloom.0
    }
}

impl Default for TransactionReceipt {
    fn default() -> Self {
        Self {
            transaction_type: TransactionType::Legacy,
            post_state_or_status: PostStateOrStatus::default(),
            cumulative_gas_used: u64::default(),
            logs: Vec::default(),
        }
    }
}

pub const RECEIPT_STATUS_SUCCESS_RLP: &[u8] = &[0x01];
pub const RECEIPT_STATUS_FAILED_RLP: &[u8] = &[];

impl Eip2718Marshallable for TransactionReceipt {
    fn marshal(&self) -> Vec<u8> {
        let post_state_or_status = match self.post_state_or_status {
            PostStateOrStatus::Status(1) => RlpString(RECEIPT_STATUS_SUCCESS_RLP.to_vec()),
            PostStateOrStatus::Status(_) => RlpString(RECEIPT_STATUS_FAILED_RLP.to_vec()),
            PostStateOrStatus::PostState(post_state) => RlpString(post_state.to_vec()),
        };
        let mut out = Vec::new();
        if self.transaction_type != TransactionType::Legacy {
            out.put_u8(self.transaction_type as u8);
        }
        Header {
            list: true,
            payload_length: post_state_or_status.length()
                + self.cumulative_gas_used.length()
                + self.logs_bloom().length()
                + self.logs.length(),
        }
        .encode(&mut out);
        post_state_or_status.encode(&mut out);
        self.cumulative_gas_used.encode(&mut out);
        self.logs_bloom().encode(&mut out);
        self.logs.encode(&mut out);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcTransactionReceipt {
    #[serde(rename = "type")]
    pub transaction_type: AsHex<TransactionType>,
    pub status: AsHex<u64>,
    #[serde(rename = "root")]
    pub post_state: Option<AsHex<Vec<u8>>>,
    pub cumulative_gas_used: AsHex<u64>,
    pub logs_bloom: AsHex<Bloom>,
    pub logs: Vec<Log>,
    // Fields that are part of the JSON RPC response but we currently don't use:
    // pub block_hash: AsHex<Hash>,
    // pub block_number: AsHex<u64>,
    // pub contract_address: Option<AsHex<Address>>,
    // pub effective_gas_price: Option<AsHex<u64>>,
    // pub from: AsHex<Address>,
    // pub gas_used: AsHex<u64>,
    // pub to: Option<AsHex<Address>>,
    // pub transaction_hash: AsHex<Hash>,
    // pub transaction_index: AsHex<u64>,
}

impl From<JsonRpcTransactionReceipt> for TransactionReceipt {
    fn from(value: JsonRpcTransactionReceipt) -> Self {
        let post_state_or_status = match (value.post_state, value.status) {
            (Some(post_state), _) if post_state.0.len() == 32 => {
                PostStateOrStatus::PostState(post_state.0.try_into().unwrap())
            }
            (_, status) => PostStateOrStatus::Status(status.0),
        };
        Self {
            transaction_type: value.transaction_type.0,
            post_state_or_status,
            cumulative_gas_used: value.cumulative_gas_used.0,
            logs: value.logs,
        }
    }
}

impl From<TransactionReceipt> for JsonRpcTransactionReceipt {
    fn from(value: TransactionReceipt) -> Self {
        let logs_bloom = value.logs_bloom();
        let (post_state, status) = match value.post_state_or_status {
            PostStateOrStatus::PostState(root) => (Some(AsHex(root.to_vec())), AsHex(0u64)),
            PostStateOrStatus::Status(status) => (None, AsHex(status)),
        };
        Self {
            transaction_type: AsHex(value.transaction_type),
            post_state,
            status,
            cumulative_gas_used: AsHex(value.cumulative_gas_used),
            logs_bloom: AsHex(logs_bloom),
            logs: value.logs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Hash, HexConvert,
        test_data::test_data_receipts::{RECEIPTS_ROOT, generate_receipts_with_data},
        verify,
    };

    #[test]
    fn logs_bloom_is_computed_correctly() {
        for receipt_data in generate_receipts_with_data() {
            let receipt = receipt_data.receipt;
            let computed_bloom = receipt.logs_bloom();
            assert_eq!(computed_bloom, receipt_data.bloom);
        }
    }

    #[test]
    fn can_be_encoded_to_rlp() {
        for value in generate_receipts_with_data() {
            let rlp_bytes = value.receipt.marshal();
            assert_eq!(
                rlp_bytes, value.rlp_encoding,
                "Encoded RLP should match the expected value"
            );
        }
    }

    #[test]
    fn can_be_serialized_and_deserialized_from_json() {
        for value in generate_receipts_with_data() {
            let serialized = serde_json::to_value(&value.receipt)
                .expect("Serialization to JSON should not fail");
            let expected_value = serde_json::to_value(
                serde_json::from_str::<TransactionReceipt>(&value.json_representation)
                    .expect("Deserialization from JSON should not fail"),
            )
            .unwrap();
            assert_eq!(
                serialized, expected_value,
                "Serialized JSON should match the expected value"
            );
        }
    }

    #[test]
    fn can_be_verified() {
        let receipts = generate_receipts_with_data()
            .into_iter()
            .map(|value| value.receipt)
            .collect::<Vec<_>>();
        let root = Hash::try_from_hex(RECEIPTS_ROOT).expect("Invalid receipts root hex");
        assert!(
            verify(&receipts, &root).is_ok(),
            "Receipts verification should succeed"
        );
    }
}
