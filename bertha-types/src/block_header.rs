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

use alloy_rlp::{Decodable, Encodable, RlpDecodable, RlpEncodable};
use ethbloom::{BloomRef, Input};
use serde::{Deserialize, Serialize};

use crate::{Address, AsHex, Bloom, Hash, RlpString, U256};

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

    // Optional fields that have been added by later hard forks and may not be present in older
    // blocks.
    /// geth: BaseFee, JSON RPC: baseFeePerGas. Added by EIP-1559 (London).
    pub base_fee_per_gas: Option<U256>,
    /// geth: WithdrawalsHash, JSON RPC: withdrawalsRoot. Added by EIP-4895 (Shanghai).
    pub withdrawals_root: Option<Hash>,
    /// Added by EIP-4844 (Cancun).
    pub blob_gas_used: Option<u64>,
    /// Added by EIP-4844 (Cancun).
    pub excess_blob_gas: Option<u64>,
    /// geth: ParentBeaconRoot. Added by EIP-4788 (Cancun). Only present on Ethereum, not on Sonic.
    pub parent_beacon_block_root: Option<Hash>,
    /// Added by EIP-7685 (Prague). Only present on Ethereum, not on Sonic.
    pub requests_hash: Option<Hash>,
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
            gas_limit: u64::default(),
            gas_used: u64::default(),
            timestamp: u64::default(),
            extra_data: Vec::new(),
            prev_randao: Hash::default(),
            nonce: <[u8; 8]>::default(),
            base_fee_per_gas: Option::default(),
            withdrawals_root: Option::default(),
            blob_gas_used: Option::default(),
            excess_blob_gas: Option::default(),
            parent_beacon_block_root: Option::default(),
            requests_hash: Option::default(),
        }
    }
}

impl Encodable for BlockHeader {
    fn length(&self) -> usize {
        RlpBlockHeader::from(self).length()
    }

    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        RlpBlockHeader::from(self).encode(out)
    }
}

impl Decodable for BlockHeader {
    fn decode(b: &mut &[u8]) -> alloy_rlp::Result<Self> {
        RlpBlockHeader::<RlpString>::decode(b).map(Self::from)
    }
}

