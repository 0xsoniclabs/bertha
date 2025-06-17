use serde::Serialize;

use crate::{
    Address, AsHex, Transaction, U256,
    transaction::{TransactionError, TransactionType},
};

/// The Legacy Ethereum transaction, defined in the EIP 2718.
/// Source: https://eips.ethereum.org/EIPS/eip-2718
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyTx {
    pub nonce: AsHex<u64>,
    pub gas_price: AsHex<U256>,
    #[serde(rename = "gas")]
    pub gas_limit: AsHex<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<AsHex<Address>>,
    pub value: AsHex<U256>,
    #[serde(rename = "input")]
    pub data: AsHex<Vec<u8>>,

    #[serde(rename = "v")]
    pub w: AsHex<U256>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

impl LegacyTx {
    /// Checks if the transaction can be converted to a Legacy transaction.
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::Legacy
    }
}

impl TryFrom<Transaction> for LegacyTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::Legacy {
            return Err(TransactionError::ConversionError(TransactionType::Legacy));
        }
        Ok(LegacyTx {
            nonce: AsHex(tx.nonce),
            gas_price: AsHex(tx.gas_price),
            gas_limit: AsHex(tx.gas_limit),
            to: tx.to.map(AsHex),
            value: AsHex(tx.value),
            data: AsHex(tx.data),
            w: AsHex(tx.y_parity),
            r: AsHex(tx.r),
            s: AsHex(tx.s),
        })
    }
}

impl From<LegacyTx> for Transaction {
    fn from(tx: LegacyTx) -> Self {
        Transaction {
            transaction_type: TransactionType::Legacy,
            nonce: tx.nonce.0,
            gas_price: tx.gas_price.0,
            gas_limit: tx.gas_limit.0,
            to: tx.to.map(|addr| addr.0),
            value: tx.value.0,
            data: tx.data.0,
            chain_id: U256::default(),
            max_priority_fee_per_gas: U256::default(),
            max_fee_per_gas: U256::default(),
            access_list: Vec::new(),
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: Vec::new(),
            y_parity: tx.w.0,
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

    #[test]
    fn is_constructible_from_returns_correct_value() {
        assert!(
            LegacyTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::Legacy,
                ..Default::default()
            }),
            "LegacyTx should be constructible from a correct Legacy transaction"
        );
        // Mismatched transaction type
        assert!(
            !LegacyTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::DynamicFee,
                ..Default::default()
            }),
            "LegacyTx should not be constructible from a transaction with a mismatched type"
        );
    }
}
