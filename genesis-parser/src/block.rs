use alloy_rlp::{Decodable, Encodable, Header, RlpDecodable, RlpEncodable};
use bertha_types::{
    Block, EIP2718Unmarshallable, EMPTY_OMMERS_HASH, EMPTY_TREE_ROOT_HASH, Eip2718Marshallable,
    Hash, HexConvert, RlpString, Transaction, TransactionType, U256,
};

use crate::transaction_receipt::{StoredReceiptRlp, StoredReceiptRlpWithTxType};

/// A wrapper around a [Transaction] to implement RLP encoding/decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RlpTransaction(Transaction);

impl Encodable for RlpTransaction {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        if self.0.transaction_type == TransactionType::Legacy {
            out.put_slice(&self.0.marshal());
        } else {
            RlpString(self.0.marshal()).encode(out);
        }
    }
}

impl Decodable for RlpTransaction {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let mut peek = *buf;
        let header = Header::decode(&mut peek)?;
        if header.list {
            Transaction::unmarshal(buf).map(RlpTransaction)
        } else {
            *buf = &buf[header.length()..];
            Transaction::unmarshal(buf).map(RlpTransaction)
        }
    }
}

/// A [FullBlock] with a block number.
/// This type and its fields correspond directly to the ones used in Sonic.
// Source: sonic/inter/ibr/inter_block_records.go
#[derive(Debug, Clone, Default, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub(crate) struct IdxFullBlock {
    pub block: FullBlock,
    pub block_number: u64, // idx
}

/// A block in the format used to store it in genesis files.
/// This type and its fields correspond directly to the ones used in Sonic.
// Source: sonic/inter/ibr/inter_block_records.go
#[derive(Debug, Clone, Default, PartialEq, Eq, RlpEncodable, RlpDecodable)]
pub(crate) struct FullBlock {
    pub block_hash: Hash,
    pub parent_hash: Hash,
    pub state_root: Hash,
    pub timestamp: u64, // in nanoseconds
    pub duration: u64,
    pub difficulty: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub base_fee: U256,
    pub prev_randao: Hash,
    pub epoch: u32,
    pub transactions: Vec<RlpTransaction>,
    pub receipts: Vec<StoredReceiptRlp>,
}

impl TryFrom<IdxFullBlock> for Block {
    type Error = &'static str;

    fn try_from(idx_full_block: IdxFullBlock) -> Result<Self, Self::Error> {
        let mut extra_data = Vec::new();
        let timestamp_nanos = idx_full_block.block.timestamp.rem_euclid(10u64.pow(9)) as u32;
        extra_data.extend_from_slice(&timestamp_nanos.to_be_bytes());
        extra_data.extend_from_slice(&idx_full_block.block.duration.to_be_bytes());

        // timestamp is in nanoseconds
        let timestamp_secs = idx_full_block.block.timestamp.div_euclid(10u64.pow(9));

        let transactions = idx_full_block
            .block
            .transactions
            .into_iter()
            .map(|tx| tx.0)
            .collect::<Vec<_>>();

        let receipts = idx_full_block
            .block
            .receipts
            .into_iter()
            .zip(&transactions)
            .map(|(receipt, tx)| StoredReceiptRlpWithTxType {
                receipt,
                transaction_type: tx.transaction_type,
            })
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            parent_hash: idx_full_block.block.parent_hash,
            ommers_hash: Hash::try_from_hex(EMPTY_OMMERS_HASH).unwrap(),
            beneficiary: Default::default(),
            state_root: idx_full_block.block.state_root,
            difficulty: idx_full_block.block.difficulty,
            number: idx_full_block.block_number,
            gas_limit: idx_full_block.block.gas_limit,
            timestamp: timestamp_secs,
            extra_data,
            prev_randao: idx_full_block.block.prev_randao,
            nonce: [0; 8],
            transactions,
            receipts,
            base_fee_per_gas: Some(idx_full_block.block.base_fee),
            withdrawals_root: Some(Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap()),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: None,
            requests_hash: None,
        })
    }
}

impl TryFrom<Block> for IdxFullBlock {
    type Error = &'static str;

