use alloy_rlp::{Decodable, Header, RlpDecodable};

use crate::{Address, Hash, Nonce, SerializableByteVec, U256, Wei};

// Source: go-ethereum/core/types/transaction.go (Transaction, TxData)
// The only field of Transaction that gets decoded is `Inner` which is of type `TxData`.
// Therefore, we use TxData directly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Transaction {
    Legacy(LegacyTx),
    AccessList(AccessListTx),
    DynamicFee(DynamicFeeTx),
    Blob(BlobTx),
    SetCode(SetCodeTx),
}

// Source: go-ethereum/core/types/transaction.go (DecodeRLP)
impl Decodable for Transaction {
    fn decode(rlp: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        let header = Header::decode(rlp)?;
        let tx = if header.list {
            Transaction::Legacy(LegacyTx::decode(rlp, header.payload_length)?)
        } else {
            if rlp.is_empty() {
                return Err(alloy_rlp::Error::InputTooShort);
            }
            // Source: go-ethereum/core/types/transaction.go (decodeTyped)
            let type_ = rlp[0];
            let mut inner_data = &rlp[1..header.payload_length];
            *rlp = &rlp[header.payload_length..];
            match type_ {
                1 => Transaction::AccessList(AccessListTx::decode(&mut inner_data)?),
                2 => Transaction::DynamicFee(DynamicFeeTx::decode(&mut inner_data)?),
                3 => Transaction::Blob(BlobTx::decode(&mut inner_data)?),
                4 => Transaction::SetCode(SetCodeTx::decode(&mut inner_data)?),
                _ => {
                    return Err(alloy_rlp::Error::Custom("invalid transaction type"));
                }
            }
        };
        Ok(tx)
    }
}

// Source: go-ethereum/core/types/tx_legacy.go (LegacyTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LegacyTx {
    pub nonce: Nonce,
    pub gas_price: U256,
    pub gas: u64,
    pub to: Nil<Address>,
    pub value: Wei,
    pub data: SerializableByteVec,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

impl LegacyTx {
    // This is essentially [Decodable::decode] but because the header was parsed already in
    // [Transaction::decode], it can not be read again. However, the payload length is needed
    // so we have to pass it explicitly.
    fn decode(b: &mut &[u8], payload_length: usize) -> alloy_rlp::Result<Self> {
        let started_len = b.len();
        if started_len < payload_length {
            return Err(alloy_rlp::Error::InputTooShort);
        }
        let this = Self {
            nonce: alloy_rlp::Decodable::decode(b)?,
            gas_price: alloy_rlp::Decodable::decode(b)?,
            gas: alloy_rlp::Decodable::decode(b)?,
            to: alloy_rlp::Decodable::decode(b)?,
            value: alloy_rlp::Decodable::decode(b)?,
            data: alloy_rlp::Decodable::decode(b)?,
            v: alloy_rlp::Decodable::decode(b)?,
            r: alloy_rlp::Decodable::decode(b)?,
            s: alloy_rlp::Decodable::decode(b)?,
        };
        let consumed = started_len - b.len();
        if consumed != payload_length {
            return Err(alloy_rlp::Error::ListLengthMismatch {
                expected: payload_length,
                got: consumed,
            });
        }
        Ok(this)
    }
}

// Source: go-ethereum/core/types/tx_access_list.go (AccessListTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, RlpDecodable)]
pub struct AccessListTx {
    pub chain_id: U256,
    pub nonce: Nonce,
    pub gas_price: U256,
    pub gas: u64,
    pub to: Nil<Address>,
    pub value: Wei,
    pub data: SerializableByteVec,
    pub access_list: Vec<AccessTuple>,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

// Source: go-ethereum/core/types/tx_access_list.go (AccessTuple)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, RlpDecodable)]
pub struct AccessTuple {
    pub address: Address,
    pub storage_keys: Vec<Hash>,
}

// Source: go-ethereum/core/types/tx_dynamic_fee.go (DynamicFeeTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, RlpDecodable)]
pub struct DynamicFeeTx {
    pub chain_id: U256,
    pub nonce: Nonce,
    pub gas_tip_cap: U256,
    pub gas_fee_cap: U256,
    pub gas: u64,
    pub to: Nil<Address>,
    pub value: Wei,
    pub data: SerializableByteVec,
    pub access_list: Vec<AccessTuple>,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

// Source: go-ethereum/core/types/tx_blob.go (BlobTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, RlpDecodable)]
pub struct BlobTx {
    pub chain_id: U256,
    pub nonce: Nonce,
    pub gas_tip_cap: U256,
    pub gas_fee_cap: U256,
    pub gas: u64,
    pub to: Nil<Address>,
    pub value: Wei,
    pub data: SerializableByteVec,
    pub access_list: Vec<AccessTuple>,
    pub blob_fee_cap: U256,
    pub blob_hashes: Vec<Hash>,
    // sidecar is not included in the RLP encoding
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

// Source: go-ethereum/core/types/tx_set_code.go (SetCodeTx)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, RlpDecodable)]
pub struct SetCodeTx {
    pub chain_id: U256,
    pub nonce: Nonce,
    pub gas_tip_cap: U256,
    pub gas_fee_cap: U256,
    pub gas: u64,
    pub to: Nil<Address>,
    pub value: Wei,
    pub data: SerializableByteVec,
    pub access_list: Vec<AccessTuple>,
    pub auth_list: Vec<SetCodeAuthorization>,
    pub v: U256,
    pub r: U256,
    pub s: U256,
}

// Source: go-ethereum/core/types/tx_set_code.go (SetCodeAuthorization)
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, RlpDecodable)]
pub struct SetCodeAuthorization {
    pub chain_id: U256,
    pub address: Address,
    pub nonce: Nonce,
    pub v: u8, // yParity
    pub r: U256,
    pub s: U256,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Nil<T>(pub Option<T>);

impl<T: Decodable> Decodable for Nil<T> {
    fn decode(from: &mut &[u8]) -> Result<Self, alloy_rlp::Error> {
        if from.starts_with(&[0x80]) {
            *from = &from[1..];
            Ok(Nil(None))
        } else {
            Ok(Nil(Some(T::decode(from)?)))
        }
    }
}
