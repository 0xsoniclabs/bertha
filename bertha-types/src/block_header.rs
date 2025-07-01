use alloy_rlp::{Decodable, Encodable};
use ethbloom::{BloomRef, Input};
use serde::{Deserialize, Serialize};
use sha3::Digest;

use crate::{Address, AsHex, Bloom, Hash, U256};

/// An Ethereum-compatible block header.
///
/// Fields are named according to the Ethereum Yellow Paper (Shanghai version).
/// Go-ethereum and JSON RPC names, where they differ, are indicated through doc comments on each
/// field.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(from = "JsonRpcBlockHeader", into = "JsonRpcBlockHeader")]
pub struct BlockHeader {
    pub parent_hash: Hash,
    /// geth: UncleHash, JSON RPC: sha3Uncles
    pub ommers_hash: Hash,
    /// geth: Coinbase, JSON RPC: miner
    pub beneficiary: Address,
    /// geth: Root, JSON RPC: stateRoot
    pub state_root: Hash,
    /// geth: TxHash, JSON RPC: transactionsRoot
    pub transactions_root: Hash,
    /// geth: ReceiptHash, JSON RPC: receiptsRoot
    pub receipts_root: Hash,
    pub logs_bloom: Bloom,
    pub difficulty: U256,
    pub number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    /// geth: Time, JSON RPC: timestamp
    pub timestamp: u64,
    /// geth: Extra, JSON RPC: extraData
    pub extra_data: Vec<u8>,
    /// geth: MixDigest, JSON RPC: mixHash
    pub prev_randao: Hash,

    /// The block nonce is a legacy field that was used for proof of work.
    /// For proof of stake it should always be "0x0000000000000000".
    ///
    /// NOTE: We don't use a [u64] type for this field,
    /// because that uses a variable length RLP encoding, where this fields needs to be
    /// encoded to a fixed length array of 8 bytes.
    pub nonce: [u8; 8],

    // Optional fields that have been added by EIP-1559, EIP-4895 and EIP-4844 and may not be
    // present in older blocks.
    /// geth: BaseFee, JSON RPC: baseFeePerGas
    pub base_fee_per_gas: Option<U256>,
    /// geth: WithdrawalsHash, JSON RPC: withdrawalsRoot
    pub withdrawals_root: Option<Hash>,
    pub blob_gas_used: Option<u64>,
    pub excess_blob_gas: Option<u64>,
}

impl Default for BlockHeader {
    fn default() -> Self {
        Self {
            parent_hash: Hash::default(),
            ommers_hash: Hash::default(),
            beneficiary: Address::default(),
            state_root: Hash::default(),
            transactions_root: Hash::default(),
            receipts_root: Hash::default(),
            logs_bloom: [0; 256],
            difficulty: U256::default(),
            number: u64::default(),
            gas_limit: 0,
            gas_used: 0,
            timestamp: 0,
            extra_data: Vec::new(),
            prev_randao: Hash::default(),
            nonce: <[u8; 8]>::default(),
            base_fee_per_gas: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
        }
    }
}

impl Encodable for BlockHeader {
    fn length(&self) -> usize {
        let payload_length = self.alloy_rlp_payload_length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }

    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        alloy_rlp::Header {
            list: true,
            payload_length: self.alloy_rlp_payload_length(),
        }
        .encode(out);
        Encodable::encode(&self.parent_hash, out);
        Encodable::encode(&self.ommers_hash, out);
        Encodable::encode(&self.beneficiary, out);
        Encodable::encode(&self.state_root, out);
        Encodable::encode(&self.transactions_root, out);
        Encodable::encode(&self.receipts_root, out);
        Encodable::encode(&self.logs_bloom, out);
        Encodable::encode(&self.difficulty, out);
        Encodable::encode(&self.number, out);
        Encodable::encode(&self.gas_limit, out);
        Encodable::encode(&self.gas_used, out);
        Encodable::encode(&self.timestamp, out);
        Encodable::encode(&self.extra_data.as_slice(), out); // <- needs custom encoding
        Encodable::encode(&self.prev_randao, out);
        Encodable::encode(&self.nonce, out);
        if let Some(val) = self.base_fee_per_gas.as_ref() {
            Encodable::encode(val, out)
        } else if self.withdrawals_root.is_some()
            || self.blob_gas_used.is_some()
            || self.excess_blob_gas.is_some()
        {
            out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
        }
        if let Some(val) = self.withdrawals_root.as_ref() {
            Encodable::encode(val, out)
        } else if self.blob_gas_used.is_some() || self.excess_blob_gas.is_some() {
            out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
        }
        if let Some(val) = self.blob_gas_used.as_ref() {
            Encodable::encode(val, out)
        } else if self.excess_blob_gas.is_some() {
            out.put_u8(alloy_rlp::EMPTY_STRING_CODE);
        }
        if let Some(val) = self.excess_blob_gas.as_ref() {
            Encodable::encode(val, out)
        }
    }
}

