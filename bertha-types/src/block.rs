use ethbloom::{Bloom, Input};

use crate::{
    Address, BlockHeader, EMPTY_OMMERS_HASH, EMPTY_TREE_ROOT_HASH, Hash, HexConvert, Transaction,
    TransactionReceipt, U256, compute_root_hash,
};

/// An Ethereum-compatible block in "normal form", that is, without any redundant or derived fields.
///
/// For example, it does not include fields such as `gas_used`, `transaction_root` and `logs_bloom`,
/// since these can all be computed from the contained transactions and receipts.
///
/// Fields are named according to the Ethereum Yellow Paper (Shanghai version).
/// Go-ethereum and JSON RPC names, where they differ, are indicated through doc comments on each
/// field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
}

impl Block {
    /// Returns a new block with default values that upholds the invariants of Sonic.
    pub fn default_sonic() -> Self {
        Block {
            // in Sonic the ommers_hash is always set to the empty hash
            ommers_hash: Hash::try_from_hex(EMPTY_OMMERS_HASH).unwrap(),
            // in Sonic the extra_data must be 12 bytes long because it holds the duration and
            // nanoseconds part of the timestamp
            extra_data: vec![0; 12],
            withdrawals_root: Some(Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap()),
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
            sha3_uncles: self.ommers_hash,
            miner: self.beneficiary,
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
            mix_hash: self.prev_randao,
            nonce: self.nonce,
            base_fee_per_gas: self.base_fee_per_gas,
            withdrawals_root: self.withdrawals_root,
            blob_gas_used: self.blob_gas_used,
            excess_blob_gas: self.excess_blob_gas,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_data::test_data_blocks::generate_blocks_with_data;

    #[test]
    fn block_to_header_to_hash_produces_correct_hash() {
        for data in generate_blocks_with_data() {
            let block = data.block;
            let hash = data.block_hash;
            assert_eq!(block.to_header().compute_hash(), hash)
        }
    }
}
