use alloy_rlp::{BufMut, Encodable, Header};
use alloy_trie::{HashBuilder, Nibbles};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Address, AsHex, Bloom, Hash, Log};

#[derive(Debug, Clone, Default, PartialEq, Eq, Error)]
#[error("the computed receipt root did not match the receipt root of the block header")]
pub struct ReceiptVerificationError;

/// Receipt for a transaction.
/// The receipt provides information about the execution of the transaction like the amount of gas
/// that was used or the emitted logs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(from = "JsonRpcTransactionReceipt", into = "JsonRpcTransactionReceipt")]
pub struct TransactionReceipt {
    pub type_: u64,
    pub status: u64,
    pub cumulative_gas_used: u64,
    pub logs: Vec<Log>,

    pub transaction_index: u64,
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
        Bloom::from(bloom.0)
    }
}

impl Encodable for TransactionReceipt {
    fn length(&self) -> usize {
        let payload_length = self.rlp_payload_length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }

    fn encode(&self, out: &mut dyn BufMut) {
        // see: https://github.com/ethereum/go-ethereum/blob/a511553e448c947a0fe8f34acf7bb6f9818c2b49/core/types/receipt.go#L122-L140
        const LEGACY_TRANSACTION_TYPE: u8 = 0;
        if self.type_ != LEGACY_TRANSACTION_TYPE as u64 {
            out.put_u8(self.type_ as u8);
        }
        Header {
            list: true,
            payload_length: self.rlp_payload_length(),
        }
        .encode(out);
        self.status.encode(out);
        self.cumulative_gas_used.encode(out);
        self.logs_bloom().encode(out);
        self.logs.encode(out);
    }
}

impl TransactionReceipt {
    fn rlp_payload_length(&self) -> usize {
        self.status.length()
            + self.cumulative_gas_used.length()
            + self.logs_bloom().length()
            + self.logs.length()
    }

    fn encode_key(&self) -> Vec<u8> {
        let mut v = Vec::new();
        self.transaction_index.encode(&mut v);
        v
    }

