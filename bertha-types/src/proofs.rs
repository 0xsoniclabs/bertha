use serde::{Deserialize, Serialize};

use crate::types::{Address, Hash, Nonce, U256, Wei, serializable_byte_vec::SerializableByteVec};

/// This holds the current state a an address and the proof for the account and its associated
/// storage.]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct AccountProof {
    pub address: Address,
    pub account_proof: Vec<SerializableByteVec>,
    pub balance: Wei,
    pub code_hash: Hash,
    pub nonce: Nonce,
    pub storage_hash: Hash,
    pub storage_proof: Vec<StorageProof>,
}

/// A single storage entry and its proof.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StorageProof {
    pub key: Hash,
    // TODO: Revisit this type if / once we support querying storage values:
    // Storage values are arbitrary data up to 32 bytes. However, RPC responses use a variable
    // length encoding, including odd numbers of nibbles. This is currently not supported by the
    // SerializableByteVec type.
    pub value: U256,
    pub proof: Vec<SerializableByteVec>,
}
