mod access_list_tx;
mod blob_tx;
mod dynamic_fee_tx;
mod error;
mod legacy_tx;
mod set_code_tx;

use serde::{Deserialize, Serialize};

pub use crate::transaction::{
    access_list_tx::AccessListEntry, error::TransactionError, set_code_tx::SetCodeAuthorization,
};
use crate::{
    Address, AsHex, Hash, HexConvert, U256,
    parse_hex_error::ParseHexError,
    transaction::{
        access_list_tx::AccessListTx, blob_tx::BlobTx, dynamic_fee_tx::DynamicFeeTx,
        legacy_tx::LegacyTx, set_code_tx::SetCodeTx,
    },
};

/// An Ethereum-compatible transaction.
/// It contains all the fields required for different transaction types.
/// Fields are named according to the Ethereum Yellow Paper Shanghai version (except for EIP-7702
/// fields). Go-ethereum names, where they differ, are indicated through doc comments on each field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "JsonRpcTransaction", into = "JsonRpcTransaction")]
pub struct Transaction {
    pub transaction_type: TransactionType,
    pub chain_id: U256,
    pub nonce: u64,
    pub gas_price: U256, // LegacyTx, AccessListTx
    pub gas_limit: u64,
    pub to: Option<Address>,
    pub value: U256,
    pub data: Vec<u8>, // Called init for contract creation, data for message call transactions
    pub access_list: Vec<AccessListEntry>,
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
#[repr(u8)]
pub enum TransactionType {
    Legacy = 0,
    AccessList = 1,
    DynamicFee = 2,
    Blob = 3,
    SetCode = 4,
}

impl TryFrom<u8> for TransactionType {
    type Error = TransactionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TransactionType::Legacy),
            1 => Ok(TransactionType::AccessList),
            2 => Ok(TransactionType::DynamicFee),
            3 => Ok(TransactionType::Blob),
            4 => Ok(TransactionType::SetCode),
            _ => Err(TransactionError::InvalidTransactionType(value)),
        }
    }
}

impl HexConvert for TransactionType {
    fn try_from_hex(value: &str) -> Result<Self, ParseHexError> {
        let v = u8::from_str_radix(value.trim_start_matches("0x"), 16)
            .map_err(Into::<ParseHexError>::into)?;
        TransactionType::try_from(v)
            .map_err(|err: TransactionError| ParseHexError::Custom(err.to_string()))
    }

    fn to_hex(&self) -> String {
        format!("0x{:x}", *self as u8)
    }
}

/// A JSON-RPC representation of an Ethereum transaction.
/// The fields are named according to the Ethereum JSON-RPC specification.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcTransaction {
    // NOTE: this is not called `type` because it is a reserved keyword in Rust
    #[serde(rename = "type")]
    pub transaction_type: AsHex<TransactionType>,
    #[serde(default)]
    pub chain_id: AsHex<U256>,
    pub nonce: AsHex<u64>,
    pub gas_price: AsHex<U256>,
    pub gas: AsHex<u64>,
    #[serde(default)]
    pub to: Option<AsHex<Address>>,
    pub value: AsHex<U256>,
    pub input: AsHex<Vec<u8>>,
    #[serde(default)]
    pub access_list: Vec<AccessListEntry>,
    #[serde(default)]
    pub max_priority_fee_per_gas: AsHex<U256>,
    #[serde(default)]
    pub max_fee_per_gas: AsHex<U256>,
    #[serde(deserialize_with = "deserialize_null::<Vec<AsHex<Hash>>, _>")]
    #[serde(default)]
    pub blob_versioned_hashes: Vec<AsHex<Hash>>,
    #[serde(deserialize_with = "deserialize_null::<AsHex<U256>, _>")]
    #[serde(default)]
    pub max_fee_per_blob_gas: AsHex<U256>,
    #[serde(default)]
    pub authorization_list: Vec<SetCodeAuthorization>,
    pub v: AsHex<U256>,
    pub r: AsHex<U256>,
    pub s: AsHex<U256>,
}

impl TryFrom<JsonRpcTransaction> for Transaction {
    type Error = TransactionError;

