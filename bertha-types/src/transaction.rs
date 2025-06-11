use std::fmt::Display;
use thiserror::Error;

use crate::{Address, Hash, U256};

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("Couldn't convert transaction. Expected type {0}, found {1}")]
    TransactionTypeMismatch(TransactionType, TransactionType),
    #[error("Couldn't construct transaction {0}")]
    InvalidTransactionError(TransactionType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TransactionType {
    #[default]
    Legacy = 0,
    AccessList = 1,
    DynamicFee = 2,
    Blob = 3,
    SetCode = 4,
}

impl Display for TransactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionType::Legacy => write!(f, "LegacyTx"),
            TransactionType::DynamicFee => write!(f, "DynamicFeeTx"),
            TransactionType::AccessList => write!(f, "AccessListTx"),
            TransactionType::Blob => write!(f, "BlobTx"),
            TransactionType::SetCode => write!(f, "SetCodeTx"),
        }
    }
}

// Source: go-ethereum/core/types/tx_legacy.go (LegacyTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LegacyTx {
    pub nonce: u64,
    pub gas_price: U256,
    pub gas: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub input: Vec<u8>,

    pub v: U256,
    pub r: U256,
    pub s: U256,
}

impl LegacyTx {
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::Legacy
    }
}

// Source: go-ethereum/core/types/tx_access_list.go (AccessListTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct AccessListTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub gas_price: U256,
    pub gas: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub input: Vec<u8>,
    pub access_list: Vec<AccessTuple>,

    pub v: U256,
    pub r: U256,
    pub s: U256,
}
// Source: go-ethereum/core/types/tx_access_list.go (AccessTuple)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct AccessTuple {
    pub address: Address,
    pub storage_keys: Vec<Hash>,
}

impl AccessListTx {
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::AccessList
    }
}

// Source: go-ethereum/core/types/tx_dynamic_fee.go (DynamicFeeTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct DynamicFeeTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub input: Vec<u8>,
    pub access_list: Vec<AccessTuple>,

    pub v: U256,
    pub r: U256,
    pub s: U256,
}

impl DynamicFeeTx {
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::DynamicFee
    }
}

// Source: go-ethereum/core/types/tx_blob.go (BlobTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct BlobTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas: u64,
    pub to: Address,
    pub value: U256,
    pub input: Vec<u8>,
    pub access_list: Vec<AccessTuple>,
    pub max_fee_per_blob_gas: U256,
    pub blob_versioned_hashes: Vec<Hash>,
    // sidecar is not included in the RLP encoding
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

impl BlobTx {
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::Blob && tx.to.is_some()
    }
}

// Source: go-ethereum/core/types/tx_set_code.go (SetCodeTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SetCodeTx {
    pub chain_id: U256,
    pub nonce: u64,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub gas: u64,
    pub to: Address,
    pub value: U256,
    pub input: Vec<u8>,
    pub access_list: Vec<AccessTuple>,
    pub authorization_list: Vec<SetCodeAuthorization>,

    pub v: U256,
    pub r: U256,
    pub s: U256,
}

// Source: go-ethereum/core/types/tx_set_code.go (SetCodeAuthorization)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct SetCodeAuthorization {
    pub chain_id: U256,
    pub address: Address,
    pub nonce: u64,
    pub y_parity: u8, /* TODO: this is just one or zero, can be represented with
                       * one byte only */
    pub r: U256,
    pub s: U256,
}