impl BlockHeader {
    pub fn compute_hash(&self) -> Hash {
        let rlp = alloy_rlp::encode(self);
        alloy_primitives::keccak256(rlp).0
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

/// The RLP representation of a [BlockHeader].
///
/// The extra data must be encoded as a RLP string, not as the list that [`Vec<u8>`] would produce.
/// It is generic so encoding can borrow it as [`&[u8]`](slice) while decoding owns an [RlpString].
#[derive(RlpEncodable, RlpDecodable)]
#[rlp(trailing)]
struct RlpBlockHeader<E> {
    pub parent_hash: Hash,
    pub ommers_hash: Hash,
    pub beneficiary: Address,
    pub state_root: Hash,
    pub transactions_root: Hash,
    pub receipts_root: Hash,
    pub logs_bloom: Bloom,
    pub difficulty: U256,
    pub number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub extra_data: E,
    pub prev_randao: Hash,
    pub nonce: [u8; 8],
    pub base_fee_per_gas: Option<U256>,
    pub withdrawals_root: Option<Hash>,
    pub blob_gas_used: Option<u64>,
    pub excess_blob_gas: Option<u64>,
    pub parent_beacon_block_root: Option<Hash>,
    pub requests_hash: Option<Hash>,
}

impl<'a> From<&'a BlockHeader> for RlpBlockHeader<&'a [u8]> {
    fn from(value: &'a BlockHeader) -> Self {
        Self {
            parent_hash: value.parent_hash,
            ommers_hash: value.ommers_hash,
            beneficiary: value.beneficiary,
            state_root: value.state_root,
            transactions_root: value.transactions_root,
            receipts_root: value.receipts_root,
            logs_bloom: value.logs_bloom,
            difficulty: value.difficulty,
            number: value.number,
            gas_limit: value.gas_limit,
            gas_used: value.gas_used,
            timestamp: value.timestamp,
            extra_data: value.extra_data.as_slice(),
            prev_randao: value.prev_randao,
            nonce: value.nonce,
            base_fee_per_gas: value.base_fee_per_gas,
            withdrawals_root: value.withdrawals_root,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            parent_beacon_block_root: value.parent_beacon_block_root,
            requests_hash: value.requests_hash,
        }
    }
}

impl From<RlpBlockHeader<RlpString>> for BlockHeader {
    fn from(value: RlpBlockHeader<RlpString>) -> Self {
        Self {
            parent_hash: value.parent_hash,
            ommers_hash: value.ommers_hash,
            beneficiary: value.beneficiary,
            state_root: value.state_root,
            transactions_root: value.transactions_root,
            receipts_root: value.receipts_root,
            logs_bloom: value.logs_bloom,
            difficulty: value.difficulty,
            number: value.number,
            gas_limit: value.gas_limit,
            gas_used: value.gas_used,
            timestamp: value.timestamp,
            extra_data: value.extra_data.0,
            prev_randao: value.prev_randao,
            nonce: value.nonce,
            base_fee_per_gas: value.base_fee_per_gas,
            withdrawals_root: value.withdrawals_root,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            parent_beacon_block_root: value.parent_beacon_block_root,
            requests_hash: value.requests_hash,
        }
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
    pub parent_beacon_block_root: Option<AsHex<Hash>>,
    pub requests_hash: Option<AsHex<Hash>>,
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
            parent_beacon_block_root: value.parent_beacon_block_root.map(|v| v.0),
            requests_hash: value.requests_hash.map(|v| v.0),
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
            parent_beacon_block_root: value.parent_beacon_block_root.map(AsHex),
            requests_hash: value.requests_hash.map(AsHex),
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

    // Optional fields which only exist on Ethereum, but not on Sonic.
    const ETHEREUM_ONLY_FIELDS: &str = r#"
        "parentBeaconBlockRoot": "0x73e56887babcbf9a6396c5b5507d473a83779e6d0463ecf12c66d230a830b327",
        "requestsHash": "0xe3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
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

    /// Ethereum mainnet block 19434267, a Cancun block carrying `parentBeaconBlockRoot`
    /// (EIP-4788) but no `requestsHash`.
    const CANCUN_HEADER: &str = r#"{
        "parentHash": "0x459782063a33c29a1d55318874442faed23b9b37740dc490bdb0f7983603dea6",
        "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
        "miner": "0x95222290dd7278aa3ddd389cc1e1d165cc4bafe5",
        "stateRoot": "0x2fcf575e93545ea023ed54c3be8ddf46ff8128d2a821809c0e52319f3131fdec",
        "transactionsRoot": "0x3d6d9a820e8e755c3c8fa60601816c54a8ce0dc5e306268e18159603aa4e86eb",
        "receiptsRoot": "0xfd4f23d5fa89bc86c6f20260afadb7c2d461ecd39730ba89e3cdc19bc19dcf6a",
        "logsBloom": "0x3ae766736d80b3f39cb0dbb2df5cdf2bd53ff574afdb6726e62df971e6af653bb4463d59833bee0d7dd51f227c5fa38f5ebf05ddd9a7ecf9c7d7b0c0946e6b8c5592d7a949f4abe8eb3b5eff55aeff64d7f54a999d5fdaca373d5e7fbf7ddc33f6db6625e2c68e430f8f54d875583e5bf67f95a1557effaf9bda4cfefefd1dd6bf7ed3f997ed8de7becfff4b8973163f85198defe5e3cf9c5d7eacfe1fdab5b9eb84055e5caa6ed7aeff56d7db71f5ffd7a356a8d9bb129c6caf459734a9f358c336b52e00b317dbc5f8b9ae09b7bee692f1617a5dac785e13aa4167d1bef93e3dfdbe9ca69f4ffc94d596cba75a9ec2e6fd55a87bb6677b05ad5b9b7f9d57cf",
        "difficulty": "0x0",
        "number": "0x1288e1b",
        "gasLimit": "0x1c9c380",
        "gasUsed": "0x16f880a",
        "timestamp": "0x65f342b7",
        "extraData": "0x6265617665726275696c642e6f7267",
        "mixHash": "0x78da915db44b61353205b61bbc4439216d9651bd83b2044a3f13e970b78312db",
        "nonce": "0x0000000000000000",
        "baseFeePerGas": "0xb3c312478",
        "withdrawalsRoot": "0xa491643859ad876b419011c7e10374e961cd0fdff1267a6f3a638ffd10dedcee",
        "blobGasUsed": "0x80000",
        "excessBlobGas": "0xc0000",
        "parentBeaconBlockRoot": "0x73e56887babcbf9a6396c5b5507d473a83779e6d0463ecf12c66d230a830b327"
    }"#;
    const CANCUN_HEADER_HASH: &str =
        "0x8769113ab476aff022937753117ed909c5aea7f40e11d097a206401f8e41fc94";

