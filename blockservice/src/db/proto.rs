// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

use bertha_types::U256;

tonic::include_proto!("block");

use transaction_receipt::PostStateOrStatus;

use crate::Error;

impl From<bertha_types::AccessListEntry> for AccessListEntry {
    fn from(value: bertha_types::AccessListEntry) -> Self {
        let storage_keys = value.storage_keys.into_iter().map(Into::into).collect();
        Self {
            address: value.address.into(),
            storage_keys,
        }
    }
}

impl From<bertha_types::SetCodeAuthorization> for SetCodeAuthorization {
    fn from(value: bertha_types::SetCodeAuthorization) -> Self {
        Self {
            chain_id: value.chain_id.to_be_bytes().to_vec(),
            address: value.address.into(),
            nonce: value.nonce,
            y_parity: value.y_parity as u64,
            r: value.r.to_be_bytes().to_vec(),
            s: value.s.to_be_bytes().to_vec(),
        }
    }
}

impl From<bertha_types::Transaction> for Transaction {
    fn from(value: bertha_types::Transaction) -> Self {
        let access_list = value.access_list.into_iter().map(From::from).collect();
        let blob_versioned_hashes = value
            .blob_versioned_hashes
            .into_iter()
            .map(Into::into)
            .collect();
        let authorization_list = value
            .authorization_list
            .into_iter()
            .map(Into::into)
            .collect();

        Self {
            transaction_type: (value.transaction_type as u8).into(),
            chain_id: value.chain_id.to_be_bytes().to_vec(),
            nonce: value.nonce,
            gas_price: value.gas_price.to_be_bytes().to_vec(),
            gas_limit: value.gas_limit,
            to: value.to.map(Into::into),
            value: value.value.to_be_bytes().to_vec(),
            data: value.data,
            access_list,
            max_fee_per_gas: value.max_fee_per_gas.to_be_bytes().to_vec(),
            max_priority_fee_per_gas: value.max_priority_fee_per_gas.to_be_bytes().to_vec(),
            blob_versioned_hashes,
            max_fee_per_blob_gas: value.max_fee_per_blob_gas.to_be_bytes().to_vec(),
            authorization_list,
            y_parity: value.y_parity.to_be_bytes().to_vec(),
            r: value.r.to_be_bytes().to_vec(),
            s: value.s.to_be_bytes().to_vec(),
        }
    }
}

impl From<bertha_types::Log> for Log {
    fn from(value: bertha_types::Log) -> Self {
        Self {
            address: value.address.into(),
            topics: value.topics.iter().map(Into::into).collect(),
            data: value.data.clone(),
        }
    }
}

impl From<bertha_types::TransactionReceipt> for TransactionReceipt {
    fn from(value: bertha_types::TransactionReceipt) -> Self {
        let logs = value.logs.into_iter().map(From::from).collect();
        Self {
            transaction_type: (value.transaction_type as u8).into(),
            cumulative_gas_used: value.cumulative_gas_used,
            logs,
            post_state_or_status: match value.post_state_or_status {
                bertha_types::PostStateOrStatus::PostState(post_state) => {
                    Some(PostStateOrStatus::PostState(post_state.to_vec()))
                }
                bertha_types::PostStateOrStatus::Status(status) => {
                    Some(PostStateOrStatus::Status(status))
                }
            },
        }
    }
}

impl From<bertha_types::Block> for Block {
    fn from(value: bertha_types::Block) -> Self {
        let transactions = value
            .transactions
            .into_iter()
            .map(From::from)
            .collect::<Vec<_>>();
        let receipts = value.receipts.into_iter().map(From::from).collect();

        Block {
            parent_hash: value.parent_hash.into(),
            ommers_hash: value.ommers_hash.into(),
            beneficiary: value.beneficiary.into(),
            state_root: value.state_root.into(),
            difficulty: value.difficulty,
            number: value.number,
            gas_limit: value.gas_limit,
            timestamp: value.timestamp,
            extra_data: value.extra_data,
            prev_randao: value.prev_randao.into(),
            nonce: value.nonce.into(),

            transactions,
            receipts,

            base_fee_per_gas: value.base_fee_per_gas.map(|v| v.to_be_bytes().to_vec()),
            withdrawals_root: value.withdrawals_root.map(Into::into),
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            parent_beacon_block_root: value.parent_beacon_block_root.map(Into::into),
            requests_hash: value.requests_hash.map(Into::into),

            verkle_state_root: value.verkle_state_root.map(Into::into),
            binary_state_root: value.binary_state_root.map(Into::into),
        }
    }
}