    fn try_from(value: Block) -> Result<Self, Self::Error> {
        let block_hash = value.to_header().compute_hash();

        let gas_used = value
            .receipts
            .last()
            .map(|tx| tx.cumulative_gas_used)
            .unwrap_or_default();

        let timestamp_secs = value.timestamp;
        let timestamp_nanos = u32::from_be_bytes(
            value
                .extra_data
                .get(0..4)
                .ok_or("extra_data should be at least 12 bytes long")?
                .try_into()
                .map_err(|_| "extra_data should be at least 12 bytes long")?,
        ) as u64;
        let timestamp = timestamp_secs * 10u64.pow(9) + timestamp_nanos;

        let duration = u64::from_be_bytes(
            value
                .extra_data
                .get(4..12)
                .ok_or("extra_data should be at least 12 bytes long")?
                .try_into()
                .map_err(|_| "extra_data should be at least 12 bytes long")?,
        );

        let transactions = value.transactions.into_iter().map(RlpTransaction).collect();
        let receipts = value.receipts.into_iter().map(From::from).collect();

        Ok(IdxFullBlock {
            block: FullBlock {
                block_hash,
                parent_hash: value.parent_hash,
                state_root: value.state_root,
                timestamp,
                duration,
                difficulty: value.difficulty,
                gas_limit: value.gas_limit,
                gas_used,
                base_fee: value.base_fee_per_gas.unwrap_or_default(),
                prev_randao: value.prev_randao,
                epoch: 0, // Epoch is not used in this context
                transactions,
                receipts,
            },
            block_number: value.number,
        })
    }
}

#[cfg(test)]
mod tests {
    use bertha_types::{
        AccessListEntry, Address, RlpString, SetCodeAuthorization, TransactionReceipt,
        TransactionType,
    };

    use super::*;

    #[test]
    fn block_try_from_idx_full_block_converts_timestamp_and_duration_and_extra_data() {
        let idx_full_block = IdxFullBlock {
            block: FullBlock {
                block_hash: Hash::from([0; 32]),
                parent_hash: Hash::from([1; 32]),
                state_root: Hash::from([2; 32]),
                timestamp: 1234567890123,
                duration: 1000,
                difficulty: 42,
                gas_limit: 8000000,
                gas_used: 5000000,
                base_fee: U256::from(100u8),
                prev_randao: Hash::from([3; 32]),
                epoch: 0,
                transactions: vec![],
                receipts: vec![],
            },
            block_number: 0,
        };

        let block = Block {
            parent_hash: Hash::from([1; 32]),
            ommers_hash: Hash::try_from_hex(EMPTY_OMMERS_HASH).unwrap(),
            beneficiary: Default::default(),
            state_root: Hash::from([2; 32]),
            difficulty: 42,
            number: 0,
            gas_limit: 8000000,
            timestamp: 1234, // Converted to seconds
            // Timestamp in nanoseconds (4 bytes in big endian) + duration (8 bytes in big endian)
            extra_data: vec![
                0x21, 0xd9, 0x50, 0xcb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xe8,
            ],
            prev_randao: Hash::from([3; 32]),
            nonce: [0; 8],
            transactions: vec![],
            receipts: vec![],
            base_fee_per_gas: Some(U256::from(100u8)),
            withdrawals_root: Some(Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap()),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: None,
            requests_hash: None,
        };

        assert_eq!(Block::try_from(idx_full_block).unwrap(), block);
    }

    #[test]
    fn block_try_from_idx_full_block_returns_error_when_status_is_invalid() {
        let idx_full_block = IdxFullBlock {
            block: FullBlock {
                transactions: vec![RlpTransaction(Transaction::default())],
                receipts: vec![StoredReceiptRlp {
                    post_state_or_status: RlpString(vec![1, 2, 3]), // Invalid status
                    ..StoredReceiptRlp::default()
                }],
                ..FullBlock::default()
            },
            block_number: 0,
        };

        assert_eq!(
            Block::try_from(idx_full_block),
            Err("invalid receipt status")
        );
    }

