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

impl Default for TransactionReceipt {
    fn default() -> Self {
        Self {
            transaction_type: TransactionType::Legacy,
            status: u64::default(),
            cumulative_gas_used: u64::default(),
            logs: Vec::default(),
        }
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
    use crate::{Address, Hash, HexConvert, verify};

    #[test]

    fn encode_value_encodes_transaction_receipt_in_rlp_and_respects_different_encoding_depending_on_type()
     {
        let mut receipt = TransactionReceipt {
            status: 1,
            cumulative_gas_used: 21000,
            logs: vec![],
            transaction_type: TransactionType::Legacy,
        };

        // if the type == legacy transaction type (0) -> type field not encoded
        assert_eq!(receipt.marshal(), Vec::try_from_hex("0xf9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap());

        // if the type != legacy transaction type (0) -> type field encoded
        receipt.transaction_type = TransactionType::AccessList;
        assert_eq!(receipt.marshal(), Vec::try_from_hex("0x01f9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap());

        // if the type != legacy transaction type (0) -> type field encoded
        receipt.transaction_type = TransactionType::DynamicFee;
        assert_eq!(receipt.marshal(), Vec::try_from_hex("0x02f9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap());
    }

    fn get_transaction_receipts() -> Vec<TransactionReceipt> {
        vec![
            TransactionReceipt {
                cumulative_gas_used: 77081,
                logs: vec![
                    Log {
                        address: Address::try_from_hex(
                            "0x5a91d3042b71a92f6757fa937763d03cc65ed8bc",
                        )
                        .unwrap(),
                        topics: vec![
                            Hash::try_from_hex("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef").unwrap(),
                            Hash::try_from_hex("0x000000000000000000000000234172247c94723a2a439bcd685c1699d54a95b0").unwrap(),
                            Hash::try_from_hex("0x00000000000000000000000099a35a6301043a02add89b613a58eee2c46750a2").unwrap(),
                        ],
                        data: Vec::try_from_hex("0x000000000000000000000000000000000000000000000000002386f26fc10000").unwrap(),
                    },
                    Log {
                        address: Address::try_from_hex("61a2777db1271ef53329a13d05098f47ceaa7021")
                            .unwrap(),
                        topics: vec![
                            Hash::try_from_hex("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef").unwrap(),
                            Hash::try_from_hex("0x00000000000000000000000099a35a6301043a02add89b613a58eee2c46750a2").unwrap(),
                            Hash::try_from_hex("0x000000000000000000000000234172247c94723a2a439bcd685c1699d54a95b0").unwrap(),
                        ],
                        data: Vec::try_from_hex("0x0000000000000000000000000000000000000000000000000584dea1b5674d10").unwrap(),
                    },
                    Log {
                        address: Address::try_from_hex(
                            "0x99a35a6301043a02add89b613a58eee2c46750a2",
                        )
                        .unwrap(),
                        topics: vec![
                            Hash::try_from_hex("0xcd3829a3813dc3cdd188fd3d01dcf3268c16be2fdd2dd21d0665418816e46062").unwrap(),
                            Hash::try_from_hex("0x000000000000000000000000234172247c94723a2a439bcd685c1699d54a95b0").unwrap(),
                            Hash::try_from_hex("0x0000000000000000000000005a91d3042b71a92f6757fa937763d03cc65ed8bc").unwrap(),
                            Hash::try_from_hex("0x00000000000000000000000061a2777db1271ef53329a13d05098f47ceaa7021").unwrap(),
                        ],
                        data: Vec::try_from_hex("0x000000000000000000000000000000000000000000000000002386f26fc100000000000000000000000000000000000000000000000000000584dea1b5674d10").unwrap(),
                    },
                ],
                status: 1,
                transaction_type: TransactionType::DynamicFee,
            },
            TransactionReceipt {
                cumulative_gas_used: 98081,
                logs: vec![],
                status: 1,
                transaction_type: TransactionType::DynamicFee,
            },
        ]
    }

    #[test]
    fn logs_bloom_is_computed_correctly() {
        let receipts = get_transaction_receipts();

        let expected = Bloom::try_from_hex("0x00100000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000004000000000002000000008040000000000000000040000000000000000000001000000000000002000000000000000000020000000010000000810000000000000000000000000000000000000000000800080000040000080000000000000000400000000000008000000000000800000020000000000000000000000000000000003040800000000000000000000000000000000400000000000000000000000000000000000080000000000000000000000000000000000000000000000").unwrap();
        let received = receipts[0].logs_bloom();
        assert_eq!(received, expected);

        let expected = [0; 256];
        let received = receipts[1].logs_bloom();
        assert_eq!(received, expected);
    }

    #[test]
    fn block_receipt_verify_computes_root_hash_correctly_and_compares_it_with_specified_root() {
        let receipts = get_transaction_receipts();

        // Receipts hash fetched from the SONIC chain
        let receipts_root = Hash::try_from_hex(
            "0x158c87d05e49fa970a24cee4d209ff36c7cf1f3ac30a98175582beb82f44f8b3",
        )
        .unwrap();
        assert!(verify(&receipts, &receipts_root).is_ok());

        let receipts_root = Hash::default();
        assert!(verify(&receipts, &receipts_root).is_err());
    }

    #[test]
    fn can_be_serialized_to_json() {
        let address_hex = "0x1234567890abcdef1234567890abcdef12345678";
        let topic1_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";
        let topic2_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";
        let bloom_hex = "0x00000000000000000000800000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

        let log = Log {
            address: Address::try_from_hex(address_hex).unwrap(),
            topics: vec![
                Hash::try_from_hex(topic1_hex).unwrap(),
                Hash::try_from_hex(topic2_hex).unwrap(),
            ],
            data: vec![1, 2, 3, 4, 5],
        };

        let receipt = TransactionReceipt {
            cumulative_gas_used: 12345,
            logs: vec![log.clone()],
            status: 1,
            transaction_type: TransactionType::DynamicFee,
        };

        let expected_json = format!(
            r#"{{
            "type":"0x2",
            "status":"0x1",
            "cumulativeGasUsed":"0x3039",
            "logsBloom":"{bloom_hex}",
            "logs":[{{
                "address":"{address_hex}",
                "topics":["{topic1_hex}","{topic2_hex}"],
                "data":"0x0102030405"
            }}]
            }}"#,
        )
        .replace(" ", "")
        .replace("\n", "");

        assert_eq!(serde_json::to_string(&receipt).unwrap(), expected_json);
    }

    #[test]
    fn can_be_deserialized_from_json() {
        let address_hex = "0x1234567890abcdef1234567890abcdef12345678";
        let topic1_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";
        let topic2_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";
        let bloom_hex = "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

        let json = format!(
            r#"{{
            "cumulativeGasUsed":"0x3039",
            "logs":[{{
                "address":"{address_hex}",
                "topics":["{topic1_hex}","{topic2_hex}"],
                "data":"0x0102030405"
            }}],
            "logsBloom":"{bloom_hex}",
            "status":"0x1",
            "transactionIndex":"0x0",
            "type":"0x2"
            }}"#,
        );

        let expected_log = Log {
            address: Address::try_from_hex(address_hex).unwrap(),
            topics: vec![
                Hash::try_from_hex(topic1_hex).unwrap(),
                Hash::try_from_hex(topic2_hex).unwrap(),
            ],
            data: vec![1, 2, 3, 4, 5],
        };

        let expected_receipt = TransactionReceipt {
            cumulative_gas_used: 12345,
            logs: vec![expected_log.clone()],
            status: 1,
            transaction_type: TransactionType::DynamicFee,
        };

        assert_eq!(
            serde_json::from_str::<TransactionReceipt>(&json).unwrap(),
            expected_receipt
        );
    }
}