    fn encode_value(&self) -> Vec<u8> {
        let mut v = Vec::new();
        self.encode(&mut v);
        v
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcTransactionReceipt {
    pub type_: AsHex<u64>,
    pub status: AsHex<u64>,
    pub cumulative_gas_used: AsHex<u64>,
    pub logs_bloom: AsHex<Bloom>,
    pub logs: Vec<Log>,

    pub transaction_index: AsHex<u64>,
    // Fields that are part of the JSON RPC response but we currently don't use:
    // pub block_hash: AsHex<Hash>,
    // pub block_number: AsHex<u64>,
    // pub contract_address: Option<AsHex<Address>>,
    // pub effective_gas_price: Option<AsHex<u64>>,
    // pub from: AsHex<Address>,
    // pub gas_used: AsHex<u64>,
    // pub to: Option<AsHex<Address>>,
    // pub transaction_hash: AsHex<Hash>,
}

impl From<JsonRpcTransactionReceipt> for TransactionReceipt {
    fn from(value: JsonRpcTransactionReceipt) -> Self {
        Self {
            type_: value.type_.0,
            status: value.status.0,
            cumulative_gas_used: value.cumulative_gas_used.0,
            logs: value.logs,
            transaction_index: value.transaction_index.0,
        }
    }
}

impl From<TransactionReceipt> for JsonRpcTransactionReceipt {
    fn from(value: TransactionReceipt) -> Self {
        Self {
            type_: AsHex(value.type_),
            status: AsHex(value.status),
            cumulative_gas_used: AsHex(value.cumulative_gas_used),
            logs_bloom: AsHex(value.logs_bloom()),
            logs: value.logs,
            transaction_index: AsHex(value.transaction_index),
        }
    }
}

/// Receipts for the transactions of a block.
/// The receipts provide information about the execution of the transactions like the amount of gas
/// that was used or the emitted logs.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct BlockReceipt(Vec<TransactionReceipt>);

impl BlockReceipt {
    /// Verifies the block receipt by computing the receipts root hash and comparing it with the
    /// provided one.
    pub fn verify(
        self,
        receipts_root: &Hash,
    ) -> Result<VerifiedBlockReceipt, ReceiptVerificationError> {
        let mut trie = HashBuilder::default();
        let mut leaves: Vec<_> = self
            .0
            .iter()
            .map(|r| (Nibbles::unpack(r.encode_key()), r.encode_value()))
            .collect();
        leaves.sort_by(|l, r| l.0.cmp(&r.0));
        leaves.into_iter().for_each(|l| trie.add_leaf(l.0, &l.1));

        let root: [u8; 32] = trie.root().into();
        let root = Hash::from(root);

        if root == *receipts_root {
            Ok(VerifiedBlockReceipt(self.0))
        } else {
            Err(ReceiptVerificationError)
        }
    }
}

#[cfg(test)]
impl From<Vec<TransactionReceipt>> for BlockReceipt {
    fn from(value: Vec<TransactionReceipt>) -> Self {
        Self(value)
    }
}

/// A verified block receipt is a block receipt that has been verified against the receipts root.
/// For more information refer to [BlockReceipt].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBlockReceipt(Vec<TransactionReceipt>);

impl VerifiedBlockReceipt {
    /// Returns the logs of the block receipt that match the given address and topics.
    pub fn into_logs(self, address: Option<&Address>, topics: &[Hash]) -> Vec<Log> {
        self.0
            .into_iter()
            .flat_map(|receipt| receipt.logs)
            .filter(|log| {
                address.map(|addr| *addr == log.address).unwrap_or(true)
                    && topics.iter().all(|topic| log.topics.contains(topic))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hash, HexConvert};

    #[test]

    fn encode_key_encodes_transaction_index_in_rlp() {
        let mut receipt = TransactionReceipt {
            status: 1,
            cumulative_gas_used: 21000,
            logs: vec![],
            transaction_index: 0,
            type_: 0,
        };

        assert_eq!(receipt.encode_key(), Vec::try_from_hex("0x80").unwrap());

        receipt.transaction_index = 1;
        assert_eq!(receipt.encode_key(), Vec::try_from_hex("0x01").unwrap());
    }

    #[test]

    fn encode_value_encodes_transaction_receipt_in_rlp_and_respects_different_encoding_depending_on_type()
     {
        let mut receipt = TransactionReceipt {
            status: 1,
            cumulative_gas_used: 21000,
            logs: vec![],
            transaction_index: 0,
            type_: 0,
        };

        // if the type == legacy transaction type (0) -> type field not encoded
        assert_eq!(receipt.encode_value(), Vec::try_from_hex("0xf9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap());

        // if the type != legacy transaction type (0) -> type field encoded
        receipt.type_ = 1;
        assert_eq!(receipt.encode_value(), Vec::try_from_hex("0x01f9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap());

        // if the type != legacy transaction type (0) -> type field encoded
        receipt.type_ = 2;
        assert_eq!(receipt.encode_value(), Vec::try_from_hex("0x02f9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap());
    }

    #[test]
    fn get_logs_filters_by_all_provided_constraints() {
        let address = Address::try_from_hex("0xaf93888cbd250300470a1618206e036e11470149").unwrap();
        let topics = vec![
            Hash::try_from_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
            Hash::try_from_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000002",
            )
            .unwrap(),
            Hash::try_from_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000003",
            )
            .unwrap(),
        ];
        let log = Log {
            address,
            topics: topics.clone(),
            data: Vec::default(),
        };
        let receipt = TransactionReceipt {
            status: 0,
            cumulative_gas_used: 0,
            logs: vec![log.clone()],
            transaction_index: 0,
            type_: 0,
        };
        let block_receipt = VerifiedBlockReceipt(vec![receipt]);

        assert_eq!(
            block_receipt.clone().into_logs(None, &[]),
            vec![log.clone()]
        );
        assert_eq!(
            block_receipt.clone().into_logs(Some(&address), &[]),
            vec![log.clone()]
        );
        for topic in topics.clone() {
            assert_eq!(
                block_receipt.clone().into_logs(Some(&address), &[topic]),
                vec![log.clone()]
            );
        }
        assert_eq!(
            block_receipt.clone().into_logs(None, &topics),
            vec![log.clone()]
        );
        for topic in topics.clone() {
            assert_eq!(
                block_receipt.clone().into_logs(None, &[topic]),
                vec![log.clone()]
            );
        }
        assert_eq!(
            block_receipt.clone().into_logs(Some(&address), &topics),
            vec![log.clone()]
        );
        for topic in topics.clone() {
            assert_eq!(
                block_receipt.clone().into_logs(None, &[topic]),
                vec![log.clone()]
            );
        }
        assert_eq!(
            block_receipt
                .clone()
                .into_logs(Some(&Address::default()), &topics),
            vec![]
        );
        assert_eq!(
            block_receipt
                .clone()
                .into_logs(Some(&address), &[Hash::default()]),
            vec![]
        );
    }

    fn get_real_block_receipt() -> BlockReceipt {
        BlockReceipt(vec![
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
                transaction_index: 0,
                type_: 2,
            },
            TransactionReceipt {
                cumulative_gas_used: 98081,
                logs: vec![],
                status: 1,
                transaction_index: 1,
                type_: 2,
            },
        ])
    }

    #[test]
    fn logs_bloom_is_computed_correctly() {
        let block_receipt = get_real_block_receipt();

        let expected = Bloom::try_from_hex("0x00100000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000004000000000002000000008040000000000000000040000000000000000000001000000000000002000000000000000000020000000010000000810000000000000000000000000000000000000000000800080000040000080000000000000000400000000000008000000000000800000020000000000000000000000000000000003040800000000000000000000000000000000400000000000000000000000000000000000080000000000000000000000000000000000000000000000").unwrap();
        let received = block_receipt.0[0].logs_bloom();
        assert_eq!(received, expected);

        let expected = [0; 256];
        let received = block_receipt.0[1].logs_bloom();
        assert_eq!(received, expected);
    }

    #[test]
    fn block_receipt_verify_computes_root_hash_correctly_and_compares_it_with_specified_root() {
        let receipt = get_real_block_receipt();

        // Receipts hash fetched from the SONIC chain
        let receipts_root = Hash::try_from_hex(
            "0x158c87d05e49fa970a24cee4d209ff36c7cf1f3ac30a98175582beb82f44f8b3",
        )
        .unwrap();
        assert!(receipt.clone().verify(&receipts_root).is_ok());

        let receipts_root = Hash::default();
        assert!(receipt.verify(&receipts_root).is_err());
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
            transaction_index: 0,
            type_: 2,
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
            }}],
            "transactionIndex":"0x0"
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
            transaction_index: 0,
            type_: 2,
        };

        assert_eq!(
            serde_json::from_str::<TransactionReceipt>(&json).unwrap(),
            expected_receipt
        );
    }
}
