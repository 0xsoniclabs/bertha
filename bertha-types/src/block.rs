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
    use super::*;
    use crate::{HexConvert, Log, TransactionType};

    #[test]
    fn block_to_header_to_hash_produces_correct_hash() {
        let block = Block {
            parent_hash: [
                175, 145, 74, 3, 220, 253, 177, 54, 16, 25, 174, 47, 73, 140, 18, 237, 184, 41,
                161, 70, 198, 26, 37, 25, 201, 91, 81, 53, 92, 107, 233, 121,
            ],
            ommers_hash: [
                29, 204, 77, 232, 222, 199, 93, 122, 171, 133, 181, 103, 182, 204, 212, 26, 211,
                18, 69, 27, 148, 138, 116, 19, 240, 161, 66, 253, 64, 212, 147, 71,
            ],
            beneficiary: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            state_root: [
                43, 170, 44, 11, 24, 203, 102, 175, 148, 19, 206, 33, 12, 162, 140, 250, 139, 51,
                1, 225, 124, 48, 91, 116, 233, 18, 9, 33, 207, 28, 232, 228,
            ],
            difficulty: 0,
            number: 689967,
            gas_limit: 1000000000,
            timestamp: 1734609927,
            extra_data: vec![33, 181, 203, 134, 0, 0, 0, 0, 17, 239, 124, 88],
            prev_randao: [
                133, 248, 106, 212, 125, 177, 143, 132, 17, 200, 9, 228, 43, 184, 126, 6, 254, 129,
                182, 78, 78, 100, 78, 188, 159, 162, 187, 20, 188, 99, 177, 217,
            ],
            nonce: [0, 0, 0, 0, 0, 0, 0, 0],
            transactions: vec![Transaction {
                transaction_type: TransactionType::DynamicFee,
                chain_id: U256::from(146u64),
                nonce: 4,
                gas_price: U256::from(0u64),
                gas_limit: 46402,
                to: Some([
                    3, 158, 47, 182, 97, 2, 49, 76, 231, 182, 76, 229, 206, 62, 81, 131, 188, 148,
                    173, 56,
                ]),
                value: U256::from(0u64),
                data: vec![
                    9, 94, 167, 179, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 89, 28, 246, 148, 44, 66,
                    47, 165, 62, 141, 129, 198, 42, 150, 146, 215, 190, 167, 47, 97, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 113, 193, 20, 146, 185,
                    101, 80, 10, 115,
                ],
                access_list: vec![],
                max_fee_per_gas: U256::from(1100000000u64),
                max_priority_fee_per_gas: U256::from(1100000000u64),
                blob_versioned_hashes: vec![],
                max_fee_per_blob_gas: U256::from(0u64),
                authorization_list: vec![],
                y_parity: U256::from(0u64),
                r: U256::try_from_hex(
                    "0x2014dfffdac0d3e724e17c8ba423db2a629ca88f474eefa7eff7088a853ca06d",
                )
                .unwrap(),
                s: U256::try_from_hex(
                    "0x59be24e2ccb5eec18076c1df2a658a382af38674f105d44b55fd054e0a9ca6f6",
                )
                .unwrap(),
            }],
            receipts: vec![TransactionReceipt {
                transaction_type: TransactionType::DynamicFee,
                status: 1,
                cumulative_gas_used: 46402,
                logs: vec![Log {
                    address: [
                        3, 158, 47, 182, 97, 2, 49, 76, 231, 182, 76, 229, 206, 62, 81, 131, 188,
                        148, 173, 56,
                    ],
                    topics: vec![
                        [
                            140, 91, 225, 229, 235, 236, 125, 91, 209, 79, 113, 66, 125, 30, 132,
                            243, 221, 3, 20, 192, 247, 178, 41, 30, 91, 32, 10, 200, 199, 195, 185,
                            37,
                        ],
                        [
                            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 140, 120, 164, 80, 152, 27, 38,
                            124, 18, 230, 216, 114, 217, 86, 89, 204, 41, 34, 157, 235,
                        ],
                        [
                            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 89, 28, 246, 148, 44, 66, 47, 165,
                            62, 141, 129, 198, 42, 150, 146, 215, 190, 167, 47, 97,
                        ],
                    ],
                    data: vec![
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 113,
                        193, 20, 146, 185, 101, 80, 10, 115,
                    ],
                }],
            }],
            base_fee_per_gas: Some(U256::from(1000000000u64)),
            withdrawals_root: Some(
                Hash::try_from_hex(
                    "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
                )
                .unwrap(),
            ),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: None,
            requests_hash: None,
        };

        let hash = [
            119, 242, 95, 241, 99, 192, 109, 166, 253, 254, 237, 243, 64, 30, 175, 78, 84, 231, 41,
            42, 243, 19, 16, 197, 130, 156, 233, 166, 255, 232, 210, 90,
        ];

        assert_eq!(block.to_header().compute_hash(), hash)
    }
}
