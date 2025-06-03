use alloy_rlp::{BufMut, Encodable, Header};
use alloy_trie::{HashBuilder, Nibbles};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Address, Bloom, Hash, Log, SerializableU64};

#[derive(Debug, Clone, Default, PartialEq, Eq, Error)]
#[error("the computed receipt root did not match the receipt root of the block header")]
pub struct ReceiptVerificationError;

/// Receipt for a transaction.
/// The receipt provides information about the execution of the transaction like the amount of gas
/// that was used or the emitted logs.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    //pub block_hash: Hash,
    //pub block_number: BlockNumber,
    pub cumulative_gas_used: SerializableU64,
    //pub effective_gas_price: SerializableU64,
    //pub from: Address,
    //pub gas_used: SerializableU64,
    pub logs: Vec<Log>,
    pub logs_bloom: Bloom,
    pub status: SerializableU64,
    //pub to: Address,
    //pub transaction_hash: Hash,
    pub transaction_index: SerializableU64,
    pub type_: SerializableU64,
    //pub contract_address: Option<Address>,
}

impl Encodable for TransactionReceipt {
    fn length(&self) -> usize {
        let payload_length = self.rlp_payload_length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }

    fn encode(&self, out: &mut dyn BufMut) {
        // see: https://github.com/ethereum/go-ethereum/blob/a511553e448c947a0fe8f34acf7bb6f9818c2b49/core/types/receipt.go#L122-L140
        const LEGACY_TRANSACTION_TYPE: u8 = 0;
        if self.type_ != SerializableU64::from(LEGACY_TRANSACTION_TYPE) {
            out.put_u8(Into::<u64>::into(self.type_) as u8);
        }
        Header {
            list: true,
            payload_length: self.rlp_payload_length(),
        }
        .encode(out);
        self.status.encode(out);
        self.cumulative_gas_used.encode(out);
        self.logs_bloom.encode(out);
        self.logs.encode(out);
    }
}

