// Source: go-ethereum/core/types/receipt.go

use alloy_rlp::{Decodable, Encodable, Header};
use bertha_types::{Hash, Log, TransactionReceipt, TransactionType};

// Source: go-ethereum/core/types/receipt.go (storedReceiptRLP)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredReceiptRlp {
    post_state_or_status: Vec<u8>,
    cumulative_gas_used: u64,
    logs: Vec<Log>,
    transaction_type: TransactionType, // added for conversion to TransactionReceipt
}

impl Decodable for StoredReceiptRlp {
    // Source: go-ethereum/core/types/receipt.go (DecodeRLP)
    fn decode(rlp: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let header = Header::decode(rlp)?;
        let transaction_type;
        if header.list {
            transaction_type = TransactionType::Legacy;
        } else {
            // Source: go-ethereum/core/types/receipt.go (decodeTyped)
            if rlp.is_empty() {
                return Err(alloy_rlp::Error::InputTooShort);
            }
            transaction_type = TransactionType::try_from(rlp[0])
                .map_err(|_| alloy_rlp::Error::Custom("invalid transaction type"))?;
            *rlp = &rlp[1..];
        }

        let orig_len = rlp.len();
        if orig_len < header.payload_length {
            return Err(alloy_rlp::Error::InputTooShort);
        }
        let receipt = Self {
            post_state_or_status: Header::decode_bytes(rlp, false)?.to_vec(), // custom
            cumulative_gas_used: u64::decode(rlp)?,
            logs: Vec::decode(rlp)?,
            transaction_type,
        };
        let consumed = orig_len - rlp.len();
        if consumed != header.payload_length {
            return Err(alloy_rlp::Error::ListLengthMismatch {
                expected: header.payload_length,
                got: consumed,
            });
        }
        Ok(receipt)
    }
}

impl StoredReceiptRlp {
    fn rlp_payload_length(&self) -> usize {
        self.post_state_or_status.as_slice().length() // custom
            + self.cumulative_gas_used.length()
            + self.logs.length()
    }
}

impl Encodable for StoredReceiptRlp {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        if self.transaction_type == TransactionType::Legacy {
            let h = Header {
                list: true,
                payload_length: self.rlp_payload_length(),
            };
            h.encode(out);
        } else {
            Header {
                list: false,
                payload_length: self.rlp_payload_length(),
            }
            .encode(out);
            out.put_u8(self.transaction_type as u8);
        }
        self.post_state_or_status.as_slice().encode(out); // custom
        self.cumulative_gas_used.encode(out);
        self.logs.encode(out);
    }
}

// Source: go-ethereum/core/types/receipt.go (SetStatus)
const RECEIPT_STATUS_SUCCESS_RLP: &[u8] = &[0x01];
const RECEIPT_STATUS_FAILED_RLP: &[u8] = &[];
const RECEIPT_STATUS_SUCCESS: u64 = 1;
const RECEIPT_STATUS_FAILED: u64 = 0;

impl TryFrom<StoredReceiptRlp> for TransactionReceipt {
    type Error = &'static str;

    fn try_from(receipt_rlp: StoredReceiptRlp) -> Result<Self, Self::Error> {
        // Source: go-ethereum/core/types/receipt.go (SetStatus, SetFromRLP)
        let status = match receipt_rlp.post_state_or_status.as_slice() {
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
            transaction_type: receipt_rlp.transaction_type,
            status,
            cumulative_gas_used: receipt_rlp.cumulative_gas_used,
            logs: receipt_rlp.logs,
        })
    }
}

impl From<TransactionReceipt> for StoredReceiptRlp {
    fn from(receipt: TransactionReceipt) -> Self {
        Self {
            // Source: go-ethereum/core/types/receipt.go (statusEncoding)
            // in Sonic post_state / root is not used and always empty
            post_state_or_status: if receipt.status == RECEIPT_STATUS_FAILED {
                Vec::from(RECEIPT_STATUS_FAILED_RLP)
            } else {
                Vec::from(RECEIPT_STATUS_SUCCESS_RLP)
            },
            cumulative_gas_used: receipt.cumulative_gas_used,
            logs: receipt.logs,
            transaction_type: receipt.transaction_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_rlp::{Decodable, Encodable};
    use bertha_types::{TransactionReceipt, TransactionType};

    use crate::transaction_receipt::StoredReceiptRlp;

    #[test]
    fn from_into_is_identify() {
        let mut orig = TransactionReceipt {
            transaction_type: TransactionType::Legacy,
            status: 1,
            cumulative_gas_used: 21000,
            logs: vec![],
        };
        for transaction_type in 0..=4 {
            orig.transaction_type = TransactionType::try_from(transaction_type).unwrap();
            let rlp: StoredReceiptRlp = orig.clone().into();
            let receipt: TransactionReceipt = rlp.try_into().unwrap();
            assert_eq!(orig, receipt);
        }
    }

    #[test]
    fn encode_decode_is_identity() {
        let mut orig = StoredReceiptRlp {
            post_state_or_status: vec![0x01],
            cumulative_gas_used: 21000,
            logs: vec![],
            transaction_type: TransactionType::Legacy,
        };
        for transaction_type in 0..=4 {
            orig.transaction_type = TransactionType::try_from(transaction_type).unwrap();
            let mut buf = Vec::new();
            orig.encode(&mut buf);
            let decoded = StoredReceiptRlp::decode(&mut buf.as_slice()).unwrap();
            assert_eq!(orig, decoded);
        }
    }
}
