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

use alloy_consensus::{EthereumTxEnvelope, TxEip4844Variant, TxType};
use alloy_eips::{eip2930::AccessList, eip7702::SignedAuthorization};
use bertha_types::{AccessListEntry, SetCodeAuthorization, Transaction, TransactionType, U256};

/// Converts a [`TxType`] to a [`TransactionType`].
pub fn convert_tx_type(tx_type: TxType) -> TransactionType {
    match tx_type {
        TxType::Legacy => TransactionType::Legacy,
        TxType::Eip2930 => TransactionType::AccessList,
        TxType::Eip1559 => TransactionType::DynamicFee,
        TxType::Eip4844 => TransactionType::Blob,
        TxType::Eip7702 => TransactionType::SetCode,
    }
}

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
pub fn convert_transaction(transaction: EthereumTxEnvelope<TxEip4844Variant>) -> Transaction {
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
