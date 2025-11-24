use ethbloom::{Bloom, Input};
use serde::{Deserialize, Serialize};

use crate::{
    Address, AsHex, BlockHeader, EMPTY_OMMERS_HASH, EMPTY_TREE_ROOT_HASH, Hash, Transaction,
    TransactionReceipt, U256, compute_root_hash,
};

/// An Ethereum-compatible block in "normal form", that is, without any redundant or derived fields.
///
/// For example, it does not include fields such as `gas_used`, `transaction_root` and `logs_bloom`,
/// since these can all be computed from the contained transactions and receipts.
///
/// Moreover, it contains additional state root fields for alternative experimental data structures
/// which are not currently part of Ethereum.
///
/// Fields are named according to the Ethereum Yellow Paper (Shanghai version).
/// Go-ethereum and JSON RPC names, where they differ, are indicated through doc comments on each
/// field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "JsonBlock", into = "JsonBlock")]
pub struct Block {
    pub parent_hash: Hash,
    /// geth: UncleHash, JSON RPC: sha3Uncles
    pub ommers_hash: Hash,
    /// geth: Coinbase, JSON RPC: miner
    pub beneficiary: Address,
    /// geth: Root, JSON RPC: stateRoot
    pub state_root: Hash,
    pub difficulty: u64,
    pub number: u64,
    pub gas_limit: u64,
    /// geth: Time, JSON RPC: timestamp
    pub timestamp: u64,
    /// geth: Extra, JSON RPC: extraData
    pub extra_data: Vec<u8>,
    /// geth: MixDigest, JSON RPC: mixHash
    pub prev_randao: Hash,
    pub nonce: [u8; 8],

    pub transactions: Vec<Transaction>,
    pub receipts: Vec<TransactionReceipt>,

    /// Added by EIP-1559
    /// geth: BaseFee, JSON RPC: baseFeePerGas
    pub base_fee_per_gas: Option<U256>,

    /// Added by EIP-4895
    /// geth: WithdrawalsHash, JSON RPC: withdrawalsRoot
    pub withdrawals_root: Option<Hash>,

    /// Added by EIP-4844
    pub blob_gas_used: Option<u64>,

    /// Added by EIP-4844
    pub excess_blob_gas: Option<u64>,

    /// Added by EIP-4788
    pub parent_beacon_block_root: Option<Hash>,

    /// Added by EIP-7685
    pub requests_hash: Option<Hash>,

    // State roots for experimental data structures
    pub verkle_state_root: Option<Hash>,
    pub binary_state_root: Option<Hash>,
}

impl Block {
    /// Returns a new block with default values that upholds the invariants of Sonic.
    pub fn default_sonic() -> Self {
        Block {
            // in Sonic the ommers_hash is always set to the empty hash
            ommers_hash: EMPTY_OMMERS_HASH,
            // in Sonic the extra_data must be 12 bytes long because it holds the duration and
            // nanoseconds part of the timestamp
            extra_data: vec![0; 12],
            withdrawals_root: Some(EMPTY_TREE_ROOT_HASH),
            // in Sonic the base_fee_per_gas is always set, so default to 0 instead of None
            base_fee_per_gas: Some(U256::default()),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            ..Default::default()
        }
    }

