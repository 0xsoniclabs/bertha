use crate::{
    Address, Transaction, U256,
    transaction::{AccessTuple, TransactionError, TransactionType},
};

// The Dynamic Fee Ethereum transaction, defined in the EIP 1559.
// Source: https://eips.ethereum.org/EIPS/eip-1559
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct DynamicFeeTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessTuple>,

    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

impl TryFrom<Transaction> for DynamicFeeTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::DynamicFee {
            return Err(TransactionError::ConversionError(
                TransactionType::DynamicFee,
            ));
        }
        Ok(DynamicFeeTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas_limit: tx.gas_limit,
            to: tx.to,
            value: tx.value,
            data: tx.data,
            access_list: tx.access_list,
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
        })
    }
}

impl From<DynamicFeeTx> for Transaction {
    fn from(tx: DynamicFeeTx) -> Self {
        Transaction {
            transaction_type: TransactionType::DynamicFee,
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            gas_price: U256::default(),
            gas_limit: tx.gas_limit,
            to: tx.to,
            value: tx.value,
            data: tx.data,
            access_list: tx.access_list,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
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
        assert_eq!(
            error,
            TransactionError::ConversionError(TransactionType::DynamicFee)
        );
    }
}
