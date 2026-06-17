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

use alloy_consensus::{
    BlockBody, Eip658Value, EthereumTxEnvelope, Header, ReceiptEnvelope, TxEip4844Variant,
};
use alloy_eips::{eip2930::AccessList, eip7702::SignedAuthorization};
use bertha_types::{
    AccessListEntry, Block, Log, PostStateOrStatus, SetCodeAuthorization, Transaction,
    TransactionReceipt, TransactionType, U256,
};
use reth_era::{common::decode::DecodeCompressedRlp, era1::types::execution::BlockTuple};

use crate::Error;

/// Converts an [`AccessList`] to a [`Vec<AccessListEntry>`].
fn convert_access_list(access_list: &AccessList) -> Vec<AccessListEntry> {
    access_list
        .0
        .iter()
        .map(|item| AccessListEntry {
            address: item.address.0.0,
            storage_keys: item.storage_keys.iter().map(|k| k.0).collect(),
        })
        .collect()
}

/// Converts a list of [`SignedAuthorization`] to a list of [`SetCodeAuthorization`].
fn convert_authorization_list(
    authorization_list: &[SignedAuthorization],
) -> Vec<SetCodeAuthorization> {
    authorization_list
        .iter()
        .map(|a| SetCodeAuthorization {
            chain_id: U256::from_le_bytes(a.chain_id().to_le_bytes()),
            address: a.address.0.0,
            nonce: a.nonce,
            y_parity: a.y_parity(),
            r: U256::from_le_bytes(a.r().to_le_bytes()),
            s: U256::from_le_bytes(a.s().to_le_bytes()),
        })
        .collect()
}

/// Converts an [`EthereumTxEnvelope<TxEip4844Variant>`] to a [`Transaction`].
fn convert_transaction(transaction: EthereumTxEnvelope<TxEip4844Variant>) -> Transaction {
    match transaction {
        EthereumTxEnvelope::Legacy(signed) => {
            let sig = signed.signature();
            let tx = signed.tx();

            // EIP-155
            let y_parity = if let Some(chain_id) = tx.chain_id {
                sig.v() as u64 + chain_id * 2 + 35
            } else {
                sig.v() as u64 + 27
            };
            Transaction {
                transaction_type: TransactionType::Legacy,
                chain_id: tx.chain_id.unwrap_or_default().into(),
                nonce: tx.nonce,
                gas_price: tx.gas_price.into(),
                gas_limit: tx.gas_limit,
                to: tx.to.into_to().map(|to| to.0.0),
                value: U256::from_le_bytes(tx.value.to_le_bytes()),
                data: tx.input.to_vec(),
                access_list: Vec::default(),
                max_fee_per_gas: U256::default(),
                max_priority_fee_per_gas: U256::default(),
                blob_versioned_hashes: Vec::default(),
                max_fee_per_blob_gas: U256::default(),
                authorization_list: Vec::default(),
                y_parity: y_parity.into(),
                r: U256::from_le_bytes(sig.r().to_le_bytes()),
                s: U256::from_le_bytes(sig.s().to_le_bytes()),
            }
        }
        EthereumTxEnvelope::Eip2930(signed) => {
            let sig = signed.signature();
            let tx = signed.tx();
            Transaction {
                transaction_type: TransactionType::AccessList,
                chain_id: tx.chain_id.into(),
                nonce: tx.nonce,
                gas_price: tx.gas_price.into(),
                gas_limit: tx.gas_limit,
                to: tx.to.into_to().map(|to| to.0.0),
                value: U256::from_le_bytes(tx.value.to_le_bytes()),
                data: tx.input.to_vec(),
                access_list: convert_access_list(&tx.access_list),
                max_fee_per_gas: U256::default(),
                max_priority_fee_per_gas: U256::default(),
                blob_versioned_hashes: Vec::default(),
                max_fee_per_blob_gas: U256::default(),
                authorization_list: Vec::default(),
                y_parity: sig.v().into(),
                r: U256::from_le_bytes(sig.r().to_le_bytes()),
                s: U256::from_le_bytes(sig.s().to_le_bytes()),
            }
        }
        EthereumTxEnvelope::Eip1559(signed) => {
            let sig = signed.signature();
            let tx = signed.tx();
            Transaction {
                transaction_type: TransactionType::DynamicFee,
                chain_id: tx.chain_id.into(),
                nonce: tx.nonce,
                gas_price: U256::default(),
                gas_limit: tx.gas_limit,
                to: tx.to.into_to().map(|to| to.0.0),
                value: U256::from_le_bytes(tx.value.to_le_bytes()),
                data: tx.input.to_vec(),
                access_list: convert_access_list(&tx.access_list),
                max_fee_per_gas: tx.max_fee_per_gas.into(),
                max_priority_fee_per_gas: tx.max_priority_fee_per_gas.into(),
                blob_versioned_hashes: Vec::default(),
                max_fee_per_blob_gas: U256::default(),
                authorization_list: Vec::default(),
                y_parity: sig.v().into(),
                r: U256::from_le_bytes(sig.r().to_le_bytes()),
                s: U256::from_le_bytes(sig.s().to_le_bytes()),
            }
        }
        EthereumTxEnvelope::Eip4844(signed) => {
            let sig = signed.signature();
            let tx = signed.tx();
            let tx = match tx {
                TxEip4844Variant::TxEip4844(tx) => tx,
                TxEip4844Variant::TxEip4844WithSidecar(tx) => tx.tx(),
            };
            Transaction {
                transaction_type: TransactionType::Blob,
                chain_id: tx.chain_id.into(),
                nonce: tx.nonce,
                gas_price: U256::default(),
                gas_limit: tx.gas_limit,
                to: Some(tx.to.0.0),
                value: U256::from_le_bytes(tx.value.to_le_bytes()),
                data: tx.input.to_vec(),
                access_list: convert_access_list(&tx.access_list),
                max_fee_per_gas: tx.max_fee_per_gas.into(),
                max_priority_fee_per_gas: tx.max_priority_fee_per_gas.into(),
                blob_versioned_hashes: tx.blob_versioned_hashes.iter().map(|h| h.0).collect(),
                max_fee_per_blob_gas: tx.max_fee_per_blob_gas.into(),
                authorization_list: Vec::default(),
                y_parity: sig.v().into(),
                r: U256::from_le_bytes(sig.r().to_le_bytes()),
                s: U256::from_le_bytes(sig.s().to_le_bytes()),
            }
        }
        EthereumTxEnvelope::Eip7702(signed) => {
            let sig = signed.signature();
            let tx = signed.tx();
            Transaction {
                transaction_type: TransactionType::SetCode,
                chain_id: tx.chain_id.into(),
                nonce: tx.nonce,
                gas_price: U256::default(),
                gas_limit: tx.gas_limit,
                to: Some(tx.to.0.0),
                value: U256::from_le_bytes(tx.value.to_le_bytes()),
                data: tx.input.to_vec(),
                access_list: convert_access_list(&tx.access_list),
                max_fee_per_gas: tx.max_fee_per_gas.into(),
                max_priority_fee_per_gas: tx.max_priority_fee_per_gas.into(),
                blob_versioned_hashes: Vec::default(),
                max_fee_per_blob_gas: U256::default(),
                authorization_list: convert_authorization_list(&tx.authorization_list),
                y_parity: sig.v().into(),
                r: U256::from_le_bytes(sig.r().to_le_bytes()),
                s: U256::from_le_bytes(sig.s().to_le_bytes()),
            }
        }
    }
}

