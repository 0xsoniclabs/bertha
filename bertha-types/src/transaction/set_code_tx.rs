use crate::{
    Address, Transaction, U256,
    transaction::{AccessTuple, TransactionError, TransactionType},
};

/// The SetCode Ethereum transaction, defined in the EIP 7702.
/// Source: https://eips.ethereum.org/EIPS/eip-7702
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct SetCodeTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas_limit: u64,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessTuple>,
    pub authorization_list: Vec<SetCodeAuthorization>,

    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

/// The Authorization list internal values, used in SetCodeTx.
/// It indicates what code the signer desires to execute in the context of their EOA.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SetCodeAuthorization {
    pub chain_id: U256,
    pub address: Address,
    pub nonce: u64,
    pub y_parity: u8,
    pub r: U256,
    pub s: U256,
}

impl TryFrom<Transaction> for SetCodeTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::SetCode {
            return Err(TransactionError::ConversionError(TransactionType::SetCode));
        }

        Ok(SetCodeTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas_limit: tx.gas_limit,
            to: tx
                .to
                .ok_or(TransactionError::ConversionError(TransactionType::SetCode))?,
            value: tx.value,
            data: tx.data,
            access_list: tx.access_list,
            authorization_list: tx.authorization_list,
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
        })
    }
}

impl From<SetCodeTx> for Transaction {
    fn from(tx: SetCodeTx) -> Self {
        Transaction {
            transaction_type: TransactionType::SetCode,
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            gas_limit: tx.gas_limit,
            gas_price: U256::default(),
            to: Some(tx.to),
            value: tx.value,
            data: tx.data,
            access_list: tx.access_list,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: tx.authorization_list,
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
        let set_code_tx = SetCodeTx::default();
        let transaction: Transaction = set_code_tx.clone().into();
        let converted_back: SetCodeTx = transaction
            .try_into()
            .expect("Conversion to set code transaction must not fail");
        assert_eq!(set_code_tx, converted_back);
    }

    #[test]
    fn conversion_to_set_code_tx_fail_if_error_occurs() {
        // Attempt to convert to SetCodeTx with mismatched transaction type
        let error = SetCodeTx::try_from(Transaction {
            transaction_type: TransactionType::Blob,
            ..Default::default()
        })
        .expect_err("Conversion to set code transaction must fail");
        assert_eq!(
            error,
            TransactionError::ConversionError(TransactionType::SetCode)
        );

        // Attempt to convert to SetCodeTx with to field set to None
        let error = SetCodeTx::try_from(Transaction {
            transaction_type: TransactionType::SetCode,
            to: None,
            ..Default::default()
        })
        .expect_err("Conversion to set code transaction must fail");
        assert_eq!(
            error,
            TransactionError::ConversionError(TransactionType::SetCode)
        );
    }
}
