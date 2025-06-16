use crate::{
    Address, Transaction, U256,
    transaction::{TransactionError, TransactionType},
};

/// The Legacy Ethereum transaction, defined in the EIP 2718.
/// Source: https://eips.ethereum.org/EIPS/eip-2718
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct LegacyTx {
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub data: Vec<u8>,

    pub w: U256,
    pub r: U256,
    pub s: U256,
}

impl TryFrom<Transaction> for LegacyTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::Legacy {
            return Err(TransactionError::ConversionError(TransactionType::Legacy));
        }
        Ok(LegacyTx {
            nonce: tx.nonce,
            gas_price: tx.gas_price,
            gas_limit: tx.gas_limit,
            to: tx.to,
            value: tx.value,
            data: tx.data,
            w: tx.y_parity,
            r: tx.r,
            s: tx.s,
        })
    }
}

impl From<LegacyTx> for Transaction {
    fn from(tx: LegacyTx) -> Self {
        Transaction {
            transaction_type: TransactionType::Legacy,
            nonce: tx.nonce,
            gas_price: tx.gas_price,
            gas_limit: tx.gas_limit,
            to: tx.to,
            value: tx.value,
            data: tx.data,
            chain_id: U256::default(),
            max_priority_fee_per_gas: U256::default(),
            max_fee_per_gas: U256::default(),
            access_list: Vec::new(),
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: Vec::new(),
            y_parity: tx.w,
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
        let legacy_tx = LegacyTx::default();
        let transaction: Transaction = legacy_tx.clone().into();
        let converted_back: LegacyTx = transaction
            .try_into()
            .expect("Conversion to legacy transaction must not fail");
        assert_eq!(legacy_tx, converted_back);
    }

    #[test]
    fn conversion_to_legacy_tx_fail_if_error_occurs() {
        // Attempt to convert to LegacyTx with mismatched transaction type
        let error = LegacyTx::try_from(Transaction {
            transaction_type: TransactionType::DynamicFee,
            ..Default::default()
        })
        .expect_err("Conversion to legacy transaction must fail");
        assert_eq!(
            error,
            TransactionError::ConversionError(TransactionType::Legacy)
        );
    }
}