    #[test]
    fn idx_full_block_try_from_block_converts_extra_data_to_timestamp_and_duration_and_computes_gas_used_and_block_hash()
     {
        let block = Block {
            parent_hash: Hash::from([1; 32]),
            ommers_hash: Hash::try_from_hex(EMPTY_OMMERS_HASH).unwrap(),
            beneficiary: Default::default(),
            state_root: Hash::from([2; 32]),
            difficulty: 42,
            number: 0,
            gas_limit: 8000000,
            timestamp: 1234,
            extra_data: vec![
                0x21, 0xd9, 0x50, 0xcb, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xe8,
            ],
            prev_randao: Hash::from([3; 32]),
            nonce: [0; 8],
            transactions: vec![Transaction::default()],
            receipts: vec![TransactionReceipt {
                cumulative_gas_used: 5000000,
                ..TransactionReceipt::default()
            }],
            base_fee_per_gas: Some(U256::from(100u8)),
            withdrawals_root: Some(Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap()),
            blob_gas_used: Some(0),
            excess_blob_gas: Some(0),
            parent_beacon_block_root: None,
            requests_hash: None,
        };

        let idx_full_block = IdxFullBlock {
            block: FullBlock {
                block_hash: block.to_header().compute_hash(),
                parent_hash: Hash::from([1; 32]),
                state_root: Hash::from([2; 32]),
                timestamp: 1234567890123, // seconds from timestamp + nanoseconds from extra_data
                duration: 1000,
                difficulty: 42,
                gas_limit: 8000000,
                gas_used: 5000000,
                base_fee: U256::from(100u8),
                prev_randao: Hash::from([3; 32]),
                epoch: 0,
                transactions: vec![RlpTransaction(Transaction::default())],
                receipts: vec![StoredReceiptRlp {
                    cumulative_gas_used: 5000000,
                    ..StoredReceiptRlp::default()
                }],
            },
            block_number: 0,
        };

        assert_eq!(IdxFullBlock::try_from(block).unwrap(), idx_full_block);
    }

    #[test]
    fn idx_full_block_try_from_block_returns_error_if_extra_data_to_short() {
        let block = Block {
            extra_data: vec![0; 3],
            ..Block::default_sonic()
        };
        assert_eq!(
            IdxFullBlock::try_from(block),
            Err("extra_data should be at least 12 bytes long")
        );
    }

    #[test]
    fn block_try_from_idx_full_block_try_from_block_is_identity() {
        let block = Block {
            extra_data: vec![1; 12],
            transactions: vec![Transaction::default()],
            ..Block::default_sonic()
        };
        let converted = Block::try_from(IdxFullBlock::try_from(block.clone()).unwrap()).unwrap();
        assert_eq!(converted, block);
    }

