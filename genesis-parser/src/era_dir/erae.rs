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

use alloy_consensus::{BlockBody, Eip658Value, EthereumTxEnvelope, Header, TxEip4844Variant};
use bertha_types::{
    Block, Log, OmmerHeader, PostStateOrStatus, TransactionReceipt, U256, Withdrawal,
};
use reth_era::{
    common::decode::DecodeCompressedRlp,
    ere::types::execution::{BlockTuple, SlimReceipt},
};

use crate::{Error, era_dir::common};

/// Converts a [`SlimReceipt`] to a [`TransactionReceipt`].
fn convert_slim_receipt(receipt: SlimReceipt) -> TransactionReceipt {
    let transaction_type = common::convert_tx_type(receipt.tx_type);
    TransactionReceipt {
        transaction_type,
        post_state_or_status: match receipt.status {
            Eip658Value::Eip658(s) => PostStateOrStatus::Status(s as u64),
            Eip658Value::PostState(state) => PostStateOrStatus::PostState(state.0),
        },
        cumulative_gas_used: receipt.cumulative_gas_used,
        logs: receipt
            .logs
            .into_iter()
            .map(|log| Log {
                address: log.address.0.0,
                topics: log.data.topics().iter().map(|t| t.0).collect(),
                data: log.data.data.to_vec(),
            })
            .collect(),
    }
}

/// Converts an ere [`BlockTuple`] to a [`Block`].
pub fn convert_block(block: &BlockTuple) -> Result<Block, Error> {
    let header: Header = block.header.decode()?;
    let body = block
        .body
        .decode::<BlockBody<EthereumTxEnvelope<TxEip4844Variant>>>()?;
    let transactions = body
        .transactions
        .into_iter()
        .map(common::convert_transaction)
        .collect();
    let ommer_headers = body
        .ommers
        .iter()
        .map(|h| OmmerHeader {
            beneficiary: h.beneficiary.0.0,
            number: h.number,
        })
        .collect();
    let withdrawals = body
        .withdrawals
        .map(|ws| {
            ws.into_iter()
                .map(|w| Withdrawal {
                    index: w.index,
                    validator_index: w.validator_index,
                    address: w.address.0.0,
                    amount: w.amount,
                })
                .collect()
        })
        .unwrap_or_default();
    let receipts = block
        .receipts
        .as_ref()
        .map(|compressed| {
            compressed
                .decode_receipts()
                .map(|receipts| receipts.into_iter().map(convert_slim_receipt).collect())
        })
        .transpose()?
        .unwrap_or_default();

    Ok(Block {
        parent_hash: header.parent_hash.0,
        ommers_hash: header.ommers_hash.0,
        beneficiary: header.beneficiary.0.0,
        state_root: header.state_root.0,
        difficulty: u64::from_le_bytes(header.difficulty.as_le_slice()[..8].try_into().unwrap()),
        number: header.number,
        gas_limit: header.gas_limit,
        timestamp: header.timestamp,
        extra_data: header.extra_data.0.to_vec(),
        prev_randao: header.mix_hash.0,
        nonce: header.nonce.0,
        transactions,
        receipts,
        base_fee_per_gas: header.base_fee_per_gas.map(U256::from),
        withdrawals_root: header.withdrawals_root.map(|w| w.0),
        withdrawals,
        blob_gas_used: header.blob_gas_used,
        excess_blob_gas: header.excess_blob_gas,
        parent_beacon_block_root: header.parent_beacon_block_root.map(|r| r.0),
        requests_hash: header.requests_hash.map(|h| h.0),
        ommer_headers,
        verkle_state_root: None,
        binary_state_root: None,
    })
}
