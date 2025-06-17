use serde::Serialize;

use crate::{
    Address, AsHex, Transaction, U256,
    transaction::{AccessListEntry, TransactionError, TransactionType},
};

// The Dynamic Fee Ethereum transaction, defined in the EIP 1559.
// Source: https://eips.ethereum.org/EIPS/eip-1559
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DynamicFeeTx {
    pub chain_id: AsHex<U256>,
    pub nonce: AsHex<u64>,
    pub max_priority_fee_per_gas: AsHex<U256>,
    pub max_fee_per_gas: AsHex<U256>,
    #[serde(rename = "gas")]
    pub gas_limit: AsHex<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<AsHex<Address>>,
    pub value: AsHex<U256>,
    #[serde(rename = "input")]
    pub data: AsHex<Vec<u8>>,
    pub access_list: Vec<AccessListEntry>,

    #[serde(rename = "v")]
    pub y_parity: AsHex<U256>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

impl DynamicFeeTx {
    /// Checks if the transaction can be converted to a DynamicFee transaction.
    pub fn is_constructible_from(tx: &Transaction) -> Result<(), TransactionError> {
        if tx.transaction_type != TransactionType::DynamicFee {
            return Err(TransactionError::ConversionError(format!(
                "Expected DynamicFee transaction type, found {:?}",
                tx.transaction_type
            )));
        }
        Ok(())
    }
}

impl TryFrom<Transaction> for DynamicFeeTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        DynamicFeeTx::is_constructible_from(&tx)?;
        Ok(DynamicFeeTx {
            chain_id: AsHex(tx.chain_id),
            nonce: AsHex(tx.nonce),
            max_priority_fee_per_gas: AsHex(tx.max_priority_fee_per_gas),
            max_fee_per_gas: AsHex(tx.max_fee_per_gas),
            gas_limit: AsHex(tx.gas_limit),
            to: tx.to.map(AsHex),
            value: AsHex(tx.value),
            data: AsHex(tx.data),
            access_list: tx.access_list,
            y_parity: AsHex(tx.y_parity),
            r: AsHex(tx.r),
            s: AsHex(tx.s),
        })
    }
}

impl From<DynamicFeeTx> for Transaction {
    fn from(tx: DynamicFeeTx) -> Self {
        Transaction {
            transaction_type: TransactionType::DynamicFee,
            chain_id: tx.chain_id.0,
            nonce: tx.nonce.0,
            gas_price: U256::default(),
            gas_limit: tx.gas_limit.0,
            to: tx.to.map(|addr| addr.0),
            value: tx.value.0,
            data: tx.data.0,
            access_list: tx.access_list,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas.0,
            max_fee_per_gas: tx.max_fee_per_gas.0,
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
        let dynamic_fee_tx = DynamicFeeTx::default();
        let transaction: Transaction = dynamic_fee_tx.clone().into();
        let converted_back: DynamicFeeTx = transaction
            .try_into()
            .expect("Conversion to dynamic fee transaction must not fail");
        assert_eq!(dynamic_fee_tx, converted_back);
    }

    #[test]
    fn conversion_to_dynamic_fee_tx_fail_if_error_occurs() {
        // Attempt to convert to DynamicFeeTx with mismatched transaction type
        let error = DynamicFeeTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to dynamic fee transaction must fail");
        assert!(matches!(error, TransactionError::ConversionError(_)));
    }

    #[test]
    fn is_constructible_from_returns_correct_value() {
        assert!(
            DynamicFeeTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::DynamicFee,
                ..Default::default()
            })
            .is_ok(),
            "DynamicFeeTx should be constructible from a correct DynamicFee transaction"
        );
        // Mismatched transaction type
        let err = DynamicFeeTx::is_constructible_from(&Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err(
            "DynamicFeeTx should not be constructible from a transaction with a mismatched type",
        );
        assert!(matches!(err, TransactionError::ConversionError(_)));
    }
}
