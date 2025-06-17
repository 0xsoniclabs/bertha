mod access_list_tx;
mod blob_tx;
mod dynamic_fee_tx;
mod error;
mod legacy_tx;
mod set_code_tx;

use alloy_rlp::{Decodable, Encodable, Header};
use serde::{Deserialize, Serialize};

pub use crate::transaction::{
    access_list_tx::AccessListEntry, error::TransactionError, set_code_tx::SetCodeAuthorization,
};
use crate::{
    Address, AsHex, Hash, HexConvert, RlpNil, RlpString, U256,
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
    pub data: Vec<u8>, // Called `init` for contract creation, `data` for message call transactions
    // The following fields are in EIP order
    pub access_list: Vec<AccessListEntry>, // AccessListTx, DynamicFeeTx, BlobTx, SetCodeTx
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

impl Decodable for Transaction {
    fn decode(rlp: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        let header = Header::decode(rlp)?;
        if header.list {
            Ok(LegacyTx::decode(rlp, header.payload_length)?.into())
        } else {
            if rlp.is_empty() {
                return Err(alloy_rlp::Error::InputTooShort);
            }
            let type_ = rlp[0];
            let mut inner_data = &rlp[1..header.payload_length];
            *rlp = &rlp[header.payload_length..];
            match type_ {
                1 => Ok(AccessListTx::decode(&mut inner_data)?.into()),
                2 => Ok(DynamicFeeTx::decode(&mut inner_data)?.into()),
                3 => Ok(BlobTx::decode(&mut inner_data)?.into()),
                4 => Ok(SetCodeTx::decode(&mut inner_data)?.into()),
                _ => Err(alloy_rlp::Error::Custom("invalid transaction type")),
            }
        }
    }
}

impl Encodable for Transaction {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        fn encode_with_type_as_rlp_string<T: Encodable>(
            transaction_type: TransactionType,
            tx: T,
            out: &mut dyn alloy_rlp::BufMut,
        ) {
            let mut buf = Vec::new();
            buf.push(transaction_type as u8);
            tx.encode(&mut buf);
            RlpString(buf).encode(out);
        }

        match self.transaction_type {
            TransactionType::Legacy => LegacyTx::try_from(self.clone())
                .map(|tx| tx.encode(out))
                .unwrap_or_else(|_| "".encode(out)),
            TransactionType::AccessList => AccessListTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, out))
                .unwrap_or_else(|_| "".encode(out)),
            TransactionType::DynamicFee => DynamicFeeTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, out))
                .unwrap_or_else(|_| "".encode(out)),
            TransactionType::Blob => BlobTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, out))
                .unwrap_or_else(|_| "".encode(out)),
            TransactionType::SetCode => SetCodeTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, out))
                .unwrap_or_else(|_| "".encode(out)),
        };
    }
}