fn convert_to_fixed_size<const N: usize>(data: Vec<u8>) -> Result<[u8; N], Error> {
    data.try_into().map_err(|_| Error::TypeConversion)
}

fn convert_to_u256(data: Vec<u8>) -> Result<U256, Error> {
    Ok(U256::from_be_bytes(
        &data.try_into().map_err(|_| Error::TypeConversion)?,
    ))
}

impl TryFrom<AccessListEntry> for bertha_types::AccessListEntry {
    type Error = Error;

    fn try_from(value: AccessListEntry) -> Result<Self, Self::Error> {
        let storage_keys = value
            .storage_keys
            .into_iter()
            .map(convert_to_fixed_size)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            address: convert_to_fixed_size(value.address)?,
            storage_keys,
        })
    }
}

impl TryFrom<SetCodeAuthorization> for bertha_types::SetCodeAuthorization {
    type Error = Error;

    fn try_from(value: SetCodeAuthorization) -> Result<Self, Self::Error> {
        Ok(Self {
            chain_id: convert_to_u256(value.chain_id)?,
            address: convert_to_fixed_size(value.address)?,
            nonce: value.nonce,
            y_parity: value
                .y_parity
                .try_into()
                .map_err(|_| Error::TypeConversion)?,
            r: convert_to_u256(value.r)?,
            s: convert_to_u256(value.s)?,
        })
    }
}

impl TryFrom<Transaction> for bertha_types::Transaction {
    type Error = Error;

    fn try_from(value: Transaction) -> Result<Self, Self::Error> {
        let access_list = value
            .access_list
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;
        let blob_versioned_hashes = value
            .blob_versioned_hashes
            .into_iter()
            .map(convert_to_fixed_size)
            .collect::<Result<Vec<_>, _>>()?;
        let authorization_list = value
            .authorization_list
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            transaction_type: u8::try_from(value.transaction_type)
                .ok()
                .and_then(|v| bertha_types::TransactionType::try_from(v).ok())
                .ok_or(Error::TypeConversion)?,
            chain_id: convert_to_u256(value.chain_id)?,
            nonce: value.nonce,
            gas_price: convert_to_u256(value.gas_price)?,
            gas_limit: value.gas_limit,
            to: value.to.map(convert_to_fixed_size).transpose()?,
            value: convert_to_u256(value.value)?,
            data: value.data,
            access_list,
            max_fee_per_gas: convert_to_u256(value.max_fee_per_gas)?,
            max_priority_fee_per_gas: convert_to_u256(value.max_priority_fee_per_gas)?,
            blob_versioned_hashes,
            max_fee_per_blob_gas: convert_to_u256(value.max_fee_per_blob_gas)?,
            authorization_list,
            y_parity: convert_to_u256(value.y_parity)?,
            r: convert_to_u256(value.r)?,
            s: convert_to_u256(value.s)?,
        })
    }
}

impl TryFrom<Log> for bertha_types::Log {
    type Error = Error;

    fn try_from(value: Log) -> Result<Self, Self::Error> {
        Ok(Self {
            address: convert_to_fixed_size(value.address)?,
            topics: value
                .topics
                .into_iter()
                .map(convert_to_fixed_size)
                .collect::<Result<Vec<_>, _>>()?,
            data: value.data,
        })
    }
}

impl TryFrom<TransactionReceipt> for bertha_types::TransactionReceipt {
    type Error = Error;

