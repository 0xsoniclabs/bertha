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
use serde::{Deserialize, Serialize};

use crate::{
    Address, AsHex, Hash, Transaction, U256,
    transaction::{RlpNil, RlpString, TransactionError, TransactionType},
};

/// An Ethereum transaction with an optional access list, as defined in [EIP-2930](https://eips.ethereum.org/EIPS/eip-2930).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, RlpEncodable, RlpDecodable)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccessListTx {
    pub chain_id: AsHex<U256>,
    pub nonce: AsHex<u64>,
    pub gas_price: AsHex<U256>,
    #[serde(rename = "gas")]
    pub gas_limit: AsHex<u64>,
    #[serde(skip_serializing_if = "RlpNil::is_none")]
    pub to: RlpNil<AsHex<Address>>,
    pub value: AsHex<U256>,
    #[serde(rename = "input")]
    pub data: AsHex<RlpString>,
    pub access_list: Vec<AccessListEntry>,

    #[serde(rename = "v")]
    pub y_parity: AsHex<U256>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

/// An entry in the EIP-2930 access list.
/// It contains the address and a list of storage keys that the transaction plans to access.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, RlpEncodable, RlpDecodable,
)]
#[serde(from = "JsonRpcAccessListEntry", into = "JsonRpcAccessListEntry")]
pub struct AccessListEntry {
    pub address: Address,
    pub storage_keys: Vec<Hash>,
}

/// The JSON-RPC representation of an [AccessListEntry].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcAccessListEntry {
    pub address: AsHex<Address>,
    pub storage_keys: Vec<AsHex<Hash>>,
}

impl From<JsonRpcAccessListEntry> for AccessListEntry {
    fn from(value: JsonRpcAccessListEntry) -> Self {
        AccessListEntry {
            address: value.address.0,
            storage_keys: value.storage_keys.into_iter().map(|h| h.0).collect(),
        }
    }
}

impl From<AccessListEntry> for JsonRpcAccessListEntry {
    fn from(value: AccessListEntry) -> Self {
        JsonRpcAccessListEntry {
            address: AsHex(value.address),
            storage_keys: value.storage_keys.into_iter().map(AsHex).collect(),
        }
    }
}

impl AccessListTx {
    /// Checks if the transaction can be converted to an [AccessListTx].
    pub fn is_constructible_from(tx: &Transaction) -> Result<(), TransactionError> {
        if tx.transaction_type != TransactionType::AccessList {
            return Err(TransactionError::ConversionError(format!(
                "expected {:?}, found {:?}",
                TransactionType::AccessList,
                tx.transaction_type
            )));
        }
        Ok(())
    }
}

impl TryFrom<Transaction> for AccessListTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        AccessListTx::is_constructible_from(&tx)?;
        Ok(AccessListTx {
            chain_id: AsHex(tx.chain_id),
            nonce: AsHex(tx.nonce),
            gas_price: AsHex(tx.gas_price),
            gas_limit: AsHex(tx.gas_limit),
            to: RlpNil(tx.to.map(AsHex)),
            value: AsHex(tx.value),
            data: AsHex(RlpString(tx.data)),
            access_list: tx.access_list,
            y_parity: AsHex(tx.y_parity),
            r: AsHex(tx.r),
            s: AsHex(tx.s),
        })
    }
}

impl From<AccessListTx> for Transaction {
    fn from(tx: AccessListTx) -> Self {
        Transaction {
            transaction_type: TransactionType::AccessList,
            chain_id: tx.chain_id.0,
            nonce: tx.nonce.0,
            gas_price: tx.gas_price.0,
            gas_limit: tx.gas_limit.0,
            to: tx.to.0.map(|to| to.0),
            value: tx.value.0,
            data: tx.data.0.0,
            access_list: tx.access_list,
            max_priority_fee_per_gas: U256::default(),
            max_fee_per_gas: U256::default(),
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: Vec::new(),
            y_parity: tx.y_parity.0,
            r: tx.r.0,
            s: tx.s.0,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_be_converted_to_and_from_transaction() {
        let access_list_tx = AccessListTx::default();
        let transaction: Transaction = access_list_tx.clone().into();
        let converted_back: AccessListTx = transaction
            .try_into()
            .expect("Conversion to access list transaction must not fail");
        assert_eq!(access_list_tx, converted_back);
    }

    #[test]
    fn conversion_to_access_list_tx_fail_if_error_occurs() {
        // Attempt to convert to AccessListTx with mismatched transaction type
        let error = AccessListTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to access list transaction must fail");
        assert!(matches!(error, TransactionError::ConversionError(_)));
    }

    #[test]
    fn is_constructible_from_returns_correct_value() {
        assert!(
            AccessListTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::AccessList,
                ..Default::default()
            })
            .is_ok(),
            "AccessListTx should be constructible from a correct access list transaction"
        );
        // Mismatched transaction type
        let err = AccessListTx::is_constructible_from(&Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err(
            "AccessListTx should not be constructible from a transaction with a mismatched type",
        );
        assert!(matches!(err, TransactionError::ConversionError(_)));
    }
}