    fn try_from(value: JsonRpcTransaction) -> Result<Self, Self::Error> {
        let tx = Transaction {
            transaction_type: value.transaction_type.0,
            nonce: value.nonce.0,
            gas_price: value.gas_price.0,
            gas_limit: value.gas.0,
            to: value.to.map(|addr| addr.0),
            value: value.value.0,
            data: value.input.0,
            y_parity: value.v.0,
            r: value.r.0,
            s: value.s.0,
            chain_id: value.chain_id.0,
            max_priority_fee_per_gas: value.max_priority_fee_per_gas.0,
            max_fee_per_gas: value.max_fee_per_gas.0,
            access_list: value.access_list,
            max_fee_per_blob_gas: value.max_fee_per_blob_gas.0,
            blob_versioned_hashes: value
                .blob_versioned_hashes
                .into_iter()
                .map(|h| h.0)
                .collect(),
            authorization_list: value.authorization_list,
        };

        tx.is_valid()?;
        Ok(tx)
    }
}

impl From<Transaction> for JsonRpcTransaction {
    fn from(value: Transaction) -> Self {
        JsonRpcTransaction {
            transaction_type: AsHex(value.transaction_type),
            nonce: AsHex(value.nonce),
            gas_price: AsHex(value.gas_price),
            gas: AsHex(value.gas_limit),
            to: value.to.map(AsHex),
            value: AsHex(value.value),
            input: AsHex(value.data),
            v: AsHex(value.y_parity),
            r: AsHex(value.r),
            s: AsHex(value.s),
            chain_id: AsHex(value.chain_id),
            max_priority_fee_per_gas: AsHex(value.max_priority_fee_per_gas),
            max_fee_per_gas: AsHex(value.max_fee_per_gas),
            access_list: value.access_list,
            max_fee_per_blob_gas: AsHex(value.max_fee_per_blob_gas),
            blob_versioned_hashes: value.blob_versioned_hashes.into_iter().map(AsHex).collect(),
            authorization_list: value.authorization_list,
        }
    }
}

impl Transaction {
    /// Checks if the transaction can be converted to a
    /// specialized transaction type.
    pub fn is_valid(&self) -> Result<(), TransactionError> {
        match self.transaction_type {
            TransactionType::Legacy => LegacyTx::is_constructible_from(self),
            TransactionType::DynamicFee => DynamicFeeTx::is_constructible_from(self),
            TransactionType::AccessList => AccessListTx::is_constructible_from(self),
            TransactionType::Blob => BlobTx::is_constructible_from(self),
            TransactionType::SetCode => SetCodeTx::is_constructible_from(self),
        }
    }
}

impl Serialize for JsonRpcTransaction {
    /// Serialize the Transaction into a JSON-RPC compatible format.
    /// Depending on the transaction type, it will serialize into the specific transaction format
    /// with an additional transaction type field.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        /// An utility struct to serialize the transaction with its type.
        #[derive(Serialize)]
        struct TransactionWithType<T> {
            #[serde(rename = "type")]
            transaction_type: AsHex<TransactionType>,
            #[serde(flatten)]
            transaction: T,
        }

        let tx = Transaction::try_from(self.clone())
            .map_err(|err: TransactionError| serde::ser::Error::custom(err.to_string()))?;
        // NOTE: A conversion error can never happen here as `Transaction::try_from` checks for the
        // validity and it is therefore safe to unwrap.
        match self.transaction_type.0 {
            TransactionType::Legacy => LegacyTx::try_from(tx)
                .map(|tx| TransactionWithType {
                    transaction_type: AsHex(TransactionType::Legacy),
                    transaction: tx,
                })
                .unwrap()
                .serialize(serializer),
            TransactionType::AccessList => AccessListTx::try_from(tx)
                .map(|tx| TransactionWithType {
                    transaction_type: AsHex(TransactionType::AccessList),
                    transaction: tx,
                })
                .unwrap()
                .serialize(serializer),
            TransactionType::DynamicFee => DynamicFeeTx::try_from(tx)
                .map(|tx| TransactionWithType {
                    transaction_type: AsHex(TransactionType::DynamicFee),
                    transaction: tx,
                })
                .unwrap()
                .serialize(serializer),
            TransactionType::Blob => BlobTx::try_from(tx)
                .map(|tx| TransactionWithType {
                    transaction_type: AsHex(TransactionType::Blob),
                    transaction: tx,
                })
                .unwrap()
                .serialize(serializer),
            TransactionType::SetCode => SetCodeTx::try_from(tx)
                .map(|tx| TransactionWithType {
                    transaction_type: AsHex(TransactionType::SetCode),
                    transaction: tx,
                })
                .unwrap()
                .serialize(serializer),
        }
    }
}