/// The Ethereum transaction types, as defined by EIP 2718, EIP 2930, EIP 1559, EIP 4844, and EIP
/// 7702.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    pub to: AsHex<RlpNil<Address>>,
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
            to: value.to.0.0,
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
            to: AsHex(RlpNil(value.to)),
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

    /// Generates a set of transactions with their RLP encodings.
    fn generate_transactions_with_rlp() -> impl IntoIterator<Item = (Transaction, Vec<u8>)> {
        // tested cases
        // - all 5 transaction types
        // - to field:
        //   - set to None
        //   - set to Some
        // - data field:
        //   - empty
        //   - non-empty
        // - access_list field:
        //   - empty
        //   - non-empty
        // - blob_versioned_hashes field:
        //   - empty
        //   - non-empty
        // - authorization_list field:
        //   - empty
        //   - non-empty

        [
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x0").unwrap(),
                    chain_id: U256::try_from_hex("0x0").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::default(),
                    max_fee_per_gas: U256::default(),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    y_parity: U256::try_from_hex("0x25").unwrap(),
                    r: U256::try_from_hex(
                        "0x81f84dfa55a3b2e8abd5f03605e386c20a71050103dd518c4bf27c4b9308d0b4",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x4339e8f47dd680a7f5d49ace126c6bbbad5ee7a6ba1dcb3114b2294565c8b134",
                    )
                    .unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    blob_versioned_hashes: Vec::new(),
                    authorization_list: Vec::new(),
                },
                const_hex::decode(
                    "0xf84980808080808025a081f84dfa55a3b2e8abd5f03605e386c20a71050103dd518c4bf27c4b9308d0b4a04339e8f47dd680a7f5d49ace126c6bbbad5ee7a6ba1dcb3114b2294565c8b134",
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x0").unwrap(),
                    chain_id: U256::try_from_hex("0x0").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::default(),
                    max_fee_per_gas: U256::default(),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x01").unwrap(),
                    y_parity: U256::try_from_hex("0x26").unwrap(),
                    r: U256::try_from_hex(
                        "0x8ce4b169534418abbe9410e8fcdff4cd47e10265588b55dfa96c70f6fd62c6bf",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x23869888069d3b974aba703cf31a06eeed8466e9981697f54ec0709cd65b2ca4",
                    )
                    .unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    blob_versioned_hashes: Vec::new(),
                    authorization_list: Vec::new(),
                },
                const_hex::decode("f84980808080800126a08ce4b169534418abbe9410e8fcdff4cd47e10265588b55dfa96c70f6fd62c6bfa023869888069d3b974aba703cf31a06eeed8466e9981697f54ec0709cd65b2ca4").unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x1").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::try_from_hex("0x0").unwrap(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::default(),
                    max_priority_fee_per_gas: U256::default(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0x3ef69057fef8e5910debc8e52189c0eb57184cbe58415c58ac67d78b7c6d29ce",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x7673ba8e62f61bd1cd7f026d14c96d014e070b915a185948647fba29659ca352",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb84e01f84b01808080808080c001a03ef69057fef8e5910debc8e52189c0eb57184cbe58415c58ac67d78b7c6d29cea07673ba8e62f61bd1cd7f026d14c96d014e070b915a185948647fba29659ca352",
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x2").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0xc4dd060b048fc2b257e2a1e00ea3741884ca32b40e2ada3b70eec4f69bea1947",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x41949023f06ea394e9c2bfb5c02bf67ede6ec813c9e71a6936900aa676dd1050",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb84f02f84c0180808080808080c001a0c4dd060b048fc2b257e2a1e00ea3741884ca32b40e2ada3b70eec4f69bea1947a041949023f06ea394e9c2bfb5c02bf67ede6ec813c9e71a6936900aa676dd1050"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x3").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::try_from_hex("0x0").unwrap(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x0").unwrap(),
                    r: U256::try_from_hex(
                        "0x265974ddd1be7ef0cacd823784e994a8029a776becf760eea18a7a356f2e206c",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x351c528fba4ca69eaa850fc30f8e16bed9c083f2bb77ab89ef1c301de582c1e2",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb86503f86201808080809400000000000000000000000000000000000000008080c080c080a0265974ddd1be7ef0cacd823784e994a8029a776becf760eea18a7a356f2e206ca0351c528fba4ca69eaa850fc30f8e16bed9c083f2bb77ab89ef1c301de582c1e2"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x3").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_fee_per_blob_gas: U256::try_from_hex("0x0").unwrap(),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    blob_versioned_hashes: vec![
                        Hash::try_from_hex(
                            "0x0000000000000000000000000000000000000000000000000000000000000000",
                        )
                        .unwrap(),
                    ],
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x0").unwrap(),
                    r: U256::try_from_hex(
                        "0xea9aff2ec0c4b370ae14a055ffa0d7e5e3a00e039be41412548078c96a35cca5",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x36f768887cf167a25be29f6f127836517ea067193d4c07e7f147767d43d91d57",
                    )
                    .unwrap(),
                },
                const_hex::decode("b88603f88301808080809400000000000000000000000000000000000000008080c080e1a0000000000000000000000000000000000000000000000000000000000000000080a0ea9aff2ec0c4b370ae14a055ffa0d7e5e3a00e039be41412548078c96a35cca5a036f768887cf167a25be29f6f127836517ea067193d4c07e7f147767d43d91d57").unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x3").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: vec![AccessListEntry {
                        address: Address::try_from_hex("0x0000000000000000000000000000000000000000")
                            .unwrap(),
                        storage_keys: Vec::new(),
                    }],
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::try_from_hex("0x0").unwrap(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0xc11bf40b64864762c1a38f045ab45a19eefe17b21c2d508c11221b4e54889613",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x37e4802da391446247aaa73613f07fb7291864314d6788d20cf17a5b407e0330",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb87c03f87901808080809400000000000000000000000000000000000000008080d7d6940000000000000000000000000000000000000000c080c001a0c11bf40b64864762c1a38f045ab45a19eefe17b21c2d508c11221b4e54889613a037e4802da391446247aaa73613f07fb7291864314d6788d20cf17a5b407e0330"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x4").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0x81dcbcae18a4ca0e228c63d02a699c65653fe898581c1fe4f9b4a519e038b969",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x5fd2c190dd5001139230f62c133aa77407c77c86752b6408cbcff6a09a43401d",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb86404f86101808080809400000000000000000000000000000000000000008080c0c001a081dcbcae18a4ca0e228c63d02a699c65653fe898581c1fe4f9b4a519e038b969a05fd2c190dd5001139230f62c133aa77407c77c86752b6408cbcff6a09a43401d"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x4").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: vec![SetCodeAuthorization {
                        chain_id: U256::try_from_hex("0x0").unwrap(),
                        address: Address::try_from_hex("0x0000000000000000000000000000000000000000")
                            .unwrap(),
                        nonce: u64::try_from_hex("0x0").unwrap(),
                        y_parity: u8::try_from_hex("0x0").unwrap(),
                        r: U256::try_from_hex("0x0").unwrap(),
                        s: U256::try_from_hex("0x0").unwrap(),
                    }],
                    y_parity: U256::try_from_hex("0x0").unwrap(),
                    r: U256::try_from_hex(
                        "0x9ae41ab490c59fd1e1aae4df6a7d96931ae25e9f2120106dff8f8e6d079f6366",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x2d5bc1c108b11cda410cd7b42a342341c51012e4636a15c8c5ebb2fc5bed2962",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb87f04f87c01808080809400000000000000000000000000000000000000008080c0dbda809400000000000000000000000000000000000000008080808080a09ae41ab490c59fd1e1aae4df6a7d96931ae25e9f2120106dff8f8e6d079f6366a02d5bc1c108b11cda410cd7b42a342341c51012e4636a15c8c5ebb2fc5bed2962"
                ).unwrap()
            )
        ]
    }

    #[test]
    fn can_be_encoded_to_rlp() {
        for (tx, rlp) in generate_transactions_with_rlp() {
            let mut buf = Vec::new();
            tx.encode(&mut buf);
            assert_eq!(buf, rlp, "Encoded RLP should match expected value");
        }
    }

    #[test]
    fn can_be_decoded_from_rlp() {
        for (tx, rlp) in generate_transactions_with_rlp() {
            let decoded = Transaction::decode(&mut &rlp[..]).unwrap();
            assert_eq!(decoded, tx, "Decoded Transaction should match expected one");
        }
    }
}
