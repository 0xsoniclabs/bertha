use alloy_rlp::{Decodable, Encodable, Header};
use serde::{Deserialize, Serialize};

use crate::{Address, AsHex, Hash};

/// A log entry which was emitted by a contract during a transaction.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(from = "JsonRpcLog", into = "JsonRpcLog")]
pub struct Log {
    pub address: Address,
    pub topics: Vec<Hash>,
    pub data: Vec<u8>,
}

impl Log {
    fn alloy_rlp_payload_length(&self) -> usize {
        Encodable::length(&self.address)
            + Encodable::length(&self.topics)
            + Encodable::length(&self.data.as_slice()) // custom
    }
}

impl Encodable for Log {
    fn length(&self) -> usize {
        let payload_length = self.alloy_rlp_payload_length();
        payload_length + alloy_rlp::length_of_length(payload_length)
    }

    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        Header {
            list: true,
            payload_length: self.alloy_rlp_payload_length(),
        }
        .encode(out);
        Encodable::encode(&self.address, out);
        Encodable::encode(&self.topics, out);
        Encodable::encode(&self.data.as_slice(), out); // custom
    }
}

impl Decodable for Log {
    fn decode(b: &mut &[u8]) -> alloy_rlp::Result<Self> {
        let Header {
            list,
            payload_length,
        } = Header::decode(b)?;
        if !list {
            return Err(alloy_rlp::Error::UnexpectedString);
        }
        let started_len = b.len();
        if started_len < payload_length {
            return Err(alloy_rlp::Error::InputTooShort);
        }
        let this = Self {
            address: Decodable::decode(b)?,
            topics: Decodable::decode(b)?,
            data: Header::decode_bytes(b, false)?.to_vec(), // custom
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
struct JsonRpcLog {
    pub address: AsHex<Address>,
    pub topics: Vec<AsHex<Hash>>,
    pub data: AsHex<Vec<u8>>,
    // Fields that are part of the JSON RPC response but we currently don't use:
    // pub block_number: AsHex<u64>,
    // pub transaction_hash: AsHex<Hash>,
    // pub transaction_index: AsHex<u64>,
    // pub block_hash: AsHex<Hash>,
    // pub log_index: AsHex<u64>,
}

impl From<JsonRpcLog> for Log {
    fn from(value: JsonRpcLog) -> Self {
        Self {
            address: value.address.0,
            topics: value.topics.into_iter().map(|h| h.0).collect(),
            data: value.data.0,
        }
    }
}

impl From<Log> for JsonRpcLog {
    fn from(value: Log) -> Self {
        Self {
            address: AsHex(value.address),
            topics: value.topics.into_iter().map(AsHex).collect(),
            data: AsHex(value.data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex_convert::HexConvert;

    #[test]
    fn can_be_serialized_to_json() {
        let address_hex = "0x1234567890abcdef1234567890abcdef12345678";
        let topic1_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";
        let topic2_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";
        let log = Log {
            address: Address::try_from_hex(address_hex).unwrap(),
            topics: vec![
                Hash::try_from_hex(topic1_hex).unwrap(),
                Hash::try_from_hex(topic2_hex).unwrap(),
            ],
            data: vec![1, 2, 3, 4, 5],
        };

        assert_eq!(
            serde_json::to_string(&log).unwrap(),
            format!(
                "{{\"address\":\"{address_hex}\",\"topics\":[\"{topic1_hex}\",\"{topic2_hex}\"],\"data\":\"0x0102030405\"}}"
            )
        );
    }

    #[test]
    fn can_be_deserialized_from_json() {
        let address_hex = "0x1234567890abcdef1234567890abcdef12345678";
        let topic1_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";
        let topic2_hex = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdef12345678ababaabababaab";

        let json_str = format!(
            "{{\"address\":\"{address_hex}\",\"topics\":[\"{topic1_hex}\",\"{topic2_hex}\"],\"data\":\"0x0102030405\"}}"
        );

        let log: Log = serde_json::from_str(&json_str).unwrap();
        assert_eq!(log.address.to_hex(), address_hex);
        assert_eq!(log.topics[0].to_hex(), topic1_hex);
        assert_eq!(log.topics[1].to_hex(), topic2_hex);
        assert_eq!(log.data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn can_be_encoded_to_rlp() {
        let log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![],
            data: Vec::try_from_hex("0x").unwrap(),
        };
        let expected_rlp =
            const_hex::decode("d7940000000000000000000000000000000000000000c080").unwrap();
        assert_eq!(alloy_rlp::encode(&log), expected_rlp);

        let log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![],
            data: Vec::try_from_hex("0x01").unwrap(),
        };
        let expected_rlp =
            const_hex::decode("d7940000000000000000000000000000000000000000c001").unwrap();
        assert_eq!(alloy_rlp::encode(&log), expected_rlp);

        let log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![
                Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
            ],
            data: Vec::try_from_hex("0x").unwrap(),
        };
        let expected_rlp = const_hex::decode("f838940000000000000000000000000000000000000000e1a0000000000000000000000000000000000000000000000000000000000000000080").unwrap();
        assert_eq!(alloy_rlp::encode(&log), expected_rlp);

        let log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![
                Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
            ],
            data: Vec::try_from_hex("0x01").unwrap(),
        };
        let expected_rlp = const_hex::decode("f838940000000000000000000000000000000000000000e1a0000000000000000000000000000000000000000000000000000000000000000001").unwrap();
        assert_eq!(alloy_rlp::encode(&log), expected_rlp);
    }

    #[test]
    fn can_be_decoded_from_rlp() {
        let expected_log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![],
            data: Vec::try_from_hex("0x").unwrap(),
        };
        let rlp = const_hex::decode("d7940000000000000000000000000000000000000000c080").unwrap();
        assert_eq!(alloy_rlp::decode_exact::<Log>(&rlp).unwrap(), expected_log);

        let expected_log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![],
            data: Vec::try_from_hex("0x01").unwrap(),
        };
        let rlp = const_hex::decode("d7940000000000000000000000000000000000000000c001").unwrap();
        assert_eq!(alloy_rlp::decode_exact::<Log>(&rlp).unwrap(), expected_log);

        let expected_log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![
                Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
            ],
            data: Vec::try_from_hex("0x").unwrap(),
        };
        let rlp = const_hex::decode("f838940000000000000000000000000000000000000000e1a0000000000000000000000000000000000000000000000000000000000000000080").unwrap();
        assert_eq!(alloy_rlp::decode_exact::<Log>(&rlp).unwrap(), expected_log);

        let expected_log = Log {
            address: Address::try_from_hex("0x0000000000000000000000000000000000000000").unwrap(),
            topics: vec![
                Hash::try_from_hex(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
            ],
            data: Vec::try_from_hex("0x01").unwrap(),
        };
        let rlp = const_hex::decode("f838940000000000000000000000000000000000000000e1a0000000000000000000000000000000000000000000000000000000000000000001").unwrap();
        assert_eq!(alloy_rlp::encode(&expected_log), rlp);
    }
}
