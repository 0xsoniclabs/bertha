use alloy_rlp::RlpEncodable;
use serde::{Deserialize, Serialize};

use crate::{Address, Hash, SerializableByteVec, SerializableU64};

#[derive(Debug, Clone, Deserialize, Serialize, RlpEncodable, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct Log {
    pub address: Address,
    pub topics: Vec<Hash>,
    pub data: SerializableByteVec,
    //#[rlp(skip)]
    //pub block_number: BlockNumber,
    //#[rlp(skip)]
    //pub transaction_hash: Hash,
    #[rlp(skip)]
    pub transaction_index: SerializableU64,
    //#[rlp(skip)]
    //pub block_hash: Hash,
    //#[rlp(skip)]
    //pub log_index: SerializableU64,
    //#[rlp(skip)]
    //pub removed: bool,
}
