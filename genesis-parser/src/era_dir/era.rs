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
use reth_chainspec::ChainSpec;

use crate::{Error, era_dir::common};

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

/// Converts a [`SignedBeaconBlock`] to a [`Block`]. Block 0 is taken from the chain spec of the
/// given chain, since the beacon block at slot 0 carries an empty execution payload instead.
pub fn convert_block(
    block: SignedBeaconBlock<MainnetEthSpec>,
    chain_id: u64,
) -> Result<Block, Error> {
    let converted = match block {
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
    }?;
    if converted.number == 0 {
        return ChainSpec::from_chain_id(chain_id)
            .map(|spec| common::header_only_block(spec.genesis_header()))
            .ok_or_else(|| Error::Era(format!("no chain spec for chain {chain_id}")));
    }
    Ok(converted)
}

#[cfg(test)]
mod tests {
    use bertha_types::HexConvert;
    use lighthouse_types::{BeaconBlock, ForkName, ForkVersionDecode};
    use ssz::Encode;

    use super::*;

    /// Returns a Deneb beacon block with an empty execution payload of the given block number.
    fn deneb_block(number: u64) -> SignedBeaconBlock<MainnetEthSpec> {
        let spec = ForkName::Deneb.make_genesis_spec(lighthouse_types::ChainSpec::mainnet());
        let mut block = BeaconBlock::<MainnetEthSpec>::empty(&spec);
        block
            .body_deneb_mut()
            .unwrap()
            .execution_payload
            .execution_payload
            .block_number = number;
        // An all-zero signature decodes as the empty signature.
        let bytes = [&100u32.to_le_bytes()[..], &[0; 96], &block.as_ssz_bytes()].concat();
        SignedBeaconBlock::from_ssz_bytes_by_fork(&bytes, ForkName::Deneb).unwrap()
    }

    #[rstest::rstest]
    #[case::mainnet(
        1,
        "0xd4e56740f876aef8c010b86a40d5f56745a118d0906a34e69aec8c0db1cb8fa3"
    )]
    #[case::sepolia(
        11155111,
        "0x25a5cc106eea7138acab33231d7160d69cb777ee0c2c553fcddf5138993e6dd9"
    )]
    #[case::hoodi(
        560048,
        "0xbbe312868b376a3001692a646dd2d7d1e4406380dfd86b98aa8a34d1557c971b"
    )]
    fn convert_block_takes_block_0_from_chain_spec(#[case] chain_id: u64, #[case] hash: &str) {
        let block = convert_block(deneb_block(0), chain_id).unwrap();
        assert_eq!(block.to_header().compute_hash().to_hex(), hash);
    }

    #[test]
    fn convert_block_takes_other_blocks_from_payload() {
        let block = convert_block(deneb_block(1), 560048).unwrap();
        assert_eq!(block.number, 1);
        assert_eq!(block.state_root, [0; 32]);
    }

    #[test]
    fn convert_block_fails_for_block_0_of_unknown_chain() {
        let result = convert_block(deneb_block(0), 0);
        assert!(matches!(result, Err(Error::Era(msg)) if msg == "no chain spec for chain 0"));
    }
}
