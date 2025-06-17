use serde::{Deserialize, Serialize};

use crate::{
    Address, AsHex, Transaction, U256,
    transaction::{AccessListEntry, TransactionError, TransactionType},
};

/// An Ethereum transaction for setting code in EOAs, as defined in [EIP-7702](https://eips.ethereum.org/EIPS/eip-7702).
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
    pub access_list: Vec<AccessListEntry>,
    pub authorization_list: Vec<SetCodeAuthorization>,

    #[serde(rename = "v")]
    pub y_parity: AsHex<U256>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

/// An authorization that specifies what code the signer wants to be executed in the context of
/// their EOA, used in EIP-7702 set code transactions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    from = "JsonRpcSetCodeAuthorization",
    into = "JsonRpcSetCodeAuthorization"
)]
pub struct SetCodeAuthorization {
    pub chain_id: U256,
    pub address: Address,
    pub nonce: u64,
    pub y_parity: u8,
    pub r: U256,
    pub s: U256,
}

impl SetCodeTx {
    /// Checks if the transaction can be converted to a [SetCodeTx].
    pub fn is_constructible_from(tx: &Transaction) -> Result<(), TransactionError> {
        if tx.transaction_type != TransactionType::SetCode {
            return Err(TransactionError::ConversionError(format!(
                "Expected {:?}, found {:?}",
                TransactionType::SetCode,
                tx.transaction_type
            )));
        }
        if tx.to.is_none() {
            return Err(TransactionError::ConversionError(
                "SetCode transaction requires 'to' field to be set".to_string(),
            ));
        }
        Ok(())
    }
}

/// The JSON-RPC representation of a [SetCodeAuthorization].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcSetCodeAuthorization {
    pub chain_id: AsHex<U256>,
    pub address: AsHex<Address>,
    pub nonce: AsHex<u64>,
    pub y_parity: AsHex<u8>,
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
        SetCodeTx::is_constructible_from(&tx)?;
        Ok(SetCodeTx {
            chain_id: AsHex(tx.chain_id),
            nonce: AsHex(tx.nonce),
            max_priority_fee_per_gas: AsHex(tx.max_priority_fee_per_gas),
            max_fee_per_gas: AsHex(tx.max_fee_per_gas),
            gas_limit: AsHex(tx.gas_limit),
            // Safe to unwrap as is_constructible_from checks for None
            to: tx.to.map(AsHex).unwrap(),
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
        assert!(matches!(error, TransactionError::ConversionError(_)));

        // Attempt to convert to SetCodeTx with to field set to None
        let error = SetCodeTx::try_from(Transaction {
            transaction_type: TransactionType::SetCode,
            to: None,
            ..Default::default()
        })
        .expect_err("Conversion to set code transaction must fail");
        assert!(matches!(error, TransactionError::ConversionError(_)));
    }

    #[test]
    fn is_constructible_from_returns_correct_value() {
        assert!(
            SetCodeTx::is_constructible_from(&Transaction {
                transaction_type: TransactionType::SetCode,
                to: Some(Address::default()),
                ..Default::default()
            })
            .is_ok(),
            "SetCodeTx should be constructible from a correct SetCodeTx"
        );
        // Mismatched transaction type
        let err = SetCodeTx::is_constructible_from(&Transaction {
            transaction_type: TransactionType::Legacy,
            to: Some(Address::default()),
            ..Default::default()
        })
        .expect_err(
            "SetCodeTx should not be constructible from a transaction with a mismatched type",
        );
        assert!(matches!(err, TransactionError::ConversionError(_)));
        // Missing 'to' field
        let err = SetCodeTx::is_constructible_from(&Transaction {
            transaction_type: TransactionType::SetCode,
            to: None,
            ..Default::default()
        })
        .expect_err(
            "SetCodeTx should not be constructible from a transaction with a missing 'to' field",
        );
        assert!(matches!(err, TransactionError::ConversionError(_)));
    }
}