    /// Generates a set of transactions with their RLP encodings.
    fn generate_transactions_with_rlp() -> impl IntoIterator<Item = (Transaction, Vec<u8>)> {
        // tested cases
        // - all 5 transaction types
        // - to field:
        //   - set to None
        //   - set to Some
        // - data field:
        //   - empty
        //   - non-empty
        // - access_list field:
        //   - empty
        //   - non-empty
        // - blob_versioned_hashes field:
        //   - empty
        //   - non-empty
        // - authorization_list field:
        //   - empty
        //   - non-empty

        [
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x0").unwrap(),
                    chain_id: U256::try_from_hex("0x0").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::default(),
                    max_fee_per_gas: U256::default(),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    y_parity: U256::try_from_hex("0x25").unwrap(),
                    r: U256::try_from_hex(
                        "0x81f84dfa55a3b2e8abd5f03605e386c20a71050103dd518c4bf27c4b9308d0b4",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x4339e8f47dd680a7f5d49ace126c6bbbad5ee7a6ba1dcb3114b2294565c8b134",
                    )
                    .unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    blob_versioned_hashes: Vec::new(),
                    authorization_list: Vec::new(),
                },
                const_hex::decode(
                    "0xf84980808080808025a081f84dfa55a3b2e8abd5f03605e386c20a71050103dd518c4bf27c4b9308d0b4a04339e8f47dd680a7f5d49ace126c6bbbad5ee7a6ba1dcb3114b2294565c8b134",
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x0").unwrap(),
                    chain_id: U256::try_from_hex("0x0").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::default(),
                    max_fee_per_gas: U256::default(),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x01").unwrap(),
                    y_parity: U256::try_from_hex("0x26").unwrap(),
                    r: U256::try_from_hex(
                        "0x8ce4b169534418abbe9410e8fcdff4cd47e10265588b55dfa96c70f6fd62c6bf",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x23869888069d3b974aba703cf31a06eeed8466e9981697f54ec0709cd65b2ca4",
                    )
                    .unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    blob_versioned_hashes: Vec::new(),
                    authorization_list: Vec::new(),
                },
                const_hex::decode("f84980808080800126a08ce4b169534418abbe9410e8fcdff4cd47e10265588b55dfa96c70f6fd62c6bfa023869888069d3b974aba703cf31a06eeed8466e9981697f54ec0709cd65b2ca4").unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x1").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::try_from_hex("0x0").unwrap(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::default(),
                    max_priority_fee_per_gas: U256::default(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0x3ef69057fef8e5910debc8e52189c0eb57184cbe58415c58ac67d78b7c6d29ce",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x7673ba8e62f61bd1cd7f026d14c96d014e070b915a185948647fba29659ca352",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb84e01f84b01808080808080c001a03ef69057fef8e5910debc8e52189c0eb57184cbe58415c58ac67d78b7c6d29cea07673ba8e62f61bd1cd7f026d14c96d014e070b915a185948647fba29659ca352",
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x2").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: None,
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0xc4dd060b048fc2b257e2a1e00ea3741884ca32b40e2ada3b70eec4f69bea1947",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x41949023f06ea394e9c2bfb5c02bf67ede6ec813c9e71a6936900aa676dd1050",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb84f02f84c0180808080808080c001a0c4dd060b048fc2b257e2a1e00ea3741884ca32b40e2ada3b70eec4f69bea1947a041949023f06ea394e9c2bfb5c02bf67ede6ec813c9e71a6936900aa676dd1050"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x3").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::try_from_hex("0x0").unwrap(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x0").unwrap(),
                    r: U256::try_from_hex(
                        "0x265974ddd1be7ef0cacd823784e994a8029a776becf760eea18a7a356f2e206c",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x351c528fba4ca69eaa850fc30f8e16bed9c083f2bb77ab89ef1c301de582c1e2",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb86503f86201808080809400000000000000000000000000000000000000008080c080c080a0265974ddd1be7ef0cacd823784e994a8029a776becf760eea18a7a356f2e206ca0351c528fba4ca69eaa850fc30f8e16bed9c083f2bb77ab89ef1c301de582c1e2"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x3").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_fee_per_blob_gas: U256::try_from_hex("0x0").unwrap(),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    blob_versioned_hashes: vec![
                        Hash::try_from_hex(
                            "0x0000000000000000000000000000000000000000000000000000000000000000",
                        )
                        .unwrap(),
                    ],
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x0").unwrap(),
                    r: U256::try_from_hex(
                        "0xea9aff2ec0c4b370ae14a055ffa0d7e5e3a00e039be41412548078c96a35cca5",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x36f768887cf167a25be29f6f127836517ea067193d4c07e7f147767d43d91d57",
                    )
                    .unwrap(),
                },
                const_hex::decode("b88603f88301808080809400000000000000000000000000000000000000008080c080e1a0000000000000000000000000000000000000000000000000000000000000000080a0ea9aff2ec0c4b370ae14a055ffa0d7e5e3a00e039be41412548078c96a35cca5a036f768887cf167a25be29f6f127836517ea067193d4c07e7f147767d43d91d57").unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x3").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: vec![AccessListEntry {
                        address: Address::try_from_hex("0x0000000000000000000000000000000000000000")
                            .unwrap(),
                        storage_keys: Vec::new(),
                    }],
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::try_from_hex("0x0").unwrap(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0xc11bf40b64864762c1a38f045ab45a19eefe17b21c2d508c11221b4e54889613",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x37e4802da391446247aaa73613f07fb7291864314d6788d20cf17a5b407e0330",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb87c03f87901808080809400000000000000000000000000000000000000008080d7d6940000000000000000000000000000000000000000c080c001a0c11bf40b64864762c1a38f045ab45a19eefe17b21c2d508c11221b4e54889613a037e4802da391446247aaa73613f07fb7291864314d6788d20cf17a5b407e0330"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x4").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: Vec::new(),
                    y_parity: U256::try_from_hex("0x1").unwrap(),
                    r: U256::try_from_hex(
                        "0x81dcbcae18a4ca0e228c63d02a699c65653fe898581c1fe4f9b4a519e038b969",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x5fd2c190dd5001139230f62c133aa77407c77c86752b6408cbcff6a09a43401d",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb86404f86101808080809400000000000000000000000000000000000000008080c0c001a081dcbcae18a4ca0e228c63d02a699c65653fe898581c1fe4f9b4a519e038b969a05fd2c190dd5001139230f62c133aa77407c77c86752b6408cbcff6a09a43401d"
                ).unwrap()
            ),
            (
                Transaction {
                    transaction_type: TransactionType::try_from_hex("0x4").unwrap(),
                    chain_id: U256::try_from_hex("0x1").unwrap(),
                    nonce: u64::try_from_hex("0x0").unwrap(),
                    gas_price: U256::default(),
                    gas_limit: u64::try_from_hex("0x0").unwrap(),
                    to: Some(
                        Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
                    ),
                    value: U256::try_from_hex("0x0").unwrap(),
                    data: Vec::try_from_hex("0x").unwrap(),
                    access_list: Vec::new(),
                    max_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    max_priority_fee_per_gas: U256::try_from_hex("0x0").unwrap(),
                    blob_versioned_hashes: Vec::new(),
                    max_fee_per_blob_gas: U256::default(),
                    authorization_list: vec![SetCodeAuthorization {
                        chain_id: U256::try_from_hex("0x0").unwrap(),
                        address: Address::try_from_hex("0x0000000000000000000000000000000000000000")
                            .unwrap(),
                        nonce: u64::try_from_hex("0x0").unwrap(),
                        y_parity: u8::try_from_hex("0x0").unwrap(),
                        r: U256::try_from_hex("0x0").unwrap(),
                        s: U256::try_from_hex("0x0").unwrap(),
                    }],
                    y_parity: U256::try_from_hex("0x0").unwrap(),
                    r: U256::try_from_hex(
                        "0x9ae41ab490c59fd1e1aae4df6a7d96931ae25e9f2120106dff8f8e6d079f6366",
                    )
                    .unwrap(),
                    s: U256::try_from_hex(
                        "0x2d5bc1c108b11cda410cd7b42a342341c51012e4636a15c8c5ebb2fc5bed2962",
                    )
                    .unwrap(),
                },
                const_hex::decode(
                    "0xb87f04f87c01808080809400000000000000000000000000000000000000008080c0dbda809400000000000000000000000000000000000000008080808080a09ae41ab490c59fd1e1aae4df6a7d96931ae25e9f2120106dff8f8e6d079f6366a02d5bc1c108b11cda410cd7b42a342341c51012e4636a15c8c5ebb2fc5bed2962"
                ).unwrap()
            )
        ]
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
            chain_id: U256::default(),
            nonce: Default::default(),
            gas_price: U256::default(),
            gas_limit: Default::default(),
            value: U256::default(),
            data: Vec::default(),
            max_fee_per_gas: U256::default(),
            max_priority_fee_per_gas: U256::default(),
            max_fee_per_blob_gas: U256::default(),
            y_parity: U256::default(),
            r: U256::default(),
            s: U256::default(),
        }
    }

    #[test]
    fn can_be_encoded_to_rlp() {
        for (tx, rlp) in generate_transactions_with_rlp() {
            let mut buf = Vec::new();
            RlpTransaction(tx).encode(&mut buf);
            assert_eq!(buf, rlp, "Encoded RLP should match expected value");
        }
    }

    #[test]
    fn encodes_invalid_transaction_to_rlp_of_rlp_of_empty_string() {
        for invalid_tx in [
            make_transaction(TransactionType::SetCode, false),
            make_transaction(TransactionType::Blob, false),
        ] {
            let mut buf = Vec::new();
            RlpTransaction(invalid_tx).encode(&mut buf);

            let mut rlp_empty_str = Vec::new();
            RlpString(vec![]).encode(&mut rlp_empty_str);
            let mut rlp_rlp_empty_str = Vec::new();
            RlpString(rlp_empty_str).encode(&mut rlp_rlp_empty_str);

            assert_eq!(buf, rlp_rlp_empty_str);
        }
    }

    #[test]
    fn can_be_decoded_from_rlp() {
        for (tx, rlp) in generate_transactions_with_rlp() {
            let decoded = RlpTransaction::decode(&mut &rlp[..]).unwrap();
            assert_eq!(
                decoded.0, tx,
                "Decoded Transaction should match expected one"
            );
        }
    }

    #[test]
    fn fails_to_decode_when_transaction_type_is_invalid() {
        let tx = make_transaction(TransactionType::AccessList, false);
        let mut buf = Vec::new();
        RlpTransaction(tx).encode(&mut buf);
        let mut rlp = buf.as_slice();
        Header::decode(&mut rlp).unwrap();
        let header_len = buf.len() - rlp.len();
        // next next byte is used for the transaction type
        buf[header_len] = 0x05; // Set an invalid transaction type
        let decoded = RlpTransaction::decode(&mut &buf[..]);
        assert_eq!(
            decoded,
            Err(alloy_rlp::Error::Custom("invalid transaction type"))
        );
    }
}
