use alloy_rlp::{RlpDecodable, RlpEncodable};
use bertha_types::{
    Block, EMPTY_SHA3_OMMERS_HASH, EMPTY_TREE_ROOT_HASH, Hash, HexConvert, Transaction, U256,
};

use crate::transaction_receipt::{StoredReceiptRlp, StoredReceiptRlpWithTxType};

// Source: sonic/inter/ibr/inter_block_records.go
#[derive(Debug, Clone, Default, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub(crate) struct IdxFullBlock {
    block: FullBlock,
    block_number: u64, // idx
}

// Source: sonic/inter/ibr/inter_block_records.go
#[derive(Debug, Clone, Default, PartialEq, Eq, RlpEncodable, RlpDecodable)]
struct FullBlock {
    block_hash: Hash,
    parent_hash: Hash,
    state_root: Hash,
    timestamp: u64,
    duration: u64,
    difficulty: u64,
    gas_limit: u64,
    gas_used: u64,
    base_fee: U256,
    prev_randao: Hash,
    epoch: u32,
    txn: Vec<Transaction>,
    receipts: Vec<StoredReceiptRlp>,
}

impl TryFrom<IdxFullBlock> for Block {
    type Error = &'static str;

    fn try_from(idx_full_block: IdxFullBlock) -> Result<Self, Self::Error> {
        let mut extra_data = Vec::new();
        let timestamp_nanos = idx_full_block.block.timestamp.rem_euclid(10u64.pow(9)) as u32;
        extra_data.extend_from_slice(&timestamp_nanos.to_be_bytes());
        extra_data.extend_from_slice(&idx_full_block.block.duration.to_be_bytes());

        // timestamp is in nanoseconds
        let timestamp_secs = idx_full_block.block.timestamp.div_euclid(10u64.pow(9));

        let receipts = idx_full_block
            .block
            .receipts
            .into_iter()
            .zip(&idx_full_block.block.txn)
            .map(|(receipt, tx)| StoredReceiptRlpWithTxType {
                receipt,
                transaction_type: tx.transaction_type,
            })
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            parent_hash: idx_full_block.block.parent_hash,
            ommers_hash: Hash::try_from_hex(EMPTY_SHA3_OMMERS_HASH).unwrap(),
            beneficiary: Default::default(),
            state_root: idx_full_block.block.state_root,
            difficulty: idx_full_block.block.difficulty,
            number: idx_full_block.block_number,
            gas_limit: idx_full_block.block.gas_limit,
            timestamp: timestamp_secs,
            extra_data,
            prev_randao: idx_full_block.block.prev_randao,
            nonce: [0; 8],
            transactions: idx_full_block.block.txn,
            receipts,
            base_fee_per_gas: Some(idx_full_block.block.base_fee),
            withdrawals_root: Some(Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap()),
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            requests_hash: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_from_idx_full_block_converts_timestamp_duration_and_extra_data() {
        let idx_full_block = IdxFullBlock {
            block: FullBlock {
                block_hash: Hash::from([0; 32]),
                parent_hash: Hash::from([1; 32]),
                state_root: Hash::from([2; 32]),
                timestamp: 1234567890123,
                duration: 1000,
                difficulty: 42,
                gas_limit: 8000000,
                gas_used: 5000000,
                base_fee: U256::from(100u8),
                prev_randao: Hash::from([3; 32]),
                epoch: 1,
                txn: vec![],
                receipts: vec![],
            },
            block_number: 0,
        };

        let block = Block {
            parent_hash: Hash::from([1; 32]),
            ommers_hash: Hash::try_from_hex(EMPTY_SHA3_OMMERS_HASH).unwrap(),
            beneficiary: Default::default(),
            state_root: Hash::from([2; 32]),
            difficulty: 42,
            number: 0,
            gas_limit: 8000000,
            timestamp: 1234, // Converted to seconds
            // Timestamp in nanoseconds (4 bytes in big endian) + duration (8 bytes in big endian)
            extra_data: vec![
                0x21, 0xd9, 0x50, 0xcb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xe8,
            ],
            prev_randao: Hash::from([3; 32]),
            nonce: [0; 8],
            transactions: vec![],
            receipts: vec![],
            base_fee_per_gas: Some(U256::from(100u8)),
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            requests_hash: None,
        };

        assert_eq!(Block::try_from(idx_full_block).unwrap(), block);
    }
}
