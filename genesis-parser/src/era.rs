use std::ops::Deref;

use bertha_types::{Block, EIP2718Unmarshallable, EMPTY_OMMERS_HASH, Transaction, U256};
use e2store::{
    era::CompressedSignedBeaconBlock, ethportal_api::consensus::beacon_block::SignedBeaconBlock,
};
use tree_hash::TreeHash;

use crate::Error;

/// Parses transactions from their RLP-encoded representation.
/// Each item in `data` is expected to be a byte slice containing the RLP encoding of a single
/// transaction.
fn parse_transactions(
    data: impl IntoIterator<Item = impl Deref<Target = [u8]>>,
) -> Result<Vec<Transaction>, Error> {
    data.into_iter()
        .map(|t| Transaction::unmarshal(&mut t.deref()))
        .collect::<Result<_, _>>()
        .map_err(Error::from)
}

/// Converts a [`CompressedSignedBeaconBlock`] to a [`Block`].
pub fn convert_block(block: CompressedSignedBeaconBlock) -> Result<Block, Error> {
    match block.block {
        SignedBeaconBlock::Bellatrix(blk) => {
            let block = blk.message.body.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0,
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.0,
                state_root: block.state_root.0,
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.0,
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from(block.base_fee_per_gas.into_limbs())),
                withdrawals_root: Option::default(),
                blob_gas_used: Option::default(),
                excess_blob_gas: Option::default(),
                parent_beacon_block_root: Some(blk.message.parent_root.0),
                requests_hash: Option::default(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Capella(blk) => {
            let block = blk.message.body.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0,
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.0,
                state_root: block.state_root.0,
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.0,
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from(block.base_fee_per_gas.into_limbs())),
                withdrawals_root: Option::default(),
                blob_gas_used: Option::default(),
                excess_blob_gas: Option::default(),
                parent_beacon_block_root: Some(blk.message.parent_root.0),
                requests_hash: Option::default(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Deneb(blk) => {
            let block = blk.message.body.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0,
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.0,
                state_root: block.state_root.0,
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.0,
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from(block.base_fee_per_gas.into_limbs())),
                withdrawals_root: Option::default(),
                blob_gas_used: Option::default(),
                excess_blob_gas: Option::default(),
                parent_beacon_block_root: Some(blk.message.parent_root.0),
                requests_hash: Option::default(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Electra(blk) => {
            let execution_requests = blk.message.body.execution_requests;
            let block = blk.message.body.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0,
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.0,
                state_root: block.state_root.0,
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.0,
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from(block.base_fee_per_gas.into_limbs())),
                withdrawals_root: Some(execution_requests.withdrawals.tree_hash_root().0),
                blob_gas_used: Some(block.blob_gas_used),
                excess_blob_gas: Some(block.excess_blob_gas),
                parent_beacon_block_root: Some(blk.message.parent_root.0),
                requests_hash: Some(execution_requests.requests_hash().0),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
    }
}