    fn try_from(value: TransactionReceipt) -> Result<Self, Self::Error> {
        let logs: Vec<bertha_types::Log> = value
            .logs
            .into_iter()
            .map(TryFrom::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            transaction_type: u8::try_from(value.transaction_type)
                .ok()
                .and_then(|v| bertha_types::TransactionType::try_from(v).ok())
                .ok_or(Error::TypeConversion)?,
            post_state_or_status: match value.post_state_or_status {
                Some(PostStateOrStatus::Status(status)) => {
                    bertha_types::PostStateOrStatus::Status(status)
                }
                Some(PostStateOrStatus::PostState(post_state)) if post_state.len() == 32 => {
                    bertha_types::PostStateOrStatus::PostState(post_state.try_into().unwrap())
                }
                _ => return Err(Error::TypeConversion),
            },
            cumulative_gas_used: value.cumulative_gas_used,
            logs,
        })
    }
}

impl TryFrom<Block> for bertha_types::Block {
    type Error = Error;

    fn try_from(value: Block) -> Result<Self, Error> {
        let transactions = value
            .transactions
            .into_iter()
            .map(TryFrom::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let receipts = value
            .receipts
            .into_iter()
            .map(TryFrom::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(bertha_types::Block {
            parent_hash: convert_to_fixed_size(value.parent_hash)?,
            ommers_hash: convert_to_fixed_size(value.ommers_hash)?,
            beneficiary: convert_to_fixed_size(value.beneficiary)?,
            state_root: convert_to_fixed_size(value.state_root)?,
            difficulty: value.difficulty,
            number: value.number,
            gas_limit: value.gas_limit,
            timestamp: value.timestamp,
            extra_data: value.extra_data,
            prev_randao: convert_to_fixed_size(value.prev_randao)?,
            nonce: convert_to_fixed_size(value.nonce)?,

            transactions,
            receipts,

            base_fee_per_gas: value.base_fee_per_gas.map(convert_to_u256).transpose()?,
            withdrawals_root: value
                .withdrawals_root
                .map(convert_to_fixed_size)
                .transpose()?,
            blob_gas_used: value.blob_gas_used,
            excess_blob_gas: value.excess_blob_gas,
            parent_beacon_block_root: value
                .parent_beacon_block_root
                .map(convert_to_fixed_size)
                .transpose()?,
            requests_hash: value.requests_hash.map(convert_to_fixed_size).transpose()?,

            verkle_state_root: value
                .verkle_state_root
                .map(convert_to_fixed_size)
                .transpose()?,
            binary_state_root: value
                .binary_state_root
                .map(convert_to_fixed_size)
                .transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use bertha_types::{Address, Hash, TransactionType, U256};
    use rand::{Rng, SeedableRng, rngs::SmallRng};

    use super::*;

    struct TestRng {
        rng: SmallRng,
    }

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self {
                rng: SmallRng::seed_from_u64(seed),
            }
        }

        fn u64(&mut self) -> u64 {
            self.rng.random()
        }

        fn u256(&mut self) -> U256 {
            U256::from_be_bytes(&self.bytes::<32>())
        }

        fn bytes<const N: usize>(&mut self) -> [u8; N] {
            self.rng.random()
        }

        fn option<T>(&mut self, value: T) -> Option<T> {
            if self.rng.random_bool(0.5) {
                Some(value)
            } else {
                None
            }
        }
    }

    fn make_access_tuple(rng: &mut TestRng) -> bertha_types::AccessListEntry {
        bertha_types::AccessListEntry {
            address: rng.bytes(),
            storage_keys: vec![rng.bytes(), rng.bytes()],
        }
    }

    fn make_set_code_authorization(rng: &mut TestRng) -> bertha_types::SetCodeAuthorization {
        bertha_types::SetCodeAuthorization {
            chain_id: rng.u256(),
            address: rng.bytes(),
            nonce: rng.u64(),
            y_parity: rng.u64() as u8,
            r: rng.u256(),
            s: rng.u256(),
        }
    }

    fn make_transaction(
        rng: &mut TestRng,
        transaction_type: TransactionType,
    ) -> bertha_types::Transaction {
        let to: Address = rng.bytes();
        bertha_types::Transaction {
            transaction_type,
            chain_id: rng.u256(),
            nonce: rng.u64(),
            gas_price: rng.u256(),
            gas_limit: rng.u64(),
            to: rng.option(to),
            value: rng.u256(),
            data: rng.bytes::<128>().into(),
            // The fields we generate here are not consistent with the transaction type (it does not
            // matter).
            access_list: vec![make_access_tuple(rng), make_access_tuple(rng)],
            max_fee_per_gas: rng.u256(),
            max_priority_fee_per_gas: rng.u256(),
            blob_versioned_hashes: vec![rng.bytes(), rng.bytes(), rng.bytes()],
            max_fee_per_blob_gas: rng.u256(),
            authorization_list: vec![
                make_set_code_authorization(rng),
                make_set_code_authorization(rng),
            ],
            y_parity: rng.u256(),
            r: rng.u256(),
            s: rng.u256(),
        }
    }

    fn make_log(rng: &mut TestRng) -> bertha_types::Log {
        bertha_types::Log {
            address: rng.bytes(),
            topics: vec![rng.bytes(), rng.bytes()],
            data: rng.bytes::<10>().into(),
        }
    }

    fn make_receipt(
        rng: &mut TestRng,
        transaction_type: TransactionType,
    ) -> bertha_types::TransactionReceipt {
        let post_state_or_status = match rng.u64() % 3 {
            0 => bertha_types::PostStateOrStatus::Status(0),
            1 => bertha_types::PostStateOrStatus::Status(1),
            _ => bertha_types::PostStateOrStatus::PostState(rng.bytes()),
        };

        bertha_types::TransactionReceipt {
            transaction_type,
            post_state_or_status,
            cumulative_gas_used: rng.u64(),
            logs: vec![make_log(rng), make_log(rng)],
        }
    }

    fn make_block(rng: &mut TestRng) -> bertha_types::Block {
        let base_fee_per_gas = rng.u256();
        let withdrawals_root: Hash = rng.bytes();
        let blob_gas_used = rng.u64();
        let excess_blob_gas = rng.u64();
        let parent_beacon_block_root: Hash = rng.bytes();
        let requests_hash: Hash = rng.bytes();
        let verkle_state_root: Hash = rng.bytes();
        let binary_state_root: Hash = rng.bytes();

        bertha_types::Block {
            parent_hash: rng.bytes(),
            ommers_hash: rng.bytes(),
            beneficiary: rng.bytes(),
            state_root: rng.bytes(),
            difficulty: rng.u64(),
            number: rng.u64(),
            gas_limit: rng.u64(),
            timestamp: rng.u64(),
            extra_data: rng.bytes::<128>().into(),
            prev_randao: rng.bytes(),
            nonce: rng.bytes::<8>(),

            transactions: vec![
                make_transaction(rng, TransactionType::AccessList),
                make_transaction(rng, TransactionType::Legacy),
            ],
            receipts: vec![
                make_receipt(rng, TransactionType::AccessList),
                make_receipt(rng, TransactionType::Legacy),
            ],

            base_fee_per_gas: rng.option(base_fee_per_gas),
            withdrawals_root: rng.option(withdrawals_root),
            blob_gas_used: rng.option(blob_gas_used),
            excess_blob_gas: rng.option(excess_blob_gas),
            parent_beacon_block_root: rng.option(parent_beacon_block_root),
            requests_hash: rng.option(requests_hash),

            verkle_state_root: rng.option(verkle_state_root),
            binary_state_root: rng.option(binary_state_root),
        }
    }

    #[test]
    fn access_tuple_can_be_converted_from_and_to_protobuf_types() {
        let access_tuple = make_access_tuple(&mut TestRng::new(42));
        let proto_access_tuple: AccessListEntry = access_tuple.clone().into();
        let converted_access_tuple: bertha_types::AccessListEntry =
            proto_access_tuple.try_into().unwrap();
        assert_eq!(converted_access_tuple, access_tuple);
    }

    #[test]
    fn access_tuple_conversion_fails_for_invalid_byte_strings() {
        let access_tuple: AccessListEntry = make_access_tuple(&mut TestRng::new(123)).into();

        // Invalid address
        {
            let invalid_access_tuple = AccessListEntry {
                address: vec![0; 19],
                ..access_tuple.clone()
            };
            let err = bertha_types::AccessListEntry::try_from(invalid_access_tuple).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid storage key
        {
            let invalid_access_tuple = AccessListEntry {
                storage_keys: vec![vec![0; 31]],
                ..access_tuple
            };
            let err = bertha_types::AccessListEntry::try_from(invalid_access_tuple).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }
    }

    #[test]
    fn set_code_authorization_can_be_converted_from_and_to_protobuf_types() {
        let set_code_auth = make_set_code_authorization(&mut TestRng::new(42));
        let proto_set_code_auth: SetCodeAuthorization = set_code_auth.clone().into();
        let converted_set_code_auth: bertha_types::SetCodeAuthorization =
            proto_set_code_auth.try_into().unwrap();
        assert_eq!(converted_set_code_auth, set_code_auth);
    }

    #[test]
    fn set_code_authorization_conversion_fails_for_invalid_byte_strings() {
        let set_code_auth: SetCodeAuthorization =
            make_set_code_authorization(&mut TestRng::new(123)).into();

        // Invalid chain ID
        {
            let invalid_set_code_auth = SetCodeAuthorization {
                chain_id: vec![1; 33],
                ..set_code_auth.clone()
            };
            let err =
                bertha_types::SetCodeAuthorization::try_from(invalid_set_code_auth).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid address
        {
            let invalid_set_code_auth = SetCodeAuthorization {
                address: vec![0; 19],
                ..set_code_auth.clone()
            };
            let err =
                bertha_types::SetCodeAuthorization::try_from(invalid_set_code_auth).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid y parity
        {
            let invalid_set_code_auth = SetCodeAuthorization {
                y_parity: 256,
                ..set_code_auth.clone()
            };
            let err =
                bertha_types::SetCodeAuthorization::try_from(invalid_set_code_auth).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid r
        {
            let invalid_set_code_auth = SetCodeAuthorization {
                r: vec![1; 33],
                ..set_code_auth.clone()
            };
            let err =
                bertha_types::SetCodeAuthorization::try_from(invalid_set_code_auth).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid s
        {
            let invalid_set_code_auth = SetCodeAuthorization {
                s: vec![1; 33],
                ..set_code_auth
            };
            let err =
                bertha_types::SetCodeAuthorization::try_from(invalid_set_code_auth).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }
    }

    #[test]
    fn transaction_can_be_converted_from_and_to_protobuf_types() {
        for tx_type in 0..5 {
            let tx = make_transaction(
                &mut TestRng::new(42),
                TransactionType::try_from(tx_type as u8).unwrap(),
            );
            let proto_tx: Transaction = tx.clone().into();
            let converted_tx: bertha_types::Transaction = proto_tx.try_into().unwrap();
            assert_eq!(converted_tx, tx);
        }
    }

    #[test]
    fn transaction_conversion_fails_for_invalid_byte_strings() {
        let tx: Transaction =
            make_transaction(&mut TestRng::new(123), TransactionType::Legacy).into();

        // Invalid transaction type
        {
            let invalid_tx = Transaction {
                transaction_type: 256,
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid chain ID
        {
            let invalid_tx = Transaction {
                chain_id: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid gas price
        {
            let invalid_tx = Transaction {
                gas_price: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid address
        {
            let invalid_tx = Transaction {
                to: Some(vec![0; 19]),
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid value
        {
            let invalid_tx = Transaction {
                value: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid max fee per gas
        {
            let invalid_tx = Transaction {
                max_fee_per_gas: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid max priority fee per gas
        {
            let invalid_tx = Transaction {
                max_priority_fee_per_gas: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid blob hash
        {
            let invalid_tx = Transaction {
                blob_versioned_hashes: vec![vec![0; 31]],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid max fee per blob gas
        {
            let invalid_tx = Transaction {
                max_fee_per_blob_gas: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid y parity
        {
            let invalid_tx = Transaction {
                y_parity: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid r
        {
            let invalid_tx = Transaction {
                r: vec![1; 33],
                ..tx.clone()
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid s
        {
            let invalid_tx = Transaction {
                s: vec![1; 33],
                ..tx
            };
            let err = bertha_types::Transaction::try_from(invalid_tx).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }
    }

    #[test]
    fn log_can_be_converted_from_and_to_protobuf_types() {
        let log = make_log(&mut TestRng::new(42));
        let proto_log: Log = log.clone().into();
        let converted_log: bertha_types::Log = proto_log.try_into().unwrap();
        assert_eq!(converted_log, log);
    }

    #[test]
    fn log_conversion_fails_for_invalid_byte_strings() {
        let log: Log = make_log(&mut TestRng::new(123)).into();

        // Invalid address
        {
            let invalid_log = Log {
                address: vec![0; 19],
                ..log.clone()
            };
            let err = bertha_types::Log::try_from(invalid_log).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid topic
        {
            let invalid_log = Log {
                topics: vec![vec![0; 31]],
                ..log
            };
            let err = bertha_types::Log::try_from(invalid_log).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }
    }

    #[test]
    fn receipt_can_be_converted_from_and_to_protobuf_types() {
        let receipt = make_receipt(&mut TestRng::new(42), TransactionType::Legacy);
        let proto_receipt: TransactionReceipt = receipt.clone().into();
        let converted_receipt: bertha_types::TransactionReceipt = proto_receipt.try_into().unwrap();
        assert_eq!(converted_receipt, receipt);
    }

    #[test]
    fn receipt_conversion_fails_for_invalid_transaction_types() {
        let receipt: TransactionReceipt =
            make_receipt(&mut TestRng::new(123), TransactionType::Legacy).into();

        let invalid_receipt = TransactionReceipt {
            transaction_type: 256,
            ..receipt.clone()
        };
        let err = bertha_types::TransactionReceipt::try_from(invalid_receipt).unwrap_err();
        assert_eq!(err, Error::TypeConversion);
    }

    #[test]
    fn block_can_be_converted_from_and_to_protobuf_types() {
        let block = make_block(&mut TestRng::new(42));
        let proto_block: Block = block.clone().into();
        let converted_block: bertha_types::Block = proto_block.try_into().unwrap();
        assert_eq!(converted_block, block);
    }

    #[test]
    fn block_conversion_fails_for_invalid_byte_strings() {
        let block: Block = make_block(&mut TestRng::new(123)).into();

        // Invalid parent hash
        {
            let invalid_block = Block {
                parent_hash: vec![0; 31],
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid ommers hash
        {
            let invalid_block = Block {
                ommers_hash: vec![0; 31],
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid beneficiary
        {
            let invalid_block = Block {
                beneficiary: vec![0; 19],
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid state root
        {
            let invalid_block = Block {
                state_root: vec![0; 31],
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid prev randao
        {
            let invalid_block = Block {
                prev_randao: vec![0; 31],
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid nonce
        {
            let invalid_block = Block {
                nonce: vec![0; 7],
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid base fee per gas
        {
            let invalid_block = Block {
                base_fee_per_gas: Some(vec![1; 33]),
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid withdrawals root
        {
            let invalid_block = Block {
                withdrawals_root: Some(vec![0; 31]),
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid parent beacon block root
        {
            let invalid_block = Block {
                parent_beacon_block_root: Some(vec![0; 31]),
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }

        // Invalid requests hash
        {
            let invalid_block = Block {
                requests_hash: Some(vec![0; 31]),
                ..block.clone()
            };
            let err = bertha_types::Block::try_from(invalid_block).unwrap_err();
            assert_eq!(err, Error::TypeConversion);
        }
    }
}
