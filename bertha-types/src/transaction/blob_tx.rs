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
use serde::Serialize;

use crate::{
    Address, AsHex, Hash, Transaction, U256,
    transaction::{AccessListEntry, RlpString, TransactionError, TransactionType},
};

/// A "blob-carrying" Ethereum transaction, as defined in [EIP-4844](https://eips.ethereum.org/EIPS/eip-4844).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, RlpEncodable, RlpDecodable)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlobTx {
    pub chain_id: AsHex<U256>,
    pub nonce: AsHex<u64>,
    pub max_priority_fee_per_gas: AsHex<U256>,
    pub max_fee_per_gas: AsHex<U256>,
    #[serde(rename = "gas")]
    pub gas_limit: AsHex<u64>,
    pub to: AsHex<Address>,
    pub value: AsHex<U256>,
    #[serde(rename = "input")]
    pub data: AsHex<RlpString>,
    pub access_list: Vec<AccessListEntry>,
    pub max_fee_per_blob_gas: AsHex<U256>,
    pub blob_versioned_hashes: Vec<AsHex<Hash>>,
    // sidecar is not included in the RLP encoding
    #[serde(rename = "v")]
    pub y_parity: AsHex<U256>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

impl BlobTx {
    /// Checks if the transaction can be converted to a [BlobTx].
    pub fn is_constructible_from(tx: &Transaction) -> Result<(), TransactionError> {
        if tx.transaction_type != TransactionType::Blob {
            return Err(TransactionError::ConversionError(format!(
                "expected {:?}, found {:?}",
                TransactionType::Blob,
                tx.transaction_type
            )));
        }
        if tx.to.is_none() {
            return Err(TransactionError::ConversionError(
                "Blob transaction requires 'to' field to be set".to_string(),
            ));
        }
        Ok(())
    }
}

impl TryFrom<Transaction> for BlobTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        BlobTx::is_constructible_from(&tx)?;
        Ok(BlobTx {
            chain_id: AsHex(tx.chain_id),
            nonce: AsHex(tx.nonce),
            max_priority_fee_per_gas: AsHex(tx.max_priority_fee_per_gas),
            max_fee_per_gas: AsHex(tx.max_fee_per_gas),
            gas_limit: AsHex(tx.gas_limit),
            // Safe to unwrap as is_constructible_from checks for None
            to: tx.to.map(AsHex).unwrap(),
            value: AsHex(tx.value),
            data: AsHex(RlpString(tx.data)),
            access_list: tx.access_list,
            max_fee_per_blob_gas: AsHex(tx.max_fee_per_blob_gas),
            blob_versioned_hashes: tx.blob_versioned_hashes.into_iter().map(AsHex).collect(),
            y_parity: AsHex(tx.y_parity),
            r: AsHex(tx.r),
            s: AsHex(tx.s),
        })
    }
}

impl From<BlobTx> for Transaction {
    fn from(tx: BlobTx) -> Self {
        Transaction {
            transaction_type: TransactionType::Blob,
            chain_id: tx.chain_id.0,
            nonce: tx.nonce.0,
            gas_price: U256::default(),
            gas_limit: tx.gas_limit.0,
            to: Some(tx.to.0),
            value: tx.value.0,
            data: tx.data.0.0,
            access_list: tx.access_list,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas.0,
            max_fee_per_gas: tx.max_fee_per_gas.0,
            max_fee_per_blob_gas: tx.max_fee_per_blob_gas.0,
            blob_versioned_hashes: tx.blob_versioned_hashes.into_iter().map(|h| h.0).collect(),
            authorization_list: Vec::new(),
            y_parity: tx.y_parity.0,
            r: tx.r.0,
            s: tx.s.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn can_be_converted_to_and_from_transaction() {
        let blob_tx = BlobTx::default();
        let transaction: Transaction = blob_tx.clone().into();
        let converted_back: BlobTx = transaction
            .try_into()
            .expect("Conversion to blob transaction must not fail");
        assert_eq!(blob_tx, converted_back);
    }

    #[test]
    fn conversion_to_blob_tx_fail_if_error_occurs() {
        // Attempt to convert to BlobTx with mismatched transaction type
        let error = BlobTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to blob transaction must fail");
        assert_matches!(error, TransactionError::ConversionError(_));

        // Attempt to convert to BlobTx with to field set to None
        let error = BlobTx::try_from(Transaction {
            transaction_type: TransactionType::Blob,
            to: None,
            ..Default::default()
        })
        .expect_err("Conversion to blob transaction must fail");
        assert_matches!(error, TransactionError::ConversionError(_));
    }

    #[test]
    fn is_constructible_from_returns_correct_value() {
        assert!(
            BlobTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::Blob,
                to: Some(Address::default()),
                ..Default::default()
            })
            .is_ok(),
            "BlobTx should be constructible from a correct blob transaction"
        );
        // Mismatched transaction type
        let err = BlobTx::is_constructible_from(&Transaction {
            transaction_type: TransactionType::Legacy,
            to: Some(Address::default()),
            ..Default::default()
        })
        .expect_err("BlobTx should not be constructible from a transaction with a mismatched type");
        assert_matches!(err, TransactionError::ConversionError(_));
        // Missing 'to' field
        let err = BlobTx::is_constructible_from(&Transaction {
            transaction_type: TransactionType::Blob,
            to: None,
            ..Default::default()
        })
        .expect_err(
            "BlobTx should not be constructible from a transaction with a missing 'to' field",
        );
        assert_matches!(err, TransactionError::ConversionError(_));
    }
}
