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

use alloy_rlp::Encodable;
use serde::{Deserialize, Serialize};

use crate::{Address, AsHex, Eip2718Marshallable};

/// An EIP-4895 beacon chain withdrawal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "JsonWithdrawal", into = "JsonWithdrawal")]
pub struct Withdrawal {
    pub index: u64,
    pub validator_index: u64,
    pub address: Address,
    pub amount: u64,
}

impl Withdrawal {
    fn alloy_rlp_payload_length(&self) -> usize {
        self.index.length()
            + self.validator_index.length()
            + Encodable::length(&self.address)
            + self.amount.length()
    }
}

impl Encodable for Withdrawal {
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
        self.index.encode(out);
        self.validator_index.encode(out);
        Encodable::encode(&self.address, out);
        self.amount.encode(out);
    }
}

impl Eip2718Marshallable for Withdrawal {
    fn marshal(&self) -> Vec<u8> {
        alloy_rlp::encode(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonWithdrawal {
    pub index: AsHex<u64>,
    pub validator_index: AsHex<u64>,
    pub address: AsHex<Address>,
    pub amount: AsHex<u64>,
}

impl From<Withdrawal> for JsonWithdrawal {
    fn from(w: Withdrawal) -> Self {
        JsonWithdrawal {
            index: AsHex(w.index),
            validator_index: AsHex(w.validator_index),
            address: AsHex(w.address),
            amount: AsHex(w.amount),
        }
    }
}

impl From<JsonWithdrawal> for Withdrawal {
    fn from(j: JsonWithdrawal) -> Self {
        Withdrawal {
            index: j.index.0,
            validator_index: j.validator_index.0,
            address: j.address.0,
            amount: j.amount.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_data::test_data_withdrawals::generate_withdrawals_with_data;

    #[test]
    fn can_be_serialized_to_json() {
        for data in generate_withdrawals_with_data() {
            let serialized = serde_json::to_value(&data.withdrawal).unwrap();
            let expected = serde_json::to_value(
                serde_json::from_str::<Withdrawal>(&data.json_representation).unwrap(),
            )
            .unwrap();
            assert_eq!(serialized, expected);
        }
    }

    #[test]
    fn can_be_deserialized_from_json() {
        for data in generate_withdrawals_with_data() {
            let deserialized: Withdrawal = serde_json::from_str(&data.json_representation).unwrap();
            assert_eq!(deserialized, data.withdrawal,);
        }
    }

    #[test]
    fn can_be_encoded_to_rlp() {
        for value in generate_withdrawals_with_data() {
            let encoded = alloy_rlp::encode(&value.withdrawal);
            assert_eq!(encoded, value.rlp_encoding,);
        }
    }
}
