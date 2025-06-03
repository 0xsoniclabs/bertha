use alloy_rlp::RlpEncodable;
use ethbloom::{BloomRef, Input};
use serde::{Deserialize, Serialize};
use sha3::Digest;

use crate::types::{
    Address, BlockNumber, Bloom, Hash, SerializableByteVec, SerializableU64,
    serializable_byte_array::SerializableByteArray, u256::U256,
};

/// An Ethereum-compatible block header.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, RlpEncodable, PartialOrd, Ord,
)]
#[serde(rename_all = "camelCase")]
#[rlp(trailing)]
pub struct BlockHeader {
    // The field names correspond to the names returned in the JSON RPC payload.
    // Go-ethereum uses different names for some of them, see the comment on each line.
    pub parent_hash: Hash,               // ParentHash
    pub sha3_uncles: Hash,               // UncleHash
    pub miner: Address,                  // Coinbase
    pub state_root: Hash,                // Root
    pub transactions_root: Hash,         // TxHash
    pub receipts_root: Hash,             // ReceiptHash
    pub logs_bloom: Bloom,               // Bloom
    pub difficulty: U256,                // Difficulty
    pub number: BlockNumber,             // Number
    pub gas_limit: SerializableU64,      // GasLimit
    pub gas_used: SerializableU64,       // GasUsed
    pub timestamp: SerializableU64,      // Time
    pub extra_data: SerializableByteVec, // Extra
    pub mix_hash: Hash,                  // MixDigest

    /// The block nonce is a legacy field that was used for proof of work.
    /// For proof of stake it should always be "0x0000000000000000".
    ///
    /// NOTE: We don't use the [crate::types::Nonce] type for this field,
    /// because that uses a u64 RLP encoding, where this fields needs to be
    /// encoded to a fixed length array of 8 bytes.
    pub nonce: SerializableByteArray<8>, // Nonce

    // Optional fields that have been added by EIP-1559, EIP-4895 and EIP-4844 and may not be
    // present in older blocks.
    pub base_fee_per_gas: Option<U256>, // BaseFee
    pub withdrawals_root: Option<Hash>, // WithdrawalsHash
    #[serde(default)]
    pub blob_gas_used: Option<SerializableU64>, // BlobGasUsed
    #[serde(default)]
    pub excess_blob_gas: Option<SerializableU64>, // ExcessBlobGas
}

impl BlockHeader {
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

        let bloom = BloomRef::from(self.logs_bloom.as_bytes());
        if let Some(address) = address {
            let input = Input::Raw(address.as_bytes());
            may_contain &= bloom.contains_input(input);
        }
        for topic in topics {
            let input = Input::Raw(topic.as_bytes());
            may_contain &= bloom.contains_input(input);
        }

        may_contain
    }
}

#[cfg(test)]
mod tests {
    use ethbloom::Bloom as EthBloom;

    use super::*;

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
            header.sha3_uncles,
            Hash::try_from_hex(
                "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347"
            )
            .unwrap()
        );
        assert_eq!(
            header.miner,
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
        assert_eq!(
            header.number,
            BlockNumber::try_from_hex("0x11d5c59").unwrap()
        );
        assert_eq!(
            header.gas_limit,
            SerializableU64::try_from_hex("0x12a05f200").unwrap()
        );
        assert_eq!(
            header.gas_used,
            SerializableU64::try_from_hex("0x187e67").unwrap()
        );
        assert_eq!(
            header.timestamp,
            SerializableU64::try_from_hex("0x67f3a650").unwrap()
        );
        assert_eq!(
            header.extra_data,
            SerializableByteVec::try_from_hex("0x26a0531500000000125a1be0").unwrap()
        );
        assert_eq!(
            header.mix_hash,
            Hash::try_from_hex(
                "0xa6e19a868c8d649c9624a52842417e1ba84bc11024fbe8ef9c9c4c596ae59a1c"
            )
            .unwrap()
        );
        assert_eq!(
            header.nonce,
            SerializableByteArray::try_from_hex("0x0000000000000000").unwrap()
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
        assert_eq!(header.blob_gas_used, Some(0u64.into()));
        assert_eq!(header.excess_blob_gas, Some(0u64.into()));
    }

    #[test]
    fn optional_fields_are_handled_correctly() {
        let json: String = format!("{{{REQUIRED_FIELDS}}}");
        let header: BlockHeader = serde_json::from_str(json.as_str()).unwrap();
        assert_eq!(header.base_fee_per_gas, None);
        assert_eq!(header.withdrawals_root, None);
        assert_eq!(header.blob_gas_used, None);
        assert_eq!(header.excess_blob_gas, None);

        let json: String = format!("{{{REQUIRED_FIELDS}, \"baseFeelPerGas\": null }}");
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
        header.gas_used = SerializableU64::from(Into::<u64>::into(header.gas_used) + 1);
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
        bloom.accrue(Input::Raw(address.as_bytes()));
        for topic in &topics {
            bloom.accrue(Input::Raw(topic.as_bytes()));
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
                block.may_contain_logs(None, &[topic.clone()]),
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
