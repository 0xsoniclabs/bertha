use serde::{Deserialize, Serialize};

use crate::{
    Address, AsHex, Transaction, U256,
    transaction::{AccessTuple, TransactionError, TransactionType},
};

/// The SetCode Ethereum transaction, defined in the EIP 7702.
/// Source: https://eips.ethereum.org/EIPS/eip-7702
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetCodeTx {
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
    pub access_list: Vec<AccessTuple>,
    pub authorization_list: Vec<SetCodeAuthorization>,

    #[serde(rename = "v")]
    pub y_parity: AsHex<U256>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

/// The Authorization list internal values, used in SetCodeTx.
/// It indicates what code the signer desires to execute in the context of their EOA.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    from = "JsonRpcSetCodeAuthorization",
    into = "JsonRpcSetCodeAuthorization"
)]
pub struct SetCodeAuthorization {
    pub chain_id: U256,
    pub address: Address,
    pub nonce: u64,
    pub y_parity: u64,
    pub r: U256,
    pub s: U256,
}

impl SetCodeTx {
    /// A function to check if the transaction can be converted to a SetCode transaction.
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::SetCode && tx.to.is_some()
    }
}

/// A JSON-RPC representation of a SetCode transaction authorization.\
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcSetCodeAuthorization {
    pub chain_id: AsHex<U256>,
    pub address: AsHex<Address>,
    pub nonce: AsHex<u64>,
    pub y_parity: AsHex<u64>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

impl From<JsonRpcSetCodeAuthorization> for SetCodeAuthorization {
    fn from(value: JsonRpcSetCodeAuthorization) -> Self {
        SetCodeAuthorization {
            chain_id: value.chain_id.0,
            address: value.address.0,
            nonce: value.nonce.0,
            y_parity: value.y_parity.0,
            r: value.r.0,
            s: value.s.0,
        }
    }
}

impl From<SetCodeAuthorization> for JsonRpcSetCodeAuthorization {
    fn from(value: SetCodeAuthorization) -> Self {
        JsonRpcSetCodeAuthorization {
            chain_id: AsHex(value.chain_id),
            address: AsHex(value.address),
            nonce: AsHex(value.nonce),
            y_parity: AsHex(value.y_parity),
            r: AsHex(value.r),
            s: AsHex(value.s),
        }
    }
}

impl TryFrom<Transaction> for SetCodeTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::SetCode {
            return Err(TransactionError::ConversionError(TransactionType::SetCode));
        }

        Ok(SetCodeTx {
            chain_id: AsHex(tx.chain_id),
            nonce: AsHex(tx.nonce),
            max_priority_fee_per_gas: AsHex(tx.max_priority_fee_per_gas),
            max_fee_per_gas: AsHex(tx.max_fee_per_gas),
            gas_limit: AsHex(tx.gas_limit),
            to: tx
                .to
                .map(AsHex)
                .ok_or(TransactionError::ConversionError(TransactionType::SetCode))?,
            value: AsHex(tx.value),
            data: AsHex(tx.data),
            access_list: tx.access_list,
            authorization_list: tx.authorization_list,
            y_parity: AsHex(tx.y_parity),
            r: AsHex(tx.r),
            s: AsHex(tx.s),
        })
    }
}

impl From<SetCodeTx> for Transaction {
    fn from(tx: SetCodeTx) -> Self {
        Transaction {
            transaction_type: TransactionType::SetCode,
            chain_id: tx.chain_id.0,
            nonce: tx.nonce.0,
            gas_limit: tx.gas_limit.0,
            gas_price: U256::default(),
            to: Some(tx.to.0),
            value: tx.value.0,
            data: tx.data.0,
            access_list: tx.access_list,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas.0,
            max_fee_per_gas: tx.max_fee_per_gas.0,
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: tx.authorization_list,
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

    #[test]
    fn is_constructible_from_returns_correct_value() {
        assert!(
            SetCodeTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::SetCode,
                to: Some(Address::default()),
                ..Default::default()
            }),
            "SetCodeTx should be constructible from a correct SetCode transaction"
        );
        // Mismatched transaction type
        assert!(
            !SetCodeTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::Legacy,
                to: Some(Address::default()),
                ..Default::default()
            }),
            "SetCodeTx should not be constructible from a transaction with mismatched type"
        );
        // Missing 'to' field
        assert!(
            !SetCodeTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::SetCode,
                to: None,
                ..Default::default()
            }),
            "SetCodeTx should not be constructible from a transaction with missing 'to' field"
        );
    }
}
