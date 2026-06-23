// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use std::ops::Deref;

use bertha_types::{
    Block, EIP2718Unmarshallable, EMPTY_OMMERS_HASH, Transaction, U256, Withdrawal,
    compute_root_hash,
};
use lighthouse_types::{SignedBeaconBlock, core::MainnetEthSpec};

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

fn parse_withdrawals(
    data: impl IntoIterator<Item = lighthouse_types::Withdrawal>,
) -> Vec<Withdrawal> {
    data.into_iter()
        .map(|w| Withdrawal {
            index: w.index,
            validator_index: w.validator_index,
            address: w.address.into(),
            amount: w.amount,
        })
        .collect()
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
                withdrawals: Vec::new(),
                blob_gas_used: Option::default(),
                excess_blob_gas: Option::default(),
                parent_beacon_block_root: None,
                requests_hash: Option::default(),
                ommer_headers: Vec::new(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Capella(blk) => {
            let block = blk.message.body.execution_payload.execution_payload;
            let withdrawals = parse_withdrawals(block.withdrawals);
            let withdrawals_root = compute_root_hash(&withdrawals);

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
                withdrawals_root: Some(withdrawals_root),
                withdrawals,
                blob_gas_used: Option::default(),
                excess_blob_gas: Option::default(),
                parent_beacon_block_root: None,
                requests_hash: Option::default(),
                ommer_headers: Vec::new(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Deneb(blk) => {
            let block = blk.message.body.execution_payload.execution_payload;
            let withdrawals = parse_withdrawals(block.withdrawals);
            let withdrawals_root = compute_root_hash(&withdrawals);

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
                withdrawals_root: Some(withdrawals_root),
                withdrawals,
                blob_gas_used: Some(block.blob_gas_used),
                excess_blob_gas: Some(block.excess_blob_gas),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Option::default(),
                ommer_headers: Vec::new(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Electra(blk) => {
            let execution_requests = blk.message.body.execution_requests;
            let block = blk.message.body.execution_payload.execution_payload;
            let withdrawals = parse_withdrawals(block.withdrawals);
            let withdrawals_root = compute_root_hash(&withdrawals);

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
                withdrawals_root: Some(withdrawals_root),
                withdrawals,
                blob_gas_used: Some(block.blob_gas_used),
                excess_blob_gas: Some(block.excess_blob_gas),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Some(execution_requests.requests_hash().into()),
                ommer_headers: Vec::new(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        SignedBeaconBlock::Fulu(blk) => {
            let execution_requests = blk.message.body.execution_requests;
            let block = blk.message.body.execution_payload.execution_payload;
            let withdrawals = parse_withdrawals(block.withdrawals);
            let withdrawals_root = compute_root_hash(&withdrawals);

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
                withdrawals_root: Some(withdrawals_root),
                withdrawals,
                blob_gas_used: Some(block.blob_gas_used),
                excess_blob_gas: Some(block.excess_blob_gas),
                parent_beacon_block_root: Some(blk.message.parent_root.into()),
                requests_hash: Some(execution_requests.requests_hash().into()),
                ommer_headers: Vec::new(),
                verkle_state_root: None,
                binary_state_root: None,
            })
        }
        _ => Err(Error::Era("unsupported beacon block fork".to_string())),
    }
}
