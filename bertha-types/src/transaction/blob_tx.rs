use serde::Serialize;

use crate::{
    Address, AsHex, Hash, Transaction, U256,
    transaction::{AccessListEntry, TransactionError, TransactionType},
};

/// The Blob Ethereum transaction, defined in the EIP 4844.
/// Source: https://eips.ethereum.org/EIPS/eip-4844
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
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
    pub data: AsHex<Vec<u8>>,
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
    /// Checks if the transaction can be converted to a Blob transaction.
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::Blob && tx.to.is_some()
    }
}

impl TryFrom<Transaction> for BlobTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::Blob {
            return Err(TransactionError::ConversionError(TransactionType::Blob));
        }

        Ok(BlobTx {
            chain_id: AsHex(tx.chain_id),
            nonce: AsHex(tx.nonce),
            max_priority_fee_per_gas: AsHex(tx.max_priority_fee_per_gas),
            max_fee_per_gas: AsHex(tx.max_fee_per_gas),
            gas_limit: AsHex(tx.gas_limit),
            to: tx
                .to
                .map(AsHex)
                .ok_or(TransactionError::ConversionError(TransactionType::Blob))?,
            value: AsHex(tx.value),
            data: AsHex(tx.data),
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
            data: tx.data.0,
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
        assert_eq!(
            error,
            TransactionError::ConversionError(TransactionType::Blob)
        );

        // Attempt to convert to BlobTx with to field set to None
        let error = BlobTx::try_from(Transaction {
            transaction_type: TransactionType::Blob,
            to: None,
            ..Default::default()
        })
        .expect_err("Conversion to blob transaction must fail");
        assert_eq!(
            error,
            TransactionError::ConversionError(TransactionType::Blob)
        );
    }

    #[test]
    fn is_constructible_from_returns_correct_value() {
        assert!(
            BlobTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::Blob,
                to: Some(Address::default()),
                ..Default::default()
            }),
            "BlobTx should be constructible from a correct Blob transaction"
        );
        // Mismatched transaction type
        assert!(
            !BlobTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::Legacy,
                ..Default::default()
            }),
            "BlobTx should not be constructible from a transaction with a mismatched type"
        );
        // Missing 'to' field
        assert!(
            !BlobTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::Blob,
                to: None,
                ..Default::default()
            }),
            "BlobTx should not be constructible from a transaction with missing 'to' field"
        );
    }
}
