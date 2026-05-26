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

use std::ops::Deref;

use bertha_types::{
    Block, EIP2718Unmarshallable, EMPTY_OMMERS_HASH, EMPTY_TREE_ROOT_HASH, Transaction, U256,
};
use lighthouse_types::{
    BeaconState, ExecutionPayloadHeader, SignedBeaconBlock, core::MainnetEthSpec,
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

/// Converts a [`SignedBeaconBlock`] to a [`Block`].
pub fn convert_block(block: SignedBeaconBlock<MainnetEthSpec>) -> Result<Block, Error> {
    match block {
        SignedBeaconBlock::Bellatrix(blk) => {
            let block = blk.message.body.execution_payload.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0.into(),
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.into(),
                state_root: block.state_root.into(),
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.into(),
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from_le_bytes(block.base_fee_per_gas.to_le_bytes())),
                withdrawals_root: Option::default(),
                blob_gas_used: Option::default(),
                excess_blob_gas: Option::default(),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Option::default(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Capella(blk) => {
            let block = blk.message.body.execution_payload.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0.into(),
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.into(),
                state_root: block.state_root.into(),
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.into(),
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from_le_bytes(block.base_fee_per_gas.to_le_bytes())),
                withdrawals_root: Option::default(),
                blob_gas_used: Option::default(),
                excess_blob_gas: Option::default(),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Option::default(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Deneb(blk) => {
            let block = blk.message.body.execution_payload.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0.into(),
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.into(),
                state_root: block.state_root.into(),
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.into(),
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from_le_bytes(block.base_fee_per_gas.to_le_bytes())),
                withdrawals_root: Option::default(),
                blob_gas_used: Some(block.blob_gas_used),
                excess_blob_gas: Some(block.excess_blob_gas),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Option::default(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Electra(blk) => {
            let execution_requests = blk.message.body.execution_requests;
            let block = blk.message.body.execution_payload.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0.into(),
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.into(),
                state_root: block.state_root.into(),
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.into(),
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from_le_bytes(block.base_fee_per_gas.to_le_bytes())),
                withdrawals_root: Some(execution_requests.withdrawals.tree_hash_root().into()),
                blob_gas_used: Some(block.blob_gas_used),
                excess_blob_gas: Some(block.excess_blob_gas),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Some(execution_requests.requests_hash().into()),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Fulu(blk) => {
            let execution_requests = blk.message.body.execution_requests;
            let block = blk.message.body.execution_payload.execution_payload;
            Ok(Block {
                parent_hash: block.parent_hash.0.into(),
                ommers_hash: EMPTY_OMMERS_HASH,
                beneficiary: block.fee_recipient.0.into(),
                state_root: block.state_root.into(),
                difficulty: u64::default(), // 0 for proof-of-stake
                number: block.block_number,
                gas_limit: block.gas_limit,
                timestamp: block.timestamp,
                extra_data: block.extra_data.to_vec(),
                prev_randao: block.prev_randao.into(),
                nonce: <[u8; 8]>::default(), // 0 for proof-of-stake
                transactions: parse_transactions(block.transactions)?,
                receipts: Vec::default(), // .era files don't contain receipts
                base_fee_per_gas: Some(U256::from_le_bytes(block.base_fee_per_gas.to_le_bytes())),
                withdrawals_root: Some(execution_requests.withdrawals.tree_hash_root().into()),
                blob_gas_used: Some(block.blob_gas_used),
                excess_blob_gas: Some(block.excess_blob_gas),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Some(execution_requests.requests_hash().into()),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        _ => Err(Error::Era("unsupported beacon block fork".to_string())),
    }
}

/// Converts the genesis [`BeaconState`] to a [`Block`] by extracting the
/// `latest_execution_payload_header`. This is used for genesis era files, which contain only a
/// `BeaconState` and no `SignedBeaconBlock` entries.
pub fn convert_genesis_block(state: BeaconState<MainnetEthSpec>) -> Result<Block, Error> {
    let header = match state {
        BeaconState::Bellatrix(s) => {
            ExecutionPayloadHeader::Bellatrix(s.latest_execution_payload_header)
        }
        BeaconState::Capella(s) => {
            ExecutionPayloadHeader::Capella(s.latest_execution_payload_header)
        }
        BeaconState::Deneb(s) => ExecutionPayloadHeader::Deneb(s.latest_execution_payload_header),
        BeaconState::Electra(s) => {
            ExecutionPayloadHeader::Electra(s.latest_execution_payload_header)
        }
        BeaconState::Fulu(s) => ExecutionPayloadHeader::Fulu(s.latest_execution_payload_header),
        _ => return Err(Error::Era("unsupported beacon block fork".to_string())),
    };

    Ok(Block {
        parent_hash: header.parent_hash().0.into(),
        ommers_hash: EMPTY_OMMERS_HASH,
        beneficiary: header.fee_recipient().0.into(),
        state_root: header.state_root().into(),
        difficulty: 1, // TODO
        number: header.block_number(),
        gas_limit: header.gas_limit(),
        timestamp: header.timestamp(),
        extra_data: header.extra_data().to_vec(),
        prev_randao: header.prev_randao().into(),
        nonce: [0, 0, 0, 0, 0, 0, 0x12, 0x34], // TODO <[u8; 8]>::default(), // 0 for proof-of-stake
        transactions: Vec::new(),              // not stored in the execution payload header
        receipts: Vec::new(),                  // not stored in the execution payload header
        base_fee_per_gas: Some(U256::from_le_bytes(header.base_fee_per_gas().to_le_bytes())),
        // TODO header.withdrawals_root().ok().map(Into::into),
        withdrawals_root: Some(EMPTY_TREE_ROOT_HASH),
        blob_gas_used: header.blob_gas_used().ok(),
        excess_blob_gas: header.excess_blob_gas().ok(),
        parent_beacon_block_root: Some([0; 32]), // block 0 has no parent
        requests_hash: None,
        verkle_state_root: None,
        binary_state_root: None,
    })
}