impl SetCodeTx {
    pub fn is_constructible_from(tx: &Transaction) -> bool {
        tx.transaction_type == TransactionType::SetCode && tx.to.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transaction {
    pub transaction_type: TransactionType,
    pub transaction_index: u64,
    pub nonce: u64,
    pub gas_price: U256,
    pub gas: u64,
    // #[serde(default)]
    pub to: Option<Address>,
    pub value: U256,
    pub input: Vec<u8>,
    pub v: U256,
    pub r: U256,
    pub s: U256,
    pub chain_id: U256,
    pub max_priority_fee_per_gas: U256,
    pub max_fee_per_gas: U256,
    pub access_list: Vec<AccessTuple>,
    pub max_fee_per_blob_gas: U256,
    pub blob_versioned_hashes: Vec<Hash>,
    pub authorization_list: Vec<SetCodeAuthorization>,
}

impl Transaction {
    pub fn is_valid(&self) -> bool {
        match self.transaction_type {
            TransactionType::Legacy => LegacyTx::is_constructible_from(self),
            TransactionType::DynamicFee => DynamicFeeTx::is_constructible_from(self),
            TransactionType::AccessList => AccessListTx::is_constructible_from(self),
            TransactionType::Blob => BlobTx::is_constructible_from(self),
            TransactionType::SetCode => SetCodeTx::is_constructible_from(self),
        }
    }
}

impl TryFrom<Transaction> for LegacyTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::Legacy {
            return Err(TransactionError::TransactionTypeMismatch(
                TransactionType::Legacy,
                tx.transaction_type,
            ));
        }
        Ok(LegacyTx {
            nonce: tx.nonce,
            gas_price: tx.gas_price,
            gas: tx.gas,
            to: tx.to,
            value: tx.value,
            input: tx.input,
            v: tx.v,
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
            gas: tx.gas,
            to: tx.to,
            value: tx.value,
            input: tx.input,
            v: tx.v,
            r: tx.r,
            s: tx.s,
            chain_id: U256::default(),
            max_priority_fee_per_gas: U256::default(),
            transaction_index: u64::default(),
            max_fee_per_gas: U256::default(),
            access_list: Vec::new(),
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: Vec::new(),
        }
    }
}

impl TryFrom<Transaction> for DynamicFeeTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::DynamicFee {
            return Err(TransactionError::TransactionTypeMismatch(
                TransactionType::DynamicFee,
                tx.transaction_type,
            ));
        }
        Ok(DynamicFeeTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas: tx.gas,
            to: tx.to,
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            v: tx.v,
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
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas: tx.gas,
            to: tx.to,
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            v: tx.v,
            r: tx.r,
            s: tx.s,
            gas_price: U256::default(),
            transaction_index: u64::default(),
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: Vec::new(),
        }
    }
}

impl TryFrom<Transaction> for AccessListTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::AccessList {
            return Err(TransactionError::TransactionTypeMismatch(
                TransactionType::AccessList,
                tx.transaction_type,
            ));
        }
        Ok(AccessListTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            gas_price: tx.gas_price,
            gas: tx.gas,
            to: tx.to,
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            v: tx.v,
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
            gas: tx.gas,
            to: tx.to,
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            v: tx.v,
            r: tx.r,
            s: tx.s,
            max_priority_fee_per_gas: U256::default(),
            max_fee_per_gas: U256::default(),
            transaction_index: u64::default(),
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: Vec::new(),
        }
    }
}

impl TryFrom<Transaction> for BlobTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::Blob {
            return Err(TransactionError::TransactionTypeMismatch(
                TransactionType::Blob,
                tx.transaction_type,
            ));
        }

        Ok(BlobTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas: tx.gas,
            to: tx.to.ok_or(TransactionError::InvalidTransactionError(
                TransactionType::Blob,
            ))?,
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            max_fee_per_blob_gas: tx.max_fee_per_blob_gas,
            blob_versioned_hashes: tx.blob_versioned_hashes,
            v: tx.v,
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
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas: tx.gas,
            to: Some(tx.to),
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            max_fee_per_blob_gas: tx.max_fee_per_blob_gas,
            blob_versioned_hashes: tx.blob_versioned_hashes,
            v: tx.v,
            r: tx.r,
            s: tx.s,
            gas_price: U256::default(),
            transaction_index: u64::default(),
            authorization_list: Vec::new(),
        }
    }
}

impl TryFrom<Transaction> for SetCodeTx {
    type Error = TransactionError;