impl Decodable for BlockHeader {
    fn decode(b: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let alloy_rlp::Header {
            list,
            payload_length,
        } = alloy_rlp::Header::decode(b)?;
        if !list {
            return Err(alloy_rlp::Error::UnexpectedString);
        }
        let started_len = b.len();
        if started_len < payload_length {
            return Err(alloy_rlp::Error::InputTooShort);
        }
        let this = Self {
            parent_hash: Decodable::decode(b)?,
            ommers_hash: Decodable::decode(b)?,
            beneficiary: Decodable::decode(b)?,
            state_root: Decodable::decode(b)?,
            transactions_root: Decodable::decode(b)?,
            receipts_root: Decodable::decode(b)?,
            logs_bloom: Decodable::decode(b)?,
            difficulty: Decodable::decode(b)?,
            number: Decodable::decode(b)?,
            gas_limit: Decodable::decode(b)?,
            gas_used: Decodable::decode(b)?,
            timestamp: Decodable::decode(b)?,
            extra_data: alloy_rlp::Header::decode_bytes(b, false)?.to_vec(), // custom
            prev_randao: Decodable::decode(b)?,
            nonce: Decodable::decode(b)?,
            base_fee_per_gas: if started_len - b.len() < payload_length {
                if alloy_rlp::private::Option::map_or(b.first(), false, |b| {
                    *b == alloy_rlp::EMPTY_STRING_CODE
                }) {
                    alloy_rlp::Buf::advance(b, 1);
                    None
                } else {
                    Some(Decodable::decode(b)?)
                }
            } else {
                None
            },
            withdrawals_root: if started_len - b.len() < payload_length {
                if alloy_rlp::private::Option::map_or(b.first(), false, |b| {
                    *b == alloy_rlp::EMPTY_STRING_CODE
                }) {
                    alloy_rlp::Buf::advance(b, 1);
                    None
                } else {
                    Some(Decodable::decode(b)?)
                }
            } else {
                None
            },
            blob_gas_used: if started_len - b.len() < payload_length {
                if alloy_rlp::private::Option::map_or(b.first(), false, |b| {
                    *b == alloy_rlp::EMPTY_STRING_CODE
                }) {
                    alloy_rlp::Buf::advance(b, 1);
                    None
                } else {
                    Some(Decodable::decode(b)?)
                }
            } else {
                None
            },
            excess_blob_gas: if started_len - b.len() < payload_length {
                if alloy_rlp::private::Option::map_or(b.first(), false, |b| {
                    *b == alloy_rlp::EMPTY_STRING_CODE
                }) {
                    alloy_rlp::Buf::advance(b, 1);
                    None
                } else {
                    Some(Decodable::decode(b)?)
                }
            } else {
                None
            },
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

impl BlockHeader {
    fn alloy_rlp_payload_length(&self) -> usize {
        Encodable::length(&self.parent_hash)
            + Encodable::length(&self.ommers_hash)
            + Encodable::length(&self.beneficiary)
            + Encodable::length(&self.state_root)
            + Encodable::length(&self.transactions_root)
            + Encodable::length(&self.receipts_root)
            + Encodable::length(&self.logs_bloom)
            + Encodable::length(&self.difficulty)
            + Encodable::length(&self.number)
            + Encodable::length(&self.gas_limit)
            + Encodable::length(&self.gas_used)
            + Encodable::length(&self.timestamp)
            + Encodable::length(&self.extra_data.as_slice()) // custom
            + Encodable::length(&self.prev_randao)
            + Encodable::length(&self.nonce)
            + self
                .base_fee_per_gas
                .as_ref()
                .map(Encodable::length)
                .unwrap_or(
                    (self.withdrawals_root.is_some()
                        || self.blob_gas_used.is_some()
                        || self.excess_blob_gas.is_some()) as usize,
                )
            + self
                .withdrawals_root
                .as_ref()
                .map(Encodable::length)
                .unwrap_or(
                    (self.blob_gas_used.is_some() || self.excess_blob_gas.is_some()) as usize,
                )
            + self
                .blob_gas_used
                .as_ref()
                .map(Encodable::length)
                .unwrap_or((self.excess_blob_gas.is_some()) as usize)
            + self
                .excess_blob_gas
                .as_ref()
                .map(Encodable::length)
                .unwrap_or(0)
    }

    pub fn compute_hash(&self) -> Hash {
        let rlp = alloy_rlp::encode(self);
        let mut hasher = sha3::Keccak256::new();
        hasher.update(rlp);
        let bytes: [u8; 32] = hasher.finalize().into();
        Hash::from(bytes)
    }

    /// Checks if it is possible that the block contains logs for the given address and topics.
    /// This may have false positives, but it is guaranteed to not have false negatives.
    pub fn may_contain_logs(&self, address: Option<&Address>, topics: &[Hash]) -> bool {
        let mut may_contain = true;

        let bloom = BloomRef::from(&self.logs_bloom);
        if let Some(address) = address {
            let input = Input::Raw(address);
            may_contain &= bloom.contains_input(input);
        }
        for topic in topics {
            let input = Input::Raw(topic);
            may_contain &= bloom.contains_input(input);
        }

        may_contain
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcBlockHeader {
    pub parent_hash: AsHex<Hash>,
    pub sha3_uncles: AsHex<Hash>,
    pub miner: AsHex<Address>,
    pub state_root: AsHex<Hash>,
    pub transactions_root: AsHex<Hash>,
    pub receipts_root: AsHex<Hash>,
    pub logs_bloom: AsHex<Bloom>,
    pub difficulty: AsHex<U256>,
    pub number: AsHex<u64>,
    pub gas_limit: AsHex<u64>,
    pub gas_used: AsHex<u64>,
    pub timestamp: AsHex<u64>,
    pub extra_data: AsHex<Vec<u8>>,
    pub mix_hash: AsHex<Hash>,
    pub nonce: AsHex<[u8; 8]>,
    pub base_fee_per_gas: Option<AsHex<U256>>,
    pub withdrawals_root: Option<AsHex<Hash>>,
    pub blob_gas_used: Option<AsHex<u64>>,
    pub excess_blob_gas: Option<AsHex<u64>>,
    // Fields that are part of the JSON RPC response but we currently don't use:
    // pub timestamp_nano: AsHex<u64>,
    // pub hash: AsHex<Hash>,
    // pub epoch: AsHex<u64>,
    // pub total_difficulty: AsHex<u64>,
    // pub transactions: Vec<?> // Type of this depends on query parameter (either hash or struct)
    // pub size: AsHex<u64>,
    // pub uncles: Vec<?>
}

impl From<JsonRpcBlockHeader> for BlockHeader {
    fn from(value: JsonRpcBlockHeader) -> Self {
        Self {
            parent_hash: value.parent_hash.0,
            ommers_hash: value.sha3_uncles.0,
            beneficiary: value.miner.0,
            state_root: value.state_root.0,
            transactions_root: value.transactions_root.0,
            receipts_root: value.receipts_root.0,
            logs_bloom: value.logs_bloom.0,
            difficulty: value.difficulty.0,
            number: value.number.0,
            gas_limit: value.gas_limit.0,
            gas_used: value.gas_used.0,
            timestamp: value.timestamp.0,
            extra_data: value.extra_data.0,
            prev_randao: value.mix_hash.0,
            nonce: value.nonce.0,
            base_fee_per_gas: value.base_fee_per_gas.map(|v| v.0),
            withdrawals_root: value.withdrawals_root.map(|v| v.0),
            blob_gas_used: value.blob_gas_used.map(|v| v.0),
            excess_blob_gas: value.excess_blob_gas.map(|v| v.0),
        }
    }
}

impl From<BlockHeader> for JsonRpcBlockHeader {
    fn from(value: BlockHeader) -> Self {
        Self {
            parent_hash: AsHex(value.parent_hash),
            sha3_uncles: AsHex(value.ommers_hash),
            miner: AsHex(value.beneficiary),
            state_root: AsHex(value.state_root),
            transactions_root: AsHex(value.transactions_root),
            receipts_root: AsHex(value.receipts_root),
            logs_bloom: AsHex(value.logs_bloom),
            difficulty: AsHex(value.difficulty),
            number: AsHex(value.number),
            gas_limit: AsHex(value.gas_limit),
            gas_used: AsHex(value.gas_used),
            timestamp: AsHex(value.timestamp),
            extra_data: AsHex(value.extra_data),
            mix_hash: AsHex(value.prev_randao),
            nonce: AsHex(value.nonce),
            base_fee_per_gas: value.base_fee_per_gas.map(AsHex),
            withdrawals_root: value.withdrawals_root.map(AsHex),
            blob_gas_used: value.blob_gas_used.map(AsHex),
            excess_blob_gas: value.excess_blob_gas.map(AsHex),
        }
    }
}

#[cfg(test)]
mod tests {
    use ethbloom::Bloom as EthBloom;

    use super::*;
    use crate::hex_convert::HexConvert;

    const REQUIRED_FIELDS: &str = r#"
        "parentHash": "0x4849bafd75ec931bd8b95e168ad52aa45eb942a7b0e294825b77696f95d33f67",
        "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
        "miner": "0x0000000000000000000000000000000000000000",
        "stateRoot": "0x794ae8d5b4758807b0a043bf938ea7c3cbbaccfca400e2e6c5496f9974be45b5",
        "transactionsRoot": "0x4caf573d465dd16b43dd8e7472e6d29cba30ccda22746f0273d10fd3360f17f8",
        "receiptsRoot": "0x5f3e0100ab247f520f5a123792f54d8e15f44e9f1e00c43a28f40aad2897a1b1",
        "logsBloom": "0x01200000000000400000102000008080000000010000080000000000402010000200100000000801001000400008000008010000000000000400000000800000000000800100000000402008004000000000000000000000500000000002000000000000020000000000020000000800000820000800000200000210000010000000200208000000820000000000000000000000000100000000000000000000000000000000000000000800000400000000040000010001000000003040000000000002200000800008080000000000000040000000000000000080000020100000000000000000040010100000000000000100050000000000800020008800",
        "difficulty": "0x0",
        "number": "0x11d5c59",
        "gasLimit": "0x12a05f200",
        "gasUsed": "0x187e67",
        "timestamp": "0x67f3a650",
        "extraData": "0x26a0531500000000125a1be0",
        "mixHash": "0xa6e19a868c8d649c9624a52842417e1ba84bc11024fbe8ef9c9c4c596ae59a1c",
        "nonce": "0x0000000000000000"
    "#;

    const OPTIONAL_FIELDS: &str = r#"
        "baseFeePerGas": "0xba43b7400",
        "withdrawalsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
        "blobGasUsed": "0x0",
        "excessBlobGas": "0x0"
    "#;

    // These fields are not part of the header, but are returned by the JSON RPC call.
    const EXTRA_FIELDS: &str = r#"
        "timestampNano": "0x183401ece3fb7315",
        "hash": "0x3b8b3b13b1073259b5e13a62e3efe5a16357a204f849f1adbf1bc458c8fb30aa",
        "epoch": "0x4a1a",
        "totalDifficulty": "0x0",
        "transactions": [
            "0x8d41973d6c0cee0f05c90754e11a4b6d0dbeb4bd5e3241f0d17a2c9f3230a4ce",
            "0x381be7451fca044510e55d2a7455b9e4957bf120f0efbcae7009a1f9e3fe157a",
            "0x0a95247404d3f0ef57c2acbf75998fc8ba5de81a0929a668ff0c40fbb20ed61e",
            "0x09f8f42adcab11eb80586c6f735ea67006e6246e09b5e2267acd7af5f3bf0dba"
        ],
        "size": "0x1923",
        "uncles": []
    "#;

    #[test]
    fn can_be_deserialized_from_json() {
        let json: String = format!("{{{REQUIRED_FIELDS},{OPTIONAL_FIELDS},{EXTRA_FIELDS}}}");
        let header: BlockHeader = serde_json::from_str(json.as_str()).unwrap();
        assert_eq!(
            header.parent_hash,
            Hash::try_from_hex(
                "0x4849bafd75ec931bd8b95e168ad52aa45eb942a7b0e294825b77696f95d33f67"
            )
            .unwrap()
        );
        assert_eq!(
            header.ommers_hash,
            Hash::try_from_hex(
                "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"
            )
            .unwrap()
        );
        assert_eq!(
            header.beneficiary,
            Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap()
        );
        assert_eq!(
            header.state_root,
            Hash::try_from_hex(
                "0x794ae8d5b4758807b0a043bf938ea7c3cbbaccfca400e2e6c5496f9974be45b5"
            )
            .unwrap()
        );
        assert_eq!(
            header.transactions_root,
            Hash::try_from_hex(
                "0x4caf573d465dd16b43dd8e7472e6d29cba30ccda22746f0273d10fd3360f17f8"
            )
            .unwrap()
        );
        assert_eq!(
            header.receipts_root,
            Hash::try_from_hex(
                "0x5f3e0100ab247f520f5a123792f54d8e15f44e9f1e00c43a28f40aad2897a1b1"
            )
            .unwrap()
        );
        assert_eq!(
            header.logs_bloom,
            Bloom::try_from_hex(
                "0x01200000000000400000102000008080000000010000080000000000402010000200100000000801001000400008000008010000000000000400000000800000000000800100000000402008004000000000000000000000500000000002000000000000020000000000020000000800000820000800000200000210000010000000200208000000820000000000000000000000000100000000000000000000000000000000000000000800000400000000040000010001000000003040000000000002200000800008080000000000000040000000000000000080000020100000000000000000040010100000000000000100050000000000800020008800"
            ).unwrap()
        );
        assert_eq!(header.difficulty, U256::try_from_hex("0x0").unwrap());
        assert_eq!(header.number, u64::try_from_hex("0x11d5c59").unwrap());
        assert_eq!(header.gas_limit, u64::try_from_hex("0x12a05f200").unwrap());
        assert_eq!(header.gas_used, u64::try_from_hex("0x187e67").unwrap());
        assert_eq!(header.timestamp, u64::try_from_hex("0x67f3a650").unwrap());
        assert_eq!(
            header.extra_data,
            Vec::try_from_hex("0x26a0531500000000125a1be0").unwrap()
        );
        assert_eq!(
            header.prev_randao,
            Hash::try_from_hex(
                "0xa6e19a868c8d649c9624a52842417e1ba84bc11024fbe8ef9c9c4c596ae59a1c"
            )
            .unwrap()
        );
        assert_eq!(
            header.nonce,
            <[u8; 8]>::try_from_hex("0x0000000000000000").unwrap()
        );
        assert_eq!(
            header.base_fee_per_gas,
            Some(U256::try_from_hex("0xba43b7400").unwrap())
        );
        assert_eq!(
            header.withdrawals_root,
            Some(
                Hash::try_from_hex(
                    "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"
                )
                .unwrap()
            )
        );
        assert_eq!(header.blob_gas_used, Some(0));
        assert_eq!(header.excess_blob_gas, Some(0));
    }

    #[test]
    fn can_be_serialized_to_json() {
        let header: BlockHeader = BlockHeader {
            parent_hash: Hash::try_from_hex("0x4849bafd75ec931bd8b95e168ad52aa45eb942a7b0e294825b77696f95d33f67").unwrap(),
            ommers_hash: Hash::try_from_hex("0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347").unwrap(),
            beneficiary: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            state_root: Hash::try_from_hex("0x794ae8d5b4758807b0a043bf938ea7c3cbbaccfca400e2e6c5496f9974be45b5").unwrap(),
            transactions_root: Hash::try_from_hex("0x4caf573d465dd16b43dd8e7472e6d29cba30ccda22746f0273d10fd3360f17f8").unwrap(),
            receipts_root: Hash::try_from_hex("0x5f3e0100ab247f520f5a123792f54d8e15f44e9f1e00c43a28f40aad2897a1b1").unwrap(),
            logs_bloom: Bloom::try_from_hex(
                "0x01200000000000400000102000008080000000010000080000000000402010000200100000000801001000400008000008010000000000000400000000800000000000800100000000402008004000000000000000000000500000000002000000000000020000000000020000000800000820000800000200000210000010000000200208000000820000000000000000000000000100000000000000000000000000000000000000000800000400000000040000010001000000003040000000000002200000800008080000000000000040000000000000000080000020100000000000000000040010100000000000000100050000000000800020008800"
            ).unwrap(),
            difficulty: U256::try_from_hex("0x0").unwrap(),
            number: u64::try_from_hex("0x11d5c59").unwrap(),
            gas_limit: u64::try_from_hex("0x12a05f200").unwrap(),
            gas_used: u64::try_from_hex("0x187e67").unwrap(),
            timestamp: u64::try_from_hex("0x67f3a650").unwrap(),
            extra_data: Vec::try_from_hex("0x26a0531500000000125a1be0").unwrap(),
            prev_randao: Hash::try_from_hex("0xa6e19a868c8d649c9624a52842417e1ba84bc11024fbe8ef9c9c4c596ae59a1c").unwrap(),
            nonce: <[u8; 8]>::try_from_hex("0x0000000000000000").unwrap(),
            base_fee_per_gas: Some(U256::try_from_hex("0xba43b7400").unwrap()),
            withdrawals_root: Some(Hash::try_from_hex("0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421").unwrap()),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
        };
        let serialized = serde_json::to_string(&header).unwrap();
        let json: String = format!("{{{REQUIRED_FIELDS},{OPTIONAL_FIELDS}}}");
        let json = json.replace(" ", "").replace("\n", "");
        assert_eq!(serialized, json);
    }

    #[test]
    fn can_be_encoded_to_rlp() {
        let mut header = BlockHeader {
            parent_hash: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            ommers_hash: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            beneficiary: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            state_root: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            transactions_root: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            receipts_root: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            logs_bloom: Bloom::try_from_hex("0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            difficulty: U256::default(),
            number: 0,
            gas_limit: u64::try_from_hex("0x0").unwrap(),
            gas_used: u64::try_from_hex("0x0").unwrap(),
            timestamp: u64::try_from_hex("0x0").unwrap(),
            extra_data: Vec::try_from_hex("0x").unwrap(),
            prev_randao: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            nonce: <[u8; 8]>::try_from_hex("0x0000000000000000").unwrap(),
            base_fee_per_gas: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
        };
        let rlp = const_hex::decode(
            "f901eda00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000940000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000808080808080a00000000000000000000000000000000000000000000000000000000000000000880000000000000000"
            ).unwrap();
        assert_eq!(alloy_rlp::encode(&header), rlp.as_slice());

        header.extra_data = Vec::try_from_hex("0x01").unwrap();
        let rlp = const_hex::decode(
            "f901eda00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000940000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000808080808001a00000000000000000000000000000000000000000000000000000000000000000880000000000000000"
            ).unwrap();
        assert_eq!(alloy_rlp::encode(&header), rlp.as_slice());
    }

    #[test]
    fn can_be_decoded_from_rlp() {
        let mut header = BlockHeader {
            parent_hash: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            ommers_hash: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            beneficiary: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            state_root: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            transactions_root: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            receipts_root: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            logs_bloom: Bloom::try_from_hex("0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            difficulty: U256::default(),
            number: 0,
            gas_limit: u64::try_from_hex("0x0").unwrap(),
            gas_used: u64::try_from_hex("0x0").unwrap(),
            timestamp: u64::try_from_hex("0x0").unwrap(),
            extra_data: Vec::try_from_hex("0x").unwrap(),
            prev_randao: Hash::try_from_hex("0x0000000000000000000000000000000000000000000000000000000000000000").unwrap(),
            nonce: <[u8; 8]>::try_from_hex("0x0000000000000000").unwrap(),
            base_fee_per_gas: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
        };
        let rlp = const_hex::decode(
            "f901eda00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000940000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000808080808080a00000000000000000000000000000000000000000000000000000000000000000880000000000000000"
            ).unwrap();
        assert_eq!(
            alloy_rlp::decode_exact::<BlockHeader>(&rlp).unwrap(),
            header
        );

        header.extra_data = Vec::try_from_hex("0x01").unwrap();
        let rlp = const_hex::decode(
            "f901eda00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000940000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000a00000000000000000000000000000000000000000000000000000000000000000b9010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000808080808001a00000000000000000000000000000000000000000000000000000000000000000880000000000000000"
            ).unwrap();
        assert_eq!(
            alloy_rlp::decode_exact::<BlockHeader>(&rlp).unwrap(),
            header
        );
    }

    #[test]
    fn optional_fields_are_handled_correctly() {
        let json: String = format!("{{{REQUIRED_FIELDS}}}");
        let header: BlockHeader = serde_json::from_str(json.as_str()).unwrap();
        assert_eq!(header.base_fee_per_gas, None);
        assert_eq!(header.withdrawals_root, None);
        assert_eq!(header.blob_gas_used, None);
        assert_eq!(header.excess_blob_gas, None);

        let json: String = format!("{{{REQUIRED_FIELDS}, \"baseFeePerGas\": null }}");
        let header: BlockHeader = serde_json::from_str(json.as_str()).unwrap();
        assert_eq!(header.base_fee_per_gas, None);
    }

    #[test]
    fn compute_hash_produces_correct_hash() {
        let json: String = format!("{{{REQUIRED_FIELDS},{OPTIONAL_FIELDS}}}");
        let header: BlockHeader = serde_json::from_str(json.as_str()).unwrap();
        let hash = header.compute_hash();
        assert_eq!(
            hash.to_hex(),
            "0x3b8b3b13b1073259b5e13a62e3efe5a16357a204f849f1adbf1bc458c8fb30aa"
        );

        let mut header = header;
        header.gas_used += 1;
        assert_ne!(header.compute_hash(), hash);
    }

    #[test]
    fn may_contain_logs_checks_if_all_filters_are_fulfilled() {
        // This test data was taken from real blocks on the sonic network, but the bloom filter is
        // recomputed to not rely on the obtained data.
        // block_number = "0x1484794" / 21514132
        // block_hash = "0x5c5a7d3c48608460ab478e149d415153d1fb0d340512c7b87b94c80e53615d66"

        let address = Address::try_from_hex("0xc3ec2c370860fa71360db5277386e9aad36a99d9").unwrap();
        let topics = vec![
            Hash::try_from_hex(
                "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
            )
            .unwrap(),
            Hash::try_from_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            Hash::try_from_hex(
                "0x000000000000000000000000932ce0cbcd156c624d63ec351e14efd5dcc4af1a",
            )
            .unwrap(),
            Hash::try_from_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000061",
            )
            .unwrap(),
        ];

        let mut bloom = EthBloom::zero();
        bloom.accrue(Input::Raw(&address));
        for topic in &topics {
            bloom.accrue(Input::Raw(topic.as_slice()));
        }

        let block = BlockHeader {
            logs_bloom: Bloom::from(bloom.to_fixed_bytes()),
            ..Default::default()
        };

        assert!(
            block.may_contain_logs(None, &[]),
            "filter does not match no address and empty topics"
        );
        assert!(
            block.may_contain_logs(Some(&address), &[]),
            "filter does not match address which is contained in it"
        );
        for topic in &topics {
            assert!(
                block.may_contain_logs(None, &[*topic]),
                "filter does not match topic which is contained in it"
            );
        }
        assert!(
            block.may_contain_logs(None, &topics),
            "filter does not match topics which are contained in it"
        );
        assert!(
            block.may_contain_logs(Some(&address), &topics),
            "filter does not match address and topics which are contained in it"
        );

        // In theory these calls could also return true because false positives are possible, but in
        // practice they do not.
        assert!(
            !block.may_contain_logs(
                Some(&Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap()),
                &[]
            ),
            "filter matches zero address although it is not contained in it"
        );
        assert!(
            !block.may_contain_logs(
                Some(&Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap()),
                &topics,
            ),
            "filter matches zero address and topics although the address is not contained in it"
        );
        assert!(
            !block.may_contain_logs(
                None,
                &[Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000001"
                )
                .unwrap(),]
            ),
            "filter matches topic although it is contained in it"
        );
        assert!(
            !block.may_contain_logs(
                Some(&address),
                &[Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000001"
                )
                .unwrap()]
            ),
            "filter matches address and topic although the topic is not contained in it"
        );
    }
}
