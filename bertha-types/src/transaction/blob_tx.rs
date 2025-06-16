use crate::{
    Address, Hash, Transaction, U256,
    transaction::{AccessTuple, TransactionError, TransactionType},
};

/// The Blob Ethereum transaction, defined in the EIP 4844.
/// Source: https://eips.ethereum.org/EIPS/eip-4844
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct BlobTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: u64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessTuple>,
    pub max_fee_per_blob_gas: U256,
    pub blob_versioned_hashes: Vec<Hash>,
    // sidecar is not included in the RLP encoding
    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

impl TryFrom<Transaction> for BlobTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::Blob {
            return Err(TransactionError::ConversionError(TransactionType::Blob));
        }

        Ok(BlobTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas_limit: tx.gas_limit,
            to: tx
                .to
                .ok_or(TransactionError::ConversionError(TransactionType::Blob))?,
            value: tx.value,
            data: tx.data,
            access_list: tx.access_list,
            max_fee_per_blob_gas: tx.max_fee_per_blob_gas,
            blob_versioned_hashes: tx.blob_versioned_hashes,
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
        })
    }
}

impl From<BlobTx> for Transaction {
    fn from(tx: BlobTx) -> Self {
        Transaction {
            transaction_type: TransactionType::Blob,
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            gas_price: U256::default(),
            gas_limit: tx.gas_limit,
            to: Some(tx.to),
            value: tx.value,
            data: tx.data,
            access_list: tx.access_list,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            max_fee_per_blob_gas: tx.max_fee_per_blob_gas,
            blob_versioned_hashes: tx.blob_versioned_hashes,
            authorization_list: Vec::new(),
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
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
}
