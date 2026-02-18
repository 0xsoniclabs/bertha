// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

use alloy_rlp::{RlpDecodable, RlpEncodable};
use bertha_types::{
    Hash, Log, PostStateOrStatus, RECEIPT_STATUS_FAILED_RLP, RECEIPT_STATUS_SUCCESS_RLP, RlpString,
    TransactionReceipt, TransactionType,
};

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

impl TryFrom<StoredReceiptRlpWithTxType> for TransactionReceipt {
    type Error = &'static str;

    fn try_from(
        StoredReceiptRlpWithTxType {
            receipt,
            transaction_type,
        }: StoredReceiptRlpWithTxType,
    ) -> Result<Self, Self::Error> {
        let post_state_or_status = match receipt.post_state_or_status.0.as_slice() {
            RECEIPT_STATUS_FAILED_RLP => PostStateOrStatus::Status(0),
            RECEIPT_STATUS_SUCCESS_RLP => PostStateOrStatus::Status(1),
            root if root.len() == size_of::<Hash>() => {
                PostStateOrStatus::PostState(root.try_into().unwrap())
            }
            _ => return Err("invalid receipt status"),
        };

        Ok(Self {
            transaction_type,
            post_state_or_status,
            cumulative_gas_used: receipt.cumulative_gas_used,
            logs: receipt.logs,
        })
    }
}

impl From<TransactionReceipt> for StoredReceiptRlp {
    fn from(receipt: TransactionReceipt) -> Self {
        Self {
            post_state_or_status: match receipt.post_state_or_status {
                PostStateOrStatus::Status(1) => RlpString(Vec::from(RECEIPT_STATUS_SUCCESS_RLP)),
                PostStateOrStatus::Status(_) => RlpString(Vec::from(RECEIPT_STATUS_FAILED_RLP)),
                PostStateOrStatus::PostState(post_state) => RlpString(post_state.to_vec()),
            },
            cumulative_gas_used: receipt.cumulative_gas_used,
            logs: receipt.logs,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_rlp::{Decodable, Encodable};
    use bertha_types::{PostStateOrStatus, RlpString, TransactionReceipt, TransactionType};

    use crate::transaction_receipt::{StoredReceiptRlp, StoredReceiptRlpWithTxType};

    #[test]
    fn from_into_is_identity() {
        let mut orig = TransactionReceipt {
            transaction_type: TransactionType::Legacy,
            post_state_or_status: PostStateOrStatus::Status(1),
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
        let rlp_with_type = StoredReceiptRlpWithTxType {
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