impl TransactionReceipt {
    fn rlp_payload_length(&self) -> usize {
        self.status.length()
            + self.cumulative_gas_used.length()
            + self.logs_bloom.length()
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
    use crate::{Hash, serializable_byte_vec::SerializableByteVec};

    #[test]

    fn encode_key_encodes_transaction_index_in_rlp() {
        let mut receipt = TransactionReceipt {
            status: SerializableU64::from(1u8),
            cumulative_gas_used: SerializableU64::from(21000u64),
            logs_bloom: Bloom::default(),
            logs: vec![],
            transaction_index: SerializableU64::from(0u8),
            type_: SerializableU64::from(0u8),
        };

        assert_eq!(
            &receipt.encode_key(),
            SerializableByteVec::try_from_hex("0x80")
                .unwrap()
                .as_bytes()
        );

        receipt.transaction_index = SerializableU64::from(1u64);
        assert_eq!(
            &receipt.encode_key(),
            SerializableByteVec::try_from_hex("0x01")
                .unwrap()
                .as_bytes()
        );
    }

    #[test]

    fn encode_value_encodes_transaction_receipt_in_rlp_and_respects_different_encoding_depending_on_type()
     {
        let mut receipt = TransactionReceipt {
            status: SerializableU64::from(1u8),
            cumulative_gas_used: SerializableU64::from(21000u64),
            logs_bloom: Bloom::default(),
            logs: vec![],
            transaction_index: SerializableU64::from(0u8),
            type_: SerializableU64::from(0u8),
        };

        // if the type == legacy transaction type (0) -> type field not encoded
        assert_eq!(&receipt.encode_value(), SerializableByteVec::try_from_hex("0xf9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap().as_bytes());

        // if the type != legacy transaction type (0) -> type field encoded
        receipt.type_ = SerializableU64::from(1u8);
        assert_eq!(&receipt.encode_value(), SerializableByteVec::try_from_hex("0x01f9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap().as_bytes());

        // if the type != legacy transaction type (0) -> type field encoded
        receipt.type_ = SerializableU64::from(2u8);
        assert_eq!(&receipt.encode_value(), SerializableByteVec::try_from_hex("0x02f9010801825208b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c0").unwrap().as_bytes());
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
            address: address.clone(),
            topics: topics.clone(),
            data: SerializableByteVec::default(),
            transaction_index: SerializableU64::from(0u64),
        };
        let receipt = TransactionReceipt {
            status: SerializableU64::from(0u8),
            cumulative_gas_used: SerializableU64::from(0u64),
            logs_bloom: Bloom::default(),
            logs: vec![log.clone()],
            transaction_index: SerializableU64::from(0u8),
            type_: SerializableU64::from(0u8),
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

    #[test]
    fn verify_computes_root_hash_correctly_and_compares_it_with_specified_root() {
        let receipt = BlockReceipt(vec![
            TransactionReceipt {
                cumulative_gas_used: SerializableU64::from(77081u64),
                logs: vec![
                    Log {
                        address: Address::try_from_hex(
                            "0x5a91d3042b71a92f6757fa937763d03cc65ed8bc",
                        )
                        .unwrap(),
                        topics: vec![
                            Hash::from([
                                221, 242, 82, 173, 27, 226, 200, 155, 105, 194, 176, 104, 252, 55,
                                141, 170, 149, 43, 167, 241, 99, 196, 161, 22, 40, 245, 90, 77,
                                245, 35, 179, 239,
                            ]),
                            Hash::from([
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 35, 65, 114, 36, 124, 148, 114,
                                58, 42, 67, 155, 205, 104, 92, 22, 153, 213, 74, 149, 176,
                            ]),
                            Hash::from([
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 153, 163, 90, 99, 1, 4, 58, 2,
                                173, 216, 155, 97, 58, 88, 238, 226, 196, 103, 80, 162,
                            ]),
                        ],
                        data: SerializableByteVec::from(
                            [
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                0, 0, 0, 35, 134, 242, 111, 193, 0, 0,
                            ]
                            .as_slice(),
                        ),
                        transaction_index: SerializableU64::from(0u64),
                    },
                    Log {
                        address: Address::try_from_hex("61a2777db1271ef53329a13d05098f47ceaa7021")
                            .unwrap(),
                        topics: vec![
                            Hash::from([
                                221, 242, 82, 173, 27, 226, 200, 155, 105, 194, 176, 104, 252, 55,
                                141, 170, 149, 43, 167, 241, 99, 196, 161, 22, 40, 245, 90, 77,
                                245, 35, 179, 239,
                            ]),
                            Hash::from([
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 153, 163, 90, 99, 1, 4, 58, 2,
                                173, 216, 155, 97, 58, 88, 238, 226, 196, 103, 80, 162,
                            ]),
                            Hash::from([
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 35, 65, 114, 36, 124, 148, 114,
                                58, 42, 67, 155, 205, 104, 92, 22, 153, 213, 74, 149, 176,
                            ]),
                        ],
                        data: SerializableByteVec::from(
                            [
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                0, 0, 5, 132, 222, 161, 181, 103, 77, 16,
                            ]
                            .as_slice(),
                        ),
                        transaction_index: SerializableU64::from(0u64),
                    },
                    Log {
                        address: Address::try_from_hex(
                            "0x99a35a6301043a02add89b613a58eee2c46750a2",
                        )
                        .unwrap(),
                        topics: vec![
                            Hash::from([
                                205, 56, 41, 163, 129, 61, 195, 205, 209, 136, 253, 61, 1, 220,
                                243, 38, 140, 22, 190, 47, 221, 45, 210, 29, 6, 101, 65, 136, 22,
                                228, 96, 98,
                            ]),
                            Hash::from([
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 35, 65, 114, 36, 124, 148, 114,
                                58, 42, 67, 155, 205, 104, 92, 22, 153, 213, 74, 149, 176,
                            ]),
                            Hash::from([
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 90, 145, 211, 4, 43, 113, 169,
                                47, 103, 87, 250, 147, 119, 99, 208, 60, 198, 94, 216, 188,
                            ]),
                            Hash::from([
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 97, 162, 119, 125, 177, 39, 30,
                                245, 51, 41, 161, 61, 5, 9, 143, 71, 206, 170, 112, 33,
                            ]),
                        ],
                        data: SerializableByteVec::from(
                            [
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                0, 0, 0, 35, 134, 242, 111, 193, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 132, 222, 161, 181,
                                103, 77, 16,
                            ]
                            .as_slice(),
                        ),
                        transaction_index: SerializableU64::from(0u64),
                    },
                ],
                logs_bloom: Bloom::from([
                    0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0, 32, 0, 0, 0, 8, 4, 0,
                    0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
                    32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 0, 0, 0, 1, 0, 0, 0, 8, 16, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 128, 0, 0, 64, 0, 0,
                    128, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 128, 0, 0,
                    2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 4, 8, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ]),
                status: SerializableU64::from(1u64),
                transaction_index: SerializableU64::from(0u64),
                type_: SerializableU64::from(2u64),
            },
            TransactionReceipt {
                cumulative_gas_used: SerializableU64::from(98081u64),
                logs: vec![],
                logs_bloom: Bloom::from([
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ]),
                status: SerializableU64::from(1u64),
                transaction_index: SerializableU64::from(1u64),
                type_: SerializableU64::from(2u64),
            },
        ]);

        // Receipts hash fetched from the SONIC chain
        let receipts_root = Hash::from([
            21, 140, 135, 208, 94, 73, 250, 151, 10, 36, 206, 228, 210, 9, 255, 54, 199, 207, 31,
            58, 195, 10, 152, 23, 85, 130, 190, 184, 47, 68, 248, 179,
        ]);
        assert!(receipt.clone().verify(&receipts_root).is_ok());

        let receipts_root = Hash::default();
        assert!(receipt.verify(&receipts_root).is_err());
    }
}
