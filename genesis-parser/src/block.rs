use alloy_rlp::{RlpDecodable, RlpEncodable};
use bertha_types::{Block, EMPTY_SHA3_OMMERS_HASH, Hash, HexConvert, Transaction, U256};

use crate::transaction_receipt::StoredReceiptRlp;

// Source: sonic/inter/ibr/inter_block_records.go
#[derive(Debug, Clone, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub(crate) struct IdxFullBlock {
    block: FullBlock,
    block_number: u64, // idx
}

// Source: sonic/inter/ibr/inter_block_records.go
#[derive(Debug, Clone, PartialEq, Eq, RlpEncodable, RlpDecodable)]
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
            receipts: idx_full_block
                .block
                .receipts
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            base_fee_per_gas: Some(idx_full_block.block.base_fee),
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            requests_hash: None,
        })
    }
}

#[cfg(test)]
impl From<Block> for IdxFullBlock {
    fn from(block: Block) -> Self {
        let timestamp_secs = block.timestamp;
        let timestamp_nanos = u32::from_be_bytes(block.extra_data[0..4].try_into().unwrap());
        let duration = u64::from_be_bytes(block.extra_data[4..12].try_into().unwrap());
        let timestamp = timestamp_secs * 10u64.pow(9) + u64::from(timestamp_nanos);

        IdxFullBlock {
            block: FullBlock {
                block_hash: Hash::default(), // TODO block.header().hash(),
                parent_hash: block.parent_hash,
                state_root: block.state_root,
                timestamp,
                duration,
                difficulty: block.difficulty,
                gas_limit: block.gas_limit,
                gas_used: block.receipts.last().map_or(0, |r| r.cumulative_gas_used),
                base_fee: block.base_fee_per_gas.unwrap_or_default(),
                prev_randao: block.prev_randao,
                epoch: 0,
                txn: block.transactions,
                receipts: block.receipts.into_iter().map(Into::into).collect(),
            },
            block_number: block.number,
        }
    }
}
