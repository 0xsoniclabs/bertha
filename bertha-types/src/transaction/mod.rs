// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

mod access_list_tx;
mod blob_tx;
mod dynamic_fee_tx;
mod error;
mod legacy_tx;
mod set_code_tx;

use alloy_rlp::{Decodable, Encodable};
use serde::{Deserialize, Serialize};

pub use crate::transaction::{
    access_list_tx::AccessListEntry, error::TransactionError, set_code_tx::SetCodeAuthorization,
};
use crate::{
    Address, AsHex, Hash, HexConvert, RlpNil, RlpString, U256,
    eip_2718_utils::{EIP2718Unmarshallable, Eip2718Marshallable},
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
            blob_versioned_hashes: Vec::default(),
            max_fee_per_blob_gas: U256::default(),
            authorization_list: Vec::default(),
            y_parity: U256::default(),
            r: U256::default(),
            s: U256::default(),
        }
    }
}

impl Eip2718Marshallable for Transaction {
    fn marshal(&self) -> Vec<u8> {
        fn encode_with_type_as_rlp_string<T: Encodable>(
            transaction_type: TransactionType,
            tx: T,
            out: &mut dyn alloy_rlp::BufMut,
        ) {
            out.put_u8(transaction_type as u8);
            tx.encode(out);
        }

        let mut out = Vec::new();
        // In case the conversion to one of the inner types fails (the transaction is invalid),
        // we encode an empty string to make sure decoding fails when this data is ingested
        // again. This is essentially a trash in -> trash out policy.
        match self.transaction_type {
            TransactionType::Legacy => LegacyTx::try_from(self.clone())
                .map(|tx| tx.encode(&mut out))
                .unwrap_or_else(|_| "".encode(&mut out)),
            TransactionType::AccessList => AccessListTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, &mut out))
                .unwrap_or_else(|_| "".encode(&mut out)),
            TransactionType::DynamicFee => DynamicFeeTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, &mut out))
                .unwrap_or_else(|_| "".encode(&mut out)),
            TransactionType::Blob => BlobTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, &mut out))
                .unwrap_or_else(|_| "".encode(&mut out)),
            TransactionType::SetCode => SetCodeTx::try_from(self.clone())
                .map(|tx| encode_with_type_as_rlp_string(self.transaction_type, tx, &mut out))
                .unwrap_or_else(|_| "".encode(&mut out)),
        };

        out
    }
}