/// Converts a [`ReceiptEnvelope`] to a [`TransactionReceipt`].
fn convert_receipts(receipt: ReceiptEnvelope) -> TransactionReceipt {
    let (transaction_type, receipt) = match receipt {
        ReceiptEnvelope::Legacy(r) => (TransactionType::Legacy, r.receipt),
        ReceiptEnvelope::Eip2930(r) => (TransactionType::AccessList, r.receipt),
        ReceiptEnvelope::Eip1559(r) => (TransactionType::DynamicFee, r.receipt),
        ReceiptEnvelope::Eip4844(r) => (TransactionType::Blob, r.receipt),
        ReceiptEnvelope::Eip7702(r) => (TransactionType::SetCode, r.receipt),
    };
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

/// Converts a [`BlockTuple`] to a [`Block`].
pub fn convert_block(block: &BlockTuple) -> Result<Block, Error> {
    let header: Header = block.header.decode()?;
    let transactions = block
        .body
        .decode::<BlockBody<EthereumTxEnvelope<TxEip4844Variant>>>()?
        .transactions
        .into_iter()
        .map(convert_transaction)
        .collect();
    let receipts = block
        .receipts
        .decode::<Vec<ReceiptEnvelope>>()?
        .into_iter()
        .map(convert_receipts)
        .collect();

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
        withdrawals: Vec::new(), // withdrawals don't exist pre-merge
        blob_gas_used: header.blob_gas_used,
        excess_blob_gas: header.excess_blob_gas,
        parent_beacon_block_root: header.parent_beacon_block_root.map(|r| r.0),
        requests_hash: header.requests_hash.map(|h| h.0),
        verkle_state_root: None,
        binary_state_root: None,
    })
}
