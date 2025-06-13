use std::fmt::Display;
use thiserror::Error;

use crate::{Address, Hash, U256};

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("couldn't convert transaction to type {0}")]
    ConversionError(TransactionType),
}

/// An Ethereum-compatible transaction.
/// It contains all the fields required for different transaction types.
/// Fields are named according to the Ethereum Yellow Paper Shanghai version (except for EIP-7702 fields).
/// Go-ethereum names, where they differ, are indicated through doc comments on each field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub transaction_type: TransactionType,
    pub chain_id: U256,
    pub nonce: u64,
    pub gas_price: U256, // LegacyTx, AccessListTx
    pub gas_limit: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub data: Vec<u8>, // Called init for contract creation, data for message call transactions
    pub access_list: Vec<AccessTuple>,
    /// geth: v
    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
    // The following fields are in EIP order
    /// geth: GasFeeCap
    pub max_fee_per_gas: U256, // DynamicFeeTx, BlobTx, SetCodeTx
    /// geth: GasTipCap
    pub max_priority_fee_per_gas: U256, // DynamicFeeTx, BlobTx, SetCodeTx
    /// geth: BlobHashes
    pub blob_versioned_hashes: Vec<Hash>, // BlobTx
    /// geth: BlobFeeCap
    pub max_fee_per_blob_gas: U256, // BlobTx
    /// geth: AuthList
    pub authorization_list: Vec<SetCodeAuthorization>, // SetCodeTx
}

/// The Ethereum transaction types, as defined by EIP 2718, EIP 2930, EIP 1559, EIP 4844, and EIP 7702.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionType {
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

/// The Legacy Ethereum transaction, defined in the EIP 2718.
/// Source: https://eips.ethereum.org/EIPS/eip-2718
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct LegacyTx {
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

/// The Access List Ethereum transaction, defined in the EIP 2930.
/// Source: https://eips.ethereum.org/EIPS/eip-2930
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct AccessListTx {
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

// The Dynamic Fee Ethereum transaction, defined in the EIP 1559.
// Source: https://eips.ethereum.org/EIPS/eip-1559
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct DynamicFeeTx {
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

/// The Blob Ethereum transaction, defined in the EIP 4844.
/// Source: https://eips.ethereum.org/EIPS/eip-4844
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct BlobTx {
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

/// The SetCode Ethereum transaction, defined in the EIP 7702.
/// Source: https://eips.ethereum.org/EIPS/eip-7702
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct SetCodeTx {
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
            y_parity: tx.w,
            r: tx.r,
            s: tx.s,
            chain_id: U256::default(),
            max_priority_fee_per_gas: U256::default(),
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
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
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
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
            max_priority_fee_per_gas: U256::default(),
            max_fee_per_gas: U256::default(),
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
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            max_fee_per_blob_gas: tx.max_fee_per_blob_gas,
            blob_versioned_hashes: tx.blob_versioned_hashes,
            authorization_list: Vec::new(),
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
            y_parity: tx.y_parity,
            r: tx.r,
            s: tx.s,
            max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
            max_fee_per_gas: tx.max_fee_per_gas,
            max_fee_per_blob_gas: U256::default(),
            blob_versioned_hashes: Vec::new(),
            authorization_list: tx.authorization_list,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    impl Default for Transaction {
        fn default() -> Self {
            Self {
                transaction_type: TransactionType::Legacy,
                chain_id: U256::default(),
                nonce: 0,
                gas_price: U256::default(),
                gas_limit: 21000,
                to: None,
                value: U256::default(),
                data: Vec::new(),
                access_list: Vec::new(),
                y_parity: U256::default(),
                r: U256::default(),
                s: U256::default(),
                max_fee_per_gas: U256::default(),
                max_priority_fee_per_gas: U256::default(),
                blob_versioned_hashes: Vec::new(),
                max_fee_per_blob_gas: U256::default(),
                authorization_list: Vec::new(),
            }
        }
    }

    #[test]
    fn transaction_type_display_prints_correct_name() {
        assert_eq!(TransactionType::Legacy.to_string(), "LegacyTx");
        assert_eq!(TransactionType::AccessList.to_string(), "AccessListTx");
        assert_eq!(TransactionType::DynamicFee.to_string(), "DynamicFeeTx");
        assert_eq!(TransactionType::Blob.to_string(), "BlobTx");
        assert_eq!(TransactionType::SetCode.to_string(), "SetCodeTx");
    }

    #[test]
    fn can_be_converted_to_and_from_specialized_transaction_types() {
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
    fn conversion_to_specialized_types_fail_if_error_occurs() {
        // Attempt to convert to LegacyTx with mismatched transaction type
        let error = LegacyTx::try_from(Transaction {
            transaction_type: TransactionType::DynamicFee,
            ..Default::default()
        })
        .expect_err("Conversion to legacy transaction must fail");
        assert!(matches!(
            error,
            TransactionError::ConversionError(TransactionType::Legacy)
        ));

        // Attempt to convert to DynamicFeeTx with mismatched transaction type
        let error = DynamicFeeTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to dynamic fee transaction must fail");
        assert!(matches!(
            error,
            TransactionError::ConversionError(TransactionType::DynamicFee)
        ));

        // Attempt to convert to AccessListTx with mismatched transaction type
        let error = AccessListTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to access list transaction must fail");
        assert!(matches!(
            error,
            TransactionError::ConversionError(TransactionType::AccessList)
        ));

        // Attempt to convert to BlobTx with mismatched transaction type
        let error = BlobTx::try_from(Transaction {
            transaction_type: TransactionType::Legacy,
            ..Default::default()
        })
        .expect_err("Conversion to blob transaction must fail");
        assert!(matches!(
            error,
            TransactionError::ConversionError(TransactionType::Blob)
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
            TransactionError::ConversionError(TransactionType::Blob)
        ));

        // Attempt to convert to SetCodeTx with mismatched transaction type
        let error = SetCodeTx::try_from(Transaction {
            transaction_type: TransactionType::Blob,
            ..Default::default()
        })
        .expect_err("Conversion to set code transaction must fail");
        assert!(matches!(
            error,
            TransactionError::ConversionError(TransactionType::SetCode)
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
            TransactionError::ConversionError(TransactionType::SetCode)
        ));
    }
}