    pub fn to_header(&self) -> BlockHeader {
        let mut logs_bloom = Bloom::zero();
        for receipt in &self.receipts {
            for log in &receipt.logs {
                logs_bloom.accrue(Input::Raw(&log.address));
                for topic in &log.topics {
                    logs_bloom.accrue(Input::Raw(topic));
                }
            }
        }

        let receipts_root = compute_root_hash(&self.receipts);
        let transactions_root = compute_root_hash(&self.transactions);

        let gas_used = self
            .receipts
            .last()
            .map(|tx| tx.cumulative_gas_used)
            .unwrap_or_default();

        BlockHeader {
            parent_hash: self.parent_hash,
            ommers_hash: self.ommers_hash,
            beneficiary: self.beneficiary,
            state_root: self.state_root,
            transactions_root,
            receipts_root,
            logs_bloom: logs_bloom.0,
            difficulty: U256::from(self.difficulty),
            number: self.number,
            gas_limit: self.gas_limit,
            gas_used,
            timestamp: self.timestamp,
            extra_data: self.extra_data.clone(),
            prev_randao: self.prev_randao,
            nonce: self.nonce,
            base_fee_per_gas: self.base_fee_per_gas,
            withdrawals_root: self.withdrawals_root,
            blob_gas_used: self.blob_gas_used,
            excess_blob_gas: self.excess_blob_gas,
        }
    }

    pub fn from_header_and_transactions_and_receipts(
        header: BlockHeader,
        transactions: Vec<Transaction>,
        receipts: Vec<TransactionReceipt>,
    ) -> Self {
        Block {
            parent_hash: header.parent_hash,
            ommers_hash: header.ommers_hash,
            beneficiary: header.beneficiary,
            state_root: header.state_root,
            difficulty: header.difficulty.to_least_significant_u64(),
            number: header.number,
            gas_limit: header.gas_limit,
            timestamp: header.timestamp,
            extra_data: header.extra_data,
            prev_randao: header.prev_randao,
            nonce: header.nonce,
            transactions,
            receipts,
            base_fee_per_gas: header.base_fee_per_gas,
            withdrawals_root: header.withdrawals_root,
            blob_gas_used: header.blob_gas_used,
            excess_blob_gas: header.excess_blob_gas,
            parent_beacon_block_root: None,
            requests_hash: None,
            verkle_state_root: None,
            binary_state_root: None,
        }
    }
}

/// The JSON representation of a [Block].
///
/// While fields are named after the Ethereum JSON RPC types, the combination of fields does not
/// directly correspond to any existing RPC payload: There is no method for obtaining the block
/// header, transactions and receipts within a single response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonBlock {
    pub parent_hash: AsHex<Hash>,
    pub sha3_uncles: AsHex<Hash>,
    pub miner: AsHex<Address>,
    pub state_root: AsHex<Hash>,
    pub difficulty: AsHex<u64>,
    pub number: AsHex<u64>,
    pub gas_limit: AsHex<u64>,
    pub timestamp: AsHex<u64>,
    pub extra_data: AsHex<Vec<u8>>,
    pub mix_hash: AsHex<Hash>,
    pub nonce: AsHex<[u8; 8]>,
    pub transactions: Vec<Transaction>,
    pub receipts: Vec<TransactionReceipt>,
    pub base_fee_per_gas: Option<AsHex<U256>>,
    pub withdrawals_root: Option<AsHex<Hash>>,
    pub blob_gas_used: Option<AsHex<u64>>,
    pub excess_blob_gas: Option<AsHex<u64>>,
    pub parent_beacon_block_root: Option<AsHex<Hash>>,
    pub requests_hash: Option<AsHex<Hash>>,
}

impl From<Block> for JsonBlock {
    fn from(block: Block) -> Self {
        JsonBlock {
            parent_hash: AsHex(block.parent_hash),
            sha3_uncles: AsHex(block.ommers_hash),
            miner: AsHex(block.beneficiary),
            state_root: AsHex(block.state_root),
            difficulty: AsHex(block.difficulty),
            number: AsHex(block.number),
            gas_limit: AsHex(block.gas_limit),
            timestamp: AsHex(block.timestamp),
            extra_data: AsHex(block.extra_data),
            mix_hash: AsHex(block.prev_randao),
            nonce: AsHex(block.nonce),
            transactions: block.transactions,
            receipts: block.receipts,
            base_fee_per_gas: block.base_fee_per_gas.map(AsHex),
            withdrawals_root: block.withdrawals_root.map(AsHex),
            blob_gas_used: block.blob_gas_used.map(AsHex),
            excess_blob_gas: block.excess_blob_gas.map(AsHex),
            parent_beacon_block_root: block.parent_beacon_block_root.map(AsHex),
            requests_hash: block.requests_hash.map(AsHex),
        }
    }
}