impl EIP2718Unmarshallable for Transaction {
    fn unmarshal(buf: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        if buf.is_empty() {
            return Err(alloy_rlp::Error::InputTooShort);
        }
        let type_ = buf[0];
        if type_ > 0x7f {
            Ok(LegacyTx::decode(buf)?.into())
        } else {
            *buf = &buf[1..];
            match type_ {
                1 => Ok(AccessListTx::decode(buf)?.into()),
                2 => Ok(DynamicFeeTx::decode(buf)?.into()),
                3 => Ok(BlobTx::decode(buf)?.into()),
                4 => Ok(SetCodeTx::decode(buf)?.into()),
                _ => Err(alloy_rlp::Error::Custom("invalid transaction type")),
            }
        }
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
        u8::from_str_radix(value.trim_start_matches("0x"), 16)
            .map_err(ParseHexError::from)?
            .try_into()
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
            to: value.to.map(|to| to.0),
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
    use alloy_rlp::Header;
    use rstest::rstest;

    use super::*;
    use crate::{
        HexConvert,
        eip_2718_utils::verify,
        test_data::test_data_transaction::{TRANSACTION_ROOT, generate_transactions_with_data},
    };

    #[rstest]
    #[case::legacy_tx_with_to_field(TransactionType::Legacy, true, true)]
    #[case::legacy_tx_without_to_field(TransactionType::Legacy, false, true)]
    #[case::access_list_tx_with_to_field(TransactionType::AccessList, true, true)]
    #[case::access_list_tx_without_to_field(TransactionType::AccessList, false, true)]
    #[case::dynamic_fee_tx_with_to_field(TransactionType::DynamicFee, true, true)]
    #[case::dynamic_fee_tx_without_to_field(TransactionType::DynamicFee, false, true)]
    #[case::blob_tx_with_to_field(TransactionType::Blob, true, true)]
    #[case::blob_tx_without_to_field(TransactionType::Blob, false, false)]
    #[case::set_code_tx_with_to_field(TransactionType::SetCode, true, true)]
    #[case::set_code_tx_without_to_field(TransactionType::SetCode, false, false)]
    fn is_valid_correctly_checks_transaction(
        #[case] transaction_type: TransactionType,
        #[case] has_to: bool,
        #[case] expect_valid: bool,
    ) {
        let mut transaction = make_transaction(transaction_type, true);
        transaction.to = has_to.then_some(Address::default());

        assert_eq!(transaction.is_valid().is_ok(), expect_valid);
    }

    #[rstest]
    #[case::legacy_tx_with_to_field(TransactionType::Legacy, true, true)]
    #[case::legacy_tx_without_to_field(TransactionType::Legacy, false, true)]
    #[case::access_list_tx_with_to_field(TransactionType::AccessList, true, true)]
    #[case::access_list_tx_without_to_field(TransactionType::AccessList, false, true)]
    #[case::dynamic_fee_tx_with_to_field(TransactionType::DynamicFee, true, true)]
    #[case::dynamic_fee_tx_without_to_field(TransactionType::DynamicFee, false, true)]
    #[case::blob_tx_with_to_field(TransactionType::Blob, true, true)]
    #[case::blob_tx_without_to_field(TransactionType::Blob, false, false)]
    #[case::set_code_tx_with_to_field(TransactionType::SetCode, true, true)]
    #[case::set_code_tx_without_to_field(TransactionType::SetCode, false, false)]
    fn can_be_serialized_to_json_rpc_format_if_valid(
        #[case] transaction_type: TransactionType,
        #[case] has_to: bool,
        #[case] expect_valid: bool,
    ) {
        let mut transaction = make_transaction(transaction_type, true);
        transaction.to = has_to.then_some(Address::default());

        let res = serde_json::to_string(&transaction);
        if expect_valid {
            assert!(res.is_ok());
            let expected_json = match transaction_type {
                TransactionType::Legacy => make_json_legacy_tx(has_to),
                TransactionType::AccessList => make_json_access_list_tx(has_to),
                TransactionType::DynamicFee => make_json_dynamic_fee_tx(has_to),
                TransactionType::Blob => make_json_blob_tx(),
                TransactionType::SetCode => make_json_set_code_tx(),
            };

            assert_eq!(to_value(&res.unwrap()), expected_json);
        } else {
            assert!(res.is_err());
        }
    }

    #[rstest::rstest]
    #[case::null_value(r#"{"value": null}"#, 0u32)]
    #[case::non_null_value(r#"{"value": 1}"#, 1u32)]
    fn deserialize_null_handles_null_values(#[case] json_str: &str, #[case] expected: u32) {
        #[derive(Deserialize, Debug)]
        struct TestDeserializeNull {
            #[serde(deserialize_with = "deserialize_null")]
            value: u32,
        }

        let deserialized: TestDeserializeNull = serde_json::from_str(json_str).unwrap();
        assert_eq!(deserialized.value, expected);
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

    #[rstest::rstest]
    #[case::invalid_transaction_type(make_json_transaction_with_invalid_type())]
    #[case::blob_tx_without_to(make_json_transaction(TransactionType::Blob, false))]
    #[case::set_code_tx_without_to(make_json_transaction(TransactionType::SetCode, false))]
    fn deserialize_fails_for_invalid_transactions(#[case] json_value: serde_json::Value) {
        let res: Result<Transaction, _> = serde_json::from_value(json_value);
        assert!(res.is_err());
    }

    fn to_value(json_str: &str) -> serde_json::Value {
        serde_json::from_str(json_str).unwrap()
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

    impl Transaction {
        /// Canonicalizes the transaction by converting it to the appropriate specialized type
        /// and back, thereby resetting all unused fields to their default values.
        pub fn canonicalize(self) -> Result<Self, TransactionError> {
            match self.transaction_type {
                TransactionType::Legacy => Ok(LegacyTx::try_from(self)?.into()),
                TransactionType::AccessList => Ok(AccessListTx::try_from(self)?.into()),
                TransactionType::DynamicFee => Ok(DynamicFeeTx::try_from(self)?.into()),
                TransactionType::Blob => Ok(BlobTx::try_from(self)?.into()),
                TransactionType::SetCode => Ok(SetCodeTx::try_from(self)?.into()),
            }
        }
    }

    #[test]
    fn can_be_encoded_to_rlp() {
        for value in generate_transactions_with_data() {
            let buf = value.transaction.canonicalize().unwrap().marshal();
            assert_eq!(
                buf, value.rlp_encoding,
                "Encoded RLP should match expected value"
            );
        }
    }

    #[test]
    fn encodes_invalid_transaction_to_empty_string() {
        let invalid_blob_tx = make_transaction(TransactionType::Blob, false);
        let buf = invalid_blob_tx.marshal();
        assert_eq!(buf, [alloy_rlp::EMPTY_STRING_CODE]);

        let invalid_set_code_tx = make_transaction(TransactionType::SetCode, false);
        let buf = invalid_set_code_tx.marshal();
        assert_eq!(buf, [alloy_rlp::EMPTY_STRING_CODE]);
    }

    #[test]
    fn can_be_decoded_from_rlp() {
        for value in generate_transactions_with_data() {
            let mut rlp = value.rlp_encoding.as_slice();
            let decoded = Transaction::unmarshal(&mut rlp).unwrap();
            assert_eq!(
                decoded,
                value.transaction.canonicalize().unwrap(),
                "Decoded Transaction should match expected one"
            );
        }
    }

    #[test]
    fn fails_to_decode_when_transaction_type_is_invalid() {
        let tx = make_transaction(TransactionType::AccessList, false);
        let mut marshalled = tx.marshal();
        let mut rlp = marshalled.as_slice();
        Header::decode(&mut rlp).unwrap();
        let header_len = marshalled.len() - rlp.len();
        // next next byte is used for the transaction type
        marshalled[header_len] = 0x05; // Set an invalid transaction type
        let decoded = Transaction::unmarshal(&mut &marshalled[..]);
        assert_eq!(
            decoded,
            Err(alloy_rlp::Error::Custom("invalid transaction type"))
        );
    }

    #[test]
    fn transactions_can_be_verified() {
        let transactions = generate_transactions_with_data()
            .into_iter()
            .map(|v| v.transaction)
            .collect::<Vec<_>>();
        let res = verify(
            &transactions,
            &Hash::try_from_hex(TRANSACTION_ROOT).unwrap(),
        );
        assert!(res.is_ok(), "Block transactions should verify successfully");
    }

    #[test]
    fn can_be_serialized_and_deserialized_from_json() {
        for value in generate_transactions_with_data() {
            let json_value = serde_json::to_value(&value.transaction)
                .expect("JSON serialization should not fail");
            let expected = serde_json::to_value(
                serde_json::from_str::<Transaction>(&value.json_representation)
                    .expect("JSON Deserialization should not fail"),
            )
            .unwrap(); // Safe to unwrap as it's the inverse of the deserialization
            assert_eq!(
                json_value, expected,
                "JSON serialization should match expected value"
            );
        }
    }
}
