mod access_list_tx;
mod blob_tx;
mod dynamic_fee_tx;
mod error;
mod legacy_tx;
mod set_code_tx;

use std::fmt::Display;

#[allow(unused_imports)]
use access_list_tx::AccessListTx;
pub use access_list_tx::AccessTuple;
#[allow(unused_imports)]
use blob_tx::BlobTx;
#[allow(unused_imports)]
use dynamic_fee_tx::DynamicFeeTx;
pub use error::TransactionError;
#[allow(unused_imports)]
use legacy_tx::LegacyTx;
pub use set_code_tx::SetCodeAuthorization;
#[allow(unused_imports)]
use set_code_tx::SetCodeTx;

use crate::{Address, Hash, U256};

/// An Ethereum-compatible transaction.
/// It contains all the fields required for different transaction types.
/// Fields are named according to the Ethereum Yellow Paper Shanghai version (except for EIP-7702
/// fields). Go-ethereum names, where they differ, are indicated through doc comments on each field.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// geth: v
    pub y_parity: U256,
    pub r: U256,
    pub s: U256,
}

/// The Ethereum transaction types, as defined by EIP 2718, EIP 2930, EIP 1559, EIP 4844, and EIP
/// 7702.
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
                max_fee_per_gas: U256::default(),
                max_priority_fee_per_gas: U256::default(),
                blob_versioned_hashes: Vec::new(),
                max_fee_per_blob_gas: U256::default(),
                authorization_list: Vec::new(),
                y_parity: U256::default(),
                r: U256::default(),
                s: U256::default(),
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
}