impl From<JsonBlock> for Block {
    fn from(json_block: JsonBlock) -> Self {
        Block {
            parent_hash: json_block.parent_hash.0,
            ommers_hash: json_block.sha3_uncles.0,
            beneficiary: json_block.miner.0,
            state_root: json_block.state_root.0,
            difficulty: json_block.difficulty.0,
            number: json_block.number.0,
            gas_limit: json_block.gas_limit.0,
            timestamp: json_block.timestamp.0,
            extra_data: json_block.extra_data.0,
            prev_randao: json_block.mix_hash.0,
            nonce: json_block.nonce.0,
            transactions: json_block.transactions,
            receipts: json_block.receipts,
            base_fee_per_gas: json_block.base_fee_per_gas.map(|v| v.0),
            withdrawals_root: json_block.withdrawals_root.map(|v| v.0),
            blob_gas_used: json_block.blob_gas_used.map(|v| v.0),
            excess_blob_gas: json_block.excess_blob_gas.map(|v| v.0),
            parent_beacon_block_root: json_block.parent_beacon_block_root.map(|v| v.0),
            requests_hash: json_block.requests_hash.map(|v| v.0),
            verkle_state_root: None,
            binary_state_root: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use crate::{
        Block, Transaction, TransactionReceipt,
        test_data::test_data_blocks::generate_blocks_with_data,
    };

    #[test]
    fn block_to_header_to_hash_produces_correct_hash() {
        for data in generate_blocks_with_data() {
            let block = data.block;
            let hash = data.block_hash;
            assert_eq!(block.to_header().compute_hash(), hash)
        }
    }

    #[test]
    fn block_to_header_and_then_from_header_and_transactions_and_receipts_is_identity() {
        for data in generate_blocks_with_data() {
            let block = data.block;
            let header = block.to_header();
            let transactions = block.transactions.clone();
            let receipts = block.receipts.clone();

            let new_block =
                Block::from_header_and_transactions_and_receipts(header, transactions, receipts);
            assert_eq!(new_block, block);
        }
    }

    #[test]
    fn can_be_serialized_to_json() {
        for data in generate_blocks_with_data() {
            let block = data.block;

            let mut expected = serde_json::from_str::<Value>(&data.json_representation).unwrap();
            let expected_fields = expected.as_object_mut().unwrap();

            let serialized_block = serde_json::to_value(&block).unwrap();

            for (key, value) in serialized_block.as_object().unwrap() {
                // skip optional fields that are null because they are not present in the
                // expected JSON representation
                let opts = [
                    "baseFeePerGas",
                    "withdrawalsRoot",
                    "blobGasUsed",
                    "excessBlobGas",
                    "parentBeaconBlockRoot",
                    "requestsHash",
                ];
                if opts.contains(&key.as_str()) && value.is_null() {
                    continue;
                }

                let mut expected = expected_fields.get(key).unwrap().clone();
                if key == "transactions" {
                    // remove unused fields
                    expected = serde_json::to_value(
                        serde_json::from_value::<Vec<Transaction>>(expected).unwrap(),
                    )
                    .unwrap();
                } else if key == "receipts" {
                    // remove unused fields
                    expected = serde_json::to_value(
                        serde_json::from_value::<Vec<TransactionReceipt>>(expected).unwrap(),
                    )
                    .unwrap();
                }
                assert_eq!(*value, expected);
            }
        }
    }

    #[test]
    fn can_be_deserialized_from_json() {
        for data in generate_blocks_with_data() {
            let deserialized: Block = serde_json::from_str(&data.json_representation)
                .expect("Deserialization from JSON should not fail");
            assert_eq!(deserialized, data.block);
        }
    }
}