/// An helper function to deserialize a hex-convertible value into a default value if it is null
fn deserialize_null<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    let value: Option<T> = Option::deserialize(deserializer)?;
    Ok(value.unwrap_or_default())
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::HexConvert;

    #[test]
    fn is_valid_correctly_checks_transaction() {
        let mut transaction = make_transaction(TransactionType::Legacy, true);

        assert!(
            transaction.is_valid().is_ok(),
            "Legacy transaction with to field should be valid"
        );
        transaction.to = None;
        assert!(
            transaction.is_valid().is_ok(),
            "Legacy transaction without to field should be valid"
        );

        transaction.transaction_type = TransactionType::AccessList;
        transaction.to = Some(Address::default());
        assert!(
            transaction.is_valid().is_ok(),
            "AccessList transaction with to field should be valid"
        );
        transaction.to = None;
        assert!(
            transaction.is_valid().is_ok(),
            "AccessList transaction without to field should be valid"
        );

        transaction.transaction_type = TransactionType::DynamicFee;
        transaction.to = Some(Address::default());
        assert!(
            transaction.is_valid().is_ok(),
            "DynamicFee transaction with to field should be valid"
        );
        transaction.to = None;
        assert!(
            transaction.is_valid().is_ok(),
            "DynamicFee transaction without to field should be valid"
        );

        transaction.transaction_type = TransactionType::Blob;
        transaction.to = Some(Address::default());
        assert!(
            transaction.is_valid().is_ok(),
            "Blob transaction should be valid"
        );

        transaction.transaction_type = TransactionType::SetCode;
        assert!(
            transaction.is_valid().is_ok(),
            "SetCode transaction should be valid"
        );
    }

    #[test]
    fn is_valid_returns_false_for_invalid_transactions() {
        let invalid_blob_tx = make_transaction(TransactionType::Blob, false);
        assert!(
            invalid_blob_tx.is_valid().is_err(),
            "Blob transaction without to field should be invalid"
        );

        let invalid_set_code_tx = make_transaction(TransactionType::SetCode, false);
        assert!(
            invalid_set_code_tx.is_valid().is_err(),
            "SetCode transaction without to field should be invalid"
        );
    }

    #[test]
    fn can_be_serialized_to_json_rpc_format() {
        let mut transaction = make_transaction(TransactionType::Legacy, true);

        let json_str = serde_json::to_string(&transaction)
            .expect("Serialization of Legacy transaction with to field should not fail");
        assert_eq!(to_value(&json_str), make_json_legacy_tx(true));

        transaction.to = None;
        let json_str_without_to = serde_json::to_string(&transaction)
            .expect("Serialization of Legacy transaction without to field should not fail");
        assert_eq!(to_value(&json_str_without_to), make_json_legacy_tx(false));

        transaction.transaction_type = TransactionType::AccessList;
        transaction.to = Some(Address::default());
        let json_str_access_list = serde_json::to_string(&transaction)
            .expect("Serialization of AccessList transaction with to field should not fail");
        assert_eq!(
            to_value(&json_str_access_list),
            make_json_access_list_tx(true)
        );

        transaction.to = None;
        let json_str_access_list_without_to = serde_json::to_string(&transaction)
            .expect("Serialization of AccessList transaction without to field should not fail");
        assert_eq!(
            to_value(&json_str_access_list_without_to),
            make_json_access_list_tx(false)
        );

        transaction.transaction_type = TransactionType::DynamicFee;
        transaction.to = Some(Address::default());
        let json_str_dynamic_fee = serde_json::to_string(&transaction)
            .expect("Serialization of DynamicFee transaction with to field should not fail");
        assert_eq!(
            to_value(&json_str_dynamic_fee),
            make_json_dynamic_fee_tx(true)
        );

        transaction.to = None;
        let json_str_dynamic_fee_without_to = serde_json::to_string(&transaction)
            .expect("Serialization of DynamicFee transaction without to field should not fail");
        assert_eq!(
            to_value(&json_str_dynamic_fee_without_to),
            make_json_dynamic_fee_tx(false)
        );

        transaction.transaction_type = TransactionType::Blob;
        transaction.to = Some(Address::default());
        let json_str_blob = serde_json::to_string(&transaction)
            .expect("Serialization of valid Blob transaction should not fail");
        assert_eq!(to_value(&json_str_blob), make_json_blob_tx());

        transaction.transaction_type = TransactionType::SetCode;
        let json_str_set_code = serde_json::to_string(&transaction)
            .expect("Serialization of valid SetCode transaction should not fail");
        assert_eq!(to_value(&json_str_set_code), make_json_set_code_tx());
    }

    #[test]
    fn serialization_fails_for_invalid_transactions() {
        let transaction = make_transaction(TransactionType::Blob, false);
        let res = serde_json::to_string(&transaction);
        assert!(
            res.is_err(),
            "Serialization of Blob transaction without to field should fail"
        );

        let transaction = make_transaction(TransactionType::SetCode, false);
        let res = serde_json::to_string(&transaction);
        assert!(
            res.is_err(),
            "Serialization of SetCode transaction without to field should fail"
        );
    }

    #[test]
    fn deserialize_null_handles_null_values() {
        #[derive(Deserialize, Debug)]
        struct TestDeserializeNull {
            #[serde(deserialize_with = "deserialize_null")]
            value: u32,
        }

        let json_str = r#"{"value": null}"#;
        let deserialized: TestDeserializeNull = serde_json::from_str(json_str)
            .expect("Deserialization should handle null values correctly");
        assert_eq!(
            deserialized.value,
            u32::default(),
            "Null value should be deserialized to default value"
        );

        let json_str_with_value = r#"{"value": 42}"#;
        let deserialized_with_value: TestDeserializeNull =
            serde_json::from_str(json_str_with_value)
                .expect("Deserialization should handle non-null values correctly");
        assert_eq!(deserialized_with_value.value, 42u32);
    }

    #[test]
    fn can_be_deserialized_from_json() {
        // Legacy Tx
        {
            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::Legacy, true))
                    .expect("Deserialization should not fail");
            let legacy_tx = LegacyTx::try_from(transaction)
                .expect("Conversion to LegacyTx with to field should not fail");
            assert_eq!(legacy_tx, make_legacy_tx(true));

            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::Legacy, false))
                    .expect("Deserialization should not fail");
            let legacy_tx = LegacyTx::try_from(transaction.clone())
                .expect("Conversion to LegacyTx without to field should not fail");
            assert_eq!(legacy_tx, make_legacy_tx(false));
        }

        // Access List Tx
        {
            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::AccessList, true))
                    .expect("Deserialization should not fail");
            let access_list_tx = AccessListTx::try_from(transaction.clone())
                .expect("Conversion to AccessListTx with to field should not fail");
            assert_eq!(access_list_tx, make_access_list_tx(true));

            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::AccessList, false))
                    .expect("Deserialization should not fail");
            let access_list_tx = AccessListTx::try_from(transaction.clone())
                .expect("Conversion to AccessListTx without to field should not fail");
            assert_eq!(access_list_tx, make_access_list_tx(false));
        }

        // Dynamic Fee Tx
        {
            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::DynamicFee, true))
                    .expect("Deserialization should not fail");
            let dynamic_fee_tx = DynamicFeeTx::try_from(transaction.clone())
                .expect("Conversion to DynamicFeeTx with to field should not fail");
            assert_eq!(dynamic_fee_tx, make_dynamic_fee_tx(true));

            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::DynamicFee, false))
                    .expect("Deserialization should not fail");
            let dynamic_fee_tx = DynamicFeeTx::try_from(transaction.clone())
                .expect("Conversion to DynamicFeeTx without to should not fail");
            assert_eq!(dynamic_fee_tx, make_dynamic_fee_tx(false));
        }

        // Blob Tx
        {
            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::Blob, true))
                    .expect("Deserialization should not fail");
            let blob_tx = BlobTx::try_from(transaction.clone())
                .expect("Conversion to BlobTx should not fail");
            assert_eq!(blob_tx, make_blob_tx());
        }

        // Set Code Tx
        {
            let transaction: Transaction =
                serde_json::from_value(make_json_transaction(TransactionType::SetCode, true))
                    .expect("Deserialization should not fail");
            let set_code_tx = SetCodeTx::try_from(transaction.clone())
                .expect("Conversion to SetCodeTx should not fail");
            assert_eq!(set_code_tx, make_set_code_tx());
        }
    }

    #[test]
    fn deserialize_fails_for_invalid_transactions() {
        let res: Result<Transaction, _> =
            serde_json::from_value(make_json_transaction_with_invalid_type());
        assert!(
            res.is_err(),
            "Deserialization of Transaction with invalid transaction type should fail"
        );

        let res: Result<Transaction, _> =
            serde_json::from_value(make_json_transaction(TransactionType::Blob, false));
        assert!(
            res.is_err(),
            "Deserialization of Blob transaction without to field should fail"
        );

        let res: Result<Transaction, _> =
            serde_json::from_value(make_json_transaction(TransactionType::SetCode, false));
        assert!(
            res.is_err(),
            "Deserialization of SetCode transaction without to field should fail"
        );
    }

    fn to_value(json_str: &str) -> serde_json::Value {
        serde_json::from_str(json_str).unwrap()
    }

    impl Default for Transaction {
        fn default() -> Self {
            Transaction {
                transaction_type: TransactionType::Legacy,
                chain_id: U256::default(),
                nonce: u64::default(),
                gas_price: U256::default(),
                gas_limit: u64::default(),
                to: Some(Address::default()),
                value: U256::default(),
                data: Vec::default(),
                access_list: Vec::default(),
                max_fee_per_gas: U256::default(),
                max_priority_fee_per_gas: U256::default(),
                blob_versioned_hashes: vec![Hash::default()],
                max_fee_per_blob_gas: U256::default(),
                authorization_list: Vec::default(),
                y_parity: U256::default(),
                r: U256::default(),
                s: U256::default(),
            }
        }
    }

    fn make_transaction(type_: TransactionType, include_to: bool) -> Transaction {
        Transaction {
            transaction_type: type_,
            to: if include_to {
                Some(Address::default())
            } else {
                None
            },
            access_list: vec![AccessListEntry {
                address: Address::default(),
                storage_keys: vec![Hash::default()],
            }],
            blob_versioned_hashes: vec![Hash::default()],
            authorization_list: vec![SetCodeAuthorization {
                chain_id: U256::default(),
                address: Address::default(),
                nonce: 0,
                y_parity: u8::default(),
                r: U256::default(),
                s: U256::default(),
            }],
            ..Default::default()
        }
    }

    fn make_legacy_tx(include_to: bool) -> LegacyTx {
        let tx = make_transaction(TransactionType::Legacy, include_to);
        LegacyTx::try_from(tx).unwrap()
    }

    fn make_access_list_tx(include_to: bool) -> AccessListTx {
        let tx = make_transaction(TransactionType::AccessList, include_to);
        AccessListTx::try_from(tx).unwrap()
    }

    fn make_dynamic_fee_tx(include_to: bool) -> DynamicFeeTx {
        let tx = make_transaction(TransactionType::DynamicFee, include_to);
        DynamicFeeTx::try_from(tx).unwrap()
    }

    fn make_blob_tx() -> BlobTx {
        let tx = make_transaction(TransactionType::Blob, true);
        BlobTx::try_from(tx).unwrap()
    }

    fn make_set_code_tx() -> SetCodeTx {
        let tx = make_transaction(TransactionType::SetCode, true);
        SetCodeTx::try_from(tx).unwrap()
    }

    fn make_json_transaction(tx_type: TransactionType, include_to: bool) -> serde_json::Value {
        let tx = make_transaction(tx_type, include_to);
        let tx_json = format!(
            r#"{{
                    "type": "{}",
                    "chainId": "{}",
                    "nonce": "{}",
                    "gasPrice": "{}",
                    "gas": "{}",
                    {}
                    "value": "{}",
                    "input": "{}",
                    "accessList": [
                        {{
                            "address": "{}",
                            "storageKeys": ["{}"]
                        }}
                    ],
                    "maxFeePerGas": "{}",
                    "maxPriorityFeePerGas": "{}",
                    "blobVersionedHashes": ["{}"],
                    "maxFeePerBlobGas": "{}",
                    "authorizationList": [
                    {{
                        "chainId": "{}",
                        "address": "{}",
                        "nonce": "{}",
                        "yParity": "{}",
                        "r": "{}",
                        "s": "{}"
                    }}
                ],
                    "v": "{}",
                    "r": "{}",
                    "s": "{}"
                }}"#,
            tx.transaction_type.to_hex(),
            tx.chain_id.to_hex(),
            tx.nonce.to_hex(),
            tx.gas_price.to_hex(),
            tx.gas_limit.to_hex(),
            if include_to {
                format!(
                    r#""to": "{}","#,
                    tx.to.map(|addr| addr.to_hex()).unwrap_or_default()
                )
            } else {
                String::new()
            },
            tx.value.to_hex(),
            tx.data.to_hex(),
            tx.access_list[0].address.to_hex(),
            tx.access_list[0]
                .storage_keys
                .first()
                .map(HexConvert::to_hex)
                .unwrap(),
            tx.max_fee_per_gas.to_hex(),
            tx.max_priority_fee_per_gas.to_hex(),
            tx.blob_versioned_hashes
                .first()
                .map(HexConvert::to_hex)
                .unwrap(),
            tx.max_fee_per_blob_gas.to_hex(),
            tx.authorization_list[0].chain_id.to_hex(),
            tx.authorization_list[0].address.to_hex(),
            tx.authorization_list[0].nonce.to_hex(),
            tx.authorization_list[0].y_parity.to_hex(),
            tx.authorization_list[0].r.to_hex(),
            tx.authorization_list[0].s.to_hex(),
            tx.y_parity.to_hex(),
            tx.r.to_hex(),
            tx.s.to_hex()
        );
        serde_json::from_str(&tx_json).unwrap()
    }

    fn make_json_transaction_with_invalid_type() -> serde_json::Value {
        let mut invalid_tx = make_json_transaction(TransactionType::Legacy, true);
        invalid_tx["type"] = "0x5".into();
        invalid_tx
    }

    /// Utility function to copy fields from a value into a map
    /// This is used to construct specialized transaction JSON representations from a Transaction
    /// Value
    fn copy_fields(
        fields: Vec<&str>,
        dest: &mut serde_json::Map<String, serde_json::Value>,
        source: serde_json::Value,
        include_to: bool,
    ) {
        for f in fields {
            if f == "to" && !include_to {
                continue;
            }
            dest.insert(f.into(), source[f].clone());
        }
    }

    fn make_json_legacy_tx(include_to: bool) -> serde_json::Value {
        let tx = make_json_transaction(TransactionType::Legacy, include_to);
        let mut legacy_tx_value = serde_json::Map::new();
        let fields = [
            "type", "nonce", "gasPrice", "gas", "to", "value", "input", "v", "r", "s",
        ];

        copy_fields(fields.to_vec(), &mut legacy_tx_value, tx, include_to);
        legacy_tx_value.into()
    }

    fn make_json_access_list_tx(include_to: bool) -> serde_json::Value {
        let tx = make_json_transaction(TransactionType::AccessList, include_to);
        let mut access_list_tx_value = serde_json::Map::new();
        let fields = [
            "type",
            "chainId",
            "nonce",
            "gasPrice",
            "gas",
            "to",
            "value",
            "input",
            "accessList",
            "v",
            "r",
            "s",
        ];

        copy_fields(fields.to_vec(), &mut access_list_tx_value, tx, include_to);
        access_list_tx_value.into()
    }

    fn make_json_dynamic_fee_tx(include_to: bool) -> serde_json::Value {
        let tx = make_json_transaction(TransactionType::DynamicFee, include_to);
        let mut dynamic_fee_tx_value = serde_json::Map::new();
        let fields = [
            "type",
            "chainId",
            "nonce",
            "maxPriorityFeePerGas",
            "maxFeePerGas",
            "gas",
            "to",
            "value",
            "input",
            "accessList",
            "v",
            "r",
            "s",
        ];

        copy_fields(fields.to_vec(), &mut dynamic_fee_tx_value, tx, include_to);
        dynamic_fee_tx_value.into()
    }

    fn make_json_blob_tx() -> serde_json::Value {
        let tx = make_json_transaction(TransactionType::Blob, true);
        let mut blob_tx_value = serde_json::Map::new();
        let fields = [
            "type",
            "chainId",
            "nonce",
            "gas",
            "to",
            "value",
            "input",
            "accessList",
            "maxFeePerGas",
            "maxPriorityFeePerGas",
            "blobVersionedHashes",
            "maxFeePerBlobGas",
            "v",
            "r",
            "s",
        ];

        copy_fields(fields.to_vec(), &mut blob_tx_value, tx, true);
        blob_tx_value.into()
    }

    fn make_json_set_code_tx() -> serde_json::Value {
        let tx = make_json_transaction(TransactionType::SetCode, true);
        let mut set_code_tx_value = serde_json::Map::new();
        let fields = [
            "type",
            "chainId",
            "nonce",
            "gas",
            "to",
            "value",
            "input",
            "accessList",
            "maxFeePerGas",
            "maxPriorityFeePerGas",
            "authorizationList",
            "v",
            "r",
            "s",
        ];

        copy_fields(fields.to_vec(), &mut set_code_tx_value, tx, true);
        set_code_tx_value.into()
    }
}