    /// Ethereum mainnet block 22675756, a Prague block carrying both `parentBeaconBlockRoot`
    /// (EIP-4788) and `requestsHash` (EIP-7685).
    const PRAGUE_HEADER: &str = r#"{
        "parentHash": "0x3d3bb9edd78e83b6ebfe9dd07c1faea49dc7e0d5cc2497cd5b550ba5b47e99c6",
        "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
        "miner": "0x388c818ca8b9251b393131c08a736a67ccb19297",
        "stateRoot": "0xf44f9db805886f1c65c42c908241fe85b46d3523fff4120f6a4a6f7aca01d50d",
        "transactionsRoot": "0x6ddec6df63618af12f718cf65b6039606bdfa3125719ed38478319aafc8f3d9d",
        "receiptsRoot": "0x4defc47576887aa2370e9278b721f1ca91d5cfa47887610ee8356e28991fdd54",
        "logsBloom": "0x77ec580001350878817008b2b21c14ceb08d2e5b80c98004e7bdc0f4e051800b0830c101b4500a1412180f20432283855a80c11a8d047087241190003764138088440a42422d0bab689cc11a98028a7540a00402124c88e3053421c88800000048d7921e022204e205e8cc530c178d70040b00754841c6d085233f916c2b931501c408298775c03f68ed100741923360a8a0304111c004080428a950603063241e0dad0c40946ca20d1cd4e05180ac18414a8208ee804510c8800074294648083415290a1646a001002a330296805672452240c200241710800d1b2260546e90883aad818425c1a6206cc03508d1880de20e40e4401f1658098800ec0239e266",
        "difficulty": "0x0",
        "number": "0x15a012c",
        "gasLimit": "0x2255100",
        "gasUsed": "0x8aad38",
        "timestamp": "0x68486fcf",
        "extraData": "0x",
        "mixHash": "0xd61fff411bd67bc199b6f6c127eecc2c3974553e8a960c7be89b24645c8ed682",
        "nonce": "0x0000000000000000",
        "baseFeePerGas": "0x9de40dd0",
        "withdrawalsRoot": "0x9f2111cee1c2804747c0ec617a919a831bb1ce6cd5f56fe3775ac35644ec29e4",
        "blobGasUsed": "0x120000",
        "excessBlobGas": "0x0",
        "parentBeaconBlockRoot": "0x179ce54d3a9dc03f7b44337d6bfe72e74588260ba69883360c5f81d2f8eec6be",
        "requestsHash": "0xe3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }"#;
    const PRAGUE_HEADER_HASH: &str =
        "0x8232bfb917cb96aad4f553f6daf82cc3b11bc0734434467cf89b8a18365db112";

    #[test]
    fn can_be_deserialized_from_json() {
        let json: String = format!(
            "{{{REQUIRED_FIELDS},{OPTIONAL_FIELDS},{ETHEREUM_ONLY_FIELDS},{EXTRA_FIELDS}}}"
        );
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
        assert_eq!(
            header.parent_beacon_block_root,
            Some(
                Hash::try_from_hex(
                    "0x73e56887babcbf9a6396c5b5507d473a83779e6d0463ecf12c66d230a830b327"
                )
                .unwrap()
            )
        );
        assert_eq!(
            header.requests_hash,
            Some(
                Hash::try_from_hex(
                    "0xe3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                )
                .unwrap()
            )
        );
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
            parent_beacon_block_root: Some(Hash::try_from_hex("0x73e56887babcbf9a6396c5b5507d473a83779e6d0463ecf12c66d230a830b327").unwrap()),
            requests_hash: Some(Hash::try_from_hex("0xe3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855").unwrap()),
        };
        let serialized = serde_json::to_string(&header).unwrap();
        let json: String =
            format!("{{{REQUIRED_FIELDS},{OPTIONAL_FIELDS},{ETHEREUM_ONLY_FIELDS}}}");
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
            parent_beacon_block_root: None,
            requests_hash: None,
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
            parent_beacon_block_root: None,
            requests_hash: None,
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

    #[rstest::rstest]
    #[case::none(None, None, None, None, "")]
    #[case::first(Some(U256::from(7u64)), None, None, None, "07")]
    #[case::second(None, Some([0x11; 32]), None, None, "80a01111111111111111111111111111111111111111111111111111111111111111")]
    #[case::third(None, None, Some(9), None, "808009")]
    #[case::fourth(None, None, None, Some(3), "80808003")]
    #[case::first_third(Some(U256::from(7u64)), None, Some(9), None, "078009")]
    fn optional_rlp_fields_are_encoded_positionally(
        #[case] base_fee_per_gas: Option<U256>,
        #[case] withdrawals_root: Option<Hash>,
        #[case] blob_gas_used: Option<u64>,
        #[case] excess_blob_gas: Option<u64>,
        #[case] encoded_suffix: &str,
    ) {
        let encoded_suffix = const_hex::decode(encoded_suffix).unwrap();
        let header = BlockHeader {
            base_fee_per_gas,
            withdrawals_root,
            blob_gas_used,
            excess_blob_gas,
            ..Default::default()
        };
        let encoded = alloy_rlp::encode(&header);
        assert_eq!(
            encoded_suffix,
            encoded[encoded.len() - encoded_suffix.len()..]
        );
        assert_eq!(encoded.len(), header.length());
        assert_eq!(
            header,
            alloy_rlp::decode_exact::<BlockHeader>(&encoded).unwrap()
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
        assert_eq!(header.parent_beacon_block_root, None);
        assert_eq!(header.requests_hash, None);

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

    #[rstest::rstest]
    #[case::cancun(CANCUN_HEADER, CANCUN_HEADER_HASH)]
    #[case::prague(PRAGUE_HEADER, PRAGUE_HEADER_HASH)]
    fn compute_hash_produces_correct_hash_for_ethereum_headers(
        #[case] header_json: &str,
        #[case] expected_hash: &str,
    ) {
        let header: BlockHeader = serde_json::from_str(header_json).unwrap();
        assert_eq!(header.compute_hash().to_hex(), expected_hash);
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
            logs_bloom: bloom.to_fixed_bytes(),
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
