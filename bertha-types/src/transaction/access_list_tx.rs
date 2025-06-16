use crate::{
    Address, Hash, Transaction, U256,
    transaction::{TransactionError, TransactionType},
};

/// The Access List Ethereum transaction, defined in the EIP 2930.
/// Source: https://eips.ethereum.org/EIPS/eip-2930
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct AccessListTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub gas_price: U256,
    pub gas_limit: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub data: Vec<u8>,
    pub access_list: Vec<AccessTuple>,

    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

/// The Access List internal values, used in AccessListTx.
/// It contains the address and a list of storage keys that the transaction plans to access.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct AccessTuple {
    pub address: Address,
    pub storage_keys: Vec<Hash>,
}

impl TryFrom<Transaction> for AccessListTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::AccessList {
            return Err(TransactionError::ConversionError(
                TransactionType::AccessList,
            ));
        }
        Ok(AccessListTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            gas_price: tx.gas_price,
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

impl From<AccessListTx> for Transaction {
    fn from(tx: AccessListTx) -> Self {
        Transaction {
            transaction_type: TransactionType::AccessList,
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            gas_price: tx.gas_price,
            gas_limit: tx.gas_limit,
            to: tx.to,
            value: tx.value,
            data: tx.data,
            access_list: tx.access_list,
            max_priority_fee_per_gas: U256::default(),
            max_fee_per_gas: U256::default(),
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
        assert_eq!(
            error,
            TransactionError::ConversionError(TransactionType::AccessList)
        );
    }
}