    fn try_from(tx: Transaction) -> Result<Self, Self::Error> {
        if tx.transaction_type != TransactionType::SetCode {
            return Err(TransactionError::TransactionTypeMismatch(
                TransactionType::SetCode,
                tx.transaction_type,
            ));
        }

        Ok(SetCodeTx {
            chain_id: tx.chain_id,
            nonce: tx.nonce,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas: tx.gas,
            to: tx.to.ok_or(TransactionError::InvalidTransactionError(
                TransactionType::SetCode,
            ))?,
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            authorization_list: tx.authorization_list,
            v: tx.v,
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
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            gas: tx.gas,
            to: Some(tx.to),
            value: tx.value,
            input: tx.input,
            access_list: tx.access_list,
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            v: tx.v,
            r: tx.r,
            s: tx.s,
            gas_price: U256::default(),
            transaction_index: u64::default(),
            authorization_list: tx.authorization_list,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::transaction;

    use super::*;

    #[test]
    fn transaction_type_display_prints_correct_name() {
        assert_eq!(TransactionType::Legacy.to_string(), "LegacyTx");
        assert_eq!(TransactionType::AccessList.to_string(), "AccessListTx");
        assert_eq!(TransactionType::DynamicFee.to_string(), "DynamicFeeTx");
        assert_eq!(TransactionType::Blob.to_string(), "BlobTx");
        assert_eq!(TransactionType::SetCode.to_string(), "SetCodeTx");
    }

    #[test]
    fn can_be_converted_to_and_from_inner_transaction_types() {
        // Legacy transaction
        let legacy_tx = LegacyTx::default();
        let transaction: Transaction = legacy_tx.clone().into();
        let converted_back: LegacyTx = transaction
            .try_into()
            .expect("Conversion to legacy transaction must not fail");
        assert_eq!(legacy_tx, converted_back);

        // AccessList transaction
        let access_list_tx = AccessListTx::default();
        let transaction: Transaction = access_list_tx.clone().into();
        let converted_back: AccessListTx = transaction
            .try_into()
            .expect("Conversion to access list transaction must not fail");
        assert_eq!(access_list_tx, converted_back);

        // DynamicFee transaction
        let dynamic_fee_tx = DynamicFeeTx::default();
        let transaction: Transaction = dynamic_fee_tx.clone().into();
        let converted_back: DynamicFeeTx = transaction
            .try_into()
            .expect("Conversion to dynamic fee transaction must not fail");
        assert_eq!(dynamic_fee_tx, converted_back);

        // Blob transaction
        let blob_tx = BlobTx::default();
        let transaction: Transaction = blob_tx.clone().into();
        let converted_back: BlobTx = transaction
            .try_into()
            .expect("Conversion to blob transaction must not fail");
        assert_eq!(blob_tx, converted_back);

        // SetCode transaction
        let set_code_tx = SetCodeTx::default();
        let transaction: Transaction = set_code_tx.clone().into();
        let converted_back: SetCodeTx = transaction
            .try_into()
            .expect("Conversion to set code transaction must not fail");
        assert_eq!(set_code_tx, converted_back);
    }

    #[test]
    fn conversion_to_inner_types_fail_if_error_occurs() {
        // Attempt to convert to LegacyTx with mismatched transaction type
        let error = LegacyTx::try_from(transaction::Transaction {
            transaction_type: TransactionType::DynamicFee,
            ..Default::default()
        })
        .expect_err("Conversion to legacy transaction must fail");
        assert!(matches!(
            error,
            TransactionError::TransactionTypeMismatch(
                TransactionType::Legacy,
                TransactionType::DynamicFee
            )
        ));

        // Attempt to convert to DynamicFeeTx with mismatched transaction type
        let error = DynamicFeeTx::try_from(transaction::Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to dynamic fee transaction must fail");
        assert!(matches!(
            error,
            TransactionError::TransactionTypeMismatch(
                TransactionType::DynamicFee,
                TransactionType::Legacy
            )
        ));

        // Attempt to convert to AccessListTx with mismatched transaction type
        let error = AccessListTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to access list transaction must fail");
        assert!(matches!(
            error,
            TransactionError::TransactionTypeMismatch(
                TransactionType::AccessList,
                TransactionType::Legacy
            )
        ));

        // Attempt to convert to BlobTx with mismatched transaction type
        let error = BlobTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to blob transaction must fail");
        assert!(matches!(
            error,
            TransactionError::TransactionTypeMismatch(
                TransactionType::Blob,
                TransactionType::Legacy
            )
        ));

        // Attempt to convert to BlobTx with to field set to None
        let error = BlobTx::try_from(Transaction {
            transaction_type: TransactionType::Blob,
            to: None,
            ..Default::default()
        })
        .expect_err("Conversion to blob transaction must fail");
        assert!(matches!(
            error,
            TransactionError::InvalidTransactionError(TransactionType::Blob)
        ));

        // Attempt to convert to SetCodeTx with mismatched transaction type
        let error = SetCodeTx::try_from(Transaction {
            transaction_type: TransactionType::Blob,
            ..Default::default()
        })
        .expect_err("Conversion to set code transaction must fail");
        assert!(matches!(
            error,
            TransactionError::TransactionTypeMismatch(
                TransactionType::SetCode,
                TransactionType::Blob
            )
        ));

        // Attempt to convert to SetCodeTx with to field set to None
        let error = SetCodeTx::try_from(Transaction {
            transaction_type: TransactionType::SetCode,
            to: None,
            ..Default::default()
        })
        .expect_err("Conversion to set code transaction must fail");
        assert!(matches!(
            error,
            TransactionError::InvalidTransactionError(TransactionType::SetCode)
        ));
    }

    #[test]
    fn is_valid_correctly_check_transaction() {
        // Valid Legacy transactions
        let legacy_tx = Transaction {
            transaction_type: TransactionType::Legacy,
            to: None,
            ..Default::default()
        };
        assert!(
            legacy_tx.is_valid(),
            "Legacy transaction without to field should be valid"
        );
        let legacy_tx_with_to = Transaction {
            to: Some(Address::default()),
            ..legacy_tx.clone()
        };
        assert!(
            legacy_tx_with_to.is_valid(),
            "Legacy transaction with to field should be valid"
        );

        // Valid AccessList transactions
        let access_list_tx = Transaction {
            transaction_type: TransactionType::AccessList,
            to: None,
            ..Default::default()
        };
        assert!(
            access_list_tx.is_valid(),
            "AccessList transaction without to field should be valid"
        );
        let access_list_tx_with_to = Transaction {
            to: Some(Address::default()),
            ..access_list_tx.clone()
        };
        assert!(
            access_list_tx_with_to.is_valid(),
            "AccessList transaction with to field should be valid"
        );

        // Valid DynamicFee transactions
        let dynamic_fee_tx = Transaction {
            transaction_type: TransactionType::DynamicFee,
            to: None,
            ..Default::default()
        };
        assert!(
            dynamic_fee_tx.is_valid(),
            "DynamicFee transaction without to field should be valid"
        );
        let dynamic_fee_tx_with_to = Transaction {
            to: Some(Address::default()),
            ..dynamic_fee_tx.clone()
        };
        assert!(
            dynamic_fee_tx_with_to.is_valid(),
            "DynamicFee transaction with to field should be valid"
        );

        // Valid Blob transactions
        let blob_tx = Transaction {
            transaction_type: TransactionType::Blob,
            to: Some(Address::default()),
            ..Default::default()
        };
        assert!(blob_tx.is_valid(), "Blob transaction should be valid");

        // Valid SetCode transactions
        let set_code_tx = Transaction {
            transaction_type: TransactionType::SetCode,
            to: Some(Address::default()),
            ..Default::default()
        };
        assert!(
            set_code_tx.is_valid(),
            "SetCode transaction should be valid"
        );
    }

    #[test]
    fn is_valid_returns_false_for_invalid_transactions() {
        // Invalid Blob transaction without to field
        let invalid_blob_tx = Transaction {
            transaction_type: TransactionType::Blob,
            to: None,
            ..Default::default()
        };
        assert!(
            !invalid_blob_tx.is_valid(),
            "Blob transaction without to field should be invalid"
        );

        // Invalid SetCode transaction without to field
        let invalid_set_code_tx = Transaction {
            transaction_type: TransactionType::SetCode,
            to: None,
            ..Default::default()
        };
        assert!(
            !invalid_set_code_tx.is_valid(),
            "SetCode transaction without to field should be invalid"
        );
    }
}
