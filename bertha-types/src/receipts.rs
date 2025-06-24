use alloy_rlp::{BufMut, Encodable, Header};
use serde::{Deserialize, Serialize};

use crate::{AsHex, Bloom, Eip2718Marshallable, Log, TransactionType};

/// Receipt for a transaction.
/// The receipt provides information about the execution of the transaction like the amount of gas
/// that was used or the emitted logs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(from = "JsonRpcTransactionReceipt", into = "JsonRpcTransactionReceipt")]
pub struct TransactionReceipt {
    pub transaction_type: TransactionType,
    pub status: u64,
    pub cumulative_gas_used: u64,
    pub logs: Vec<Log>,
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

impl Eip2718Marshallable for TransactionReceipt {
    fn marshal(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.transaction_type != TransactionType::Legacy {
            out.put_u8(self.transaction_type as u8);
        }
        Header {
            list: true,
            payload_length: self.status.length()
                + self.cumulative_gas_used.length()
                + self.logs_bloom().length()
                + self.logs.length(),
        }
        .encode(&mut out);
        self.status.encode(&mut out);
        self.cumulative_gas_used.encode(&mut out);
        self.logs_bloom().encode(&mut out);
        self.logs.encode(&mut out);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcTransactionReceipt {
    #[serde(rename = "type")]
    pub transaction_type: AsHex<TransactionType>,
    pub status: AsHex<u64>,
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
        Self {
            transaction_type: value.transaction_type.0,
            status: value.status.0,
            cumulative_gas_used: value.cumulative_gas_used.0,
            logs: value.logs,
        }
    }
}

impl From<TransactionReceipt> for JsonRpcTransactionReceipt {
    fn from(value: TransactionReceipt) -> Self {
        Self {
            transaction_type: AsHex(value.transaction_type),
            status: AsHex(value.status),
            cumulative_gas_used: AsHex(value.cumulative_gas_used),
            logs_bloom: AsHex(value.logs_bloom()),
            logs: value.logs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Hash, HexConvert,
        test_data::test_data_receipts::tests::receipt::{
            RECEIPTS_ROOT, generate_receipts_with_data,
        },
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
