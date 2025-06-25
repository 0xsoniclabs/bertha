use alloy_rlp::{RlpDecodable, RlpEncodable};
use bertha_types::{Hash, Log, RlpString, TransactionReceipt, TransactionType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredReceiptRlpWithTxType {
    pub receipt: StoredReceiptRlp,
    pub transaction_type: TransactionType,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub(crate) struct StoredReceiptRlp {
    pub post_state_or_status: RlpString,
    pub cumulative_gas_used: u64,
    pub logs: Vec<Log>,
}

const RECEIPT_STATUS_SUCCESS_RLP: &[u8] = &[0x01];
const RECEIPT_STATUS_FAILED_RLP: &[u8] = &[];
const RECEIPT_STATUS_SUCCESS: u64 = 1;
const RECEIPT_STATUS_FAILED: u64 = 0;

impl TryFrom<StoredReceiptRlpWithTxType> for TransactionReceipt {
    type Error = &'static str;

    fn try_from(
        StoredReceiptRlpWithTxType {
            receipt,
            transaction_type,
        }: StoredReceiptRlpWithTxType,
    ) -> Result<Self, Self::Error> {
        let status = match receipt.post_state_or_status.0.as_slice() {
            RECEIPT_STATUS_FAILED_RLP => RECEIPT_STATUS_FAILED,
            RECEIPT_STATUS_SUCCESS_RLP => RECEIPT_STATUS_SUCCESS,
            root if root.len() == size_of::<Hash>() => {
                return Err(
                    "post_state_or_status should not contain a hash for root/post_state in sonic",
                );
            }
            _ => {
                return Err("invalid receipt status");
            }
        };

        Ok(Self {
            transaction_type,
            status,
            cumulative_gas_used: receipt.cumulative_gas_used,
            logs: receipt.logs,
        })
    }
}

impl From<TransactionReceipt> for StoredReceiptRlp {
    fn from(receipt: TransactionReceipt) -> Self {
        Self {
            // in Sonic post_state / root is not used and always empty
            post_state_or_status: if receipt.status == RECEIPT_STATUS_FAILED {
                RlpString(Vec::from(RECEIPT_STATUS_FAILED_RLP))
            } else {
                RlpString(Vec::from(RECEIPT_STATUS_SUCCESS_RLP))
            },
            cumulative_gas_used: receipt.cumulative_gas_used,
            logs: receipt.logs,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_rlp::{Decodable, Encodable};
    use bertha_types::{Hash, RlpString, TransactionReceipt, TransactionType};

    use crate::transaction_receipt::{StoredReceiptRlp, StoredReceiptRlpWithTxType};

    #[test]
    fn from_into_is_identity() {
        let mut orig = TransactionReceipt {
            transaction_type: TransactionType::Legacy,
            status: 1,
            cumulative_gas_used: 21000,
            logs: vec![],
        };
        for transaction_type in 0..=4 {
            orig.transaction_type = TransactionType::try_from(transaction_type).unwrap();
            let rlp: StoredReceiptRlp = orig.clone().into();
            let rlp_with_type = StoredReceiptRlpWithTxType {
                receipt: rlp,
                transaction_type: orig.transaction_type,
            };
            let receipt: TransactionReceipt = rlp_with_type.try_into().unwrap();
            assert_eq!(orig, receipt);
        }
    }

    #[test]
    fn from_returns_error_if_status_is_invalid() {
        let mut rlp_with_type = StoredReceiptRlpWithTxType {
            receipt: StoredReceiptRlp {
                post_state_or_status: RlpString(vec![0x02]),
                cumulative_gas_used: Default::default(),
                logs: Vec::default(),
            },
            transaction_type: TransactionType::Legacy,
        };
        assert_eq!(
            TransactionReceipt::try_from(rlp_with_type.clone()).unwrap_err(),
            "invalid receipt status"
        );

        rlp_with_type.receipt.post_state_or_status = RlpString(Hash::default().to_vec());
        assert_eq!(
            TransactionReceipt::try_from(rlp_with_type).unwrap_err(),
            "post_state_or_status should not contain a hash for root/post_state in sonic"
        );
    }

    #[test]
    fn encode_decode_is_identity() {
        let orig = StoredReceiptRlp {
            post_state_or_status: RlpString(vec![0x01]),
            cumulative_gas_used: 21000,
            logs: vec![],
        };
        let mut buf = Vec::new();
        orig.encode(&mut buf);
        let decoded = StoredReceiptRlp::decode(&mut buf.as_slice()).unwrap();
        assert_eq!(orig, decoded);
    }
}
