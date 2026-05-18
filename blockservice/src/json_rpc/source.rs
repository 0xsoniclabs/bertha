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

use bertha_types::{BlockHeader, HexConvert, Transaction, TransactionReceipt};
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use serde::{Deserialize, Serialize};

use crate::json_rpc::Error;

/// An abstraction which provides means to request blockchain data.
#[cfg_attr(test, mockall::automock)]
pub trait Source: Send + Sync {
    /// Returns the block header and transactions for the specified block number.
    fn get_block_header_with_transactions(
        &self,
        block_number: u64,
    ) -> impl Future<Output = Result<BlockHeaderWithTransactions, Error>> + Send;

    /// Returns the receipts for the block with the specified block number.
    fn get_block_receipts(
        &self,
        block_number: u64,
    ) -> impl Future<Output = Result<Vec<TransactionReceipt>, Error>> + Send;
}

/// A source which requests data from a remote server using JSON RPC.
/// Using this source directly is not recommended, as it does not verify the
/// correctness of the returned data, thereby implicitly trusting the RPC server.
#[derive(Debug)]
pub struct NetworkSource {
    http_client: HttpClient,
}

impl NetworkSource {
    pub fn try_new(server: impl AsRef<str>) -> Result<Self, Error> {
        Ok(Self {
            http_client: HttpClientBuilder::new().build(server)?,
        })
    }
}

impl Source for NetworkSource {
    async fn get_block_header_with_transactions(
        &self,
        block_number: u64,
    ) -> Result<BlockHeaderWithTransactions, Error> {
        // see: https://docs.chainstack.com/reference/fantom-getblockbynumber
        // see: https://docs.chainstack.com/reference/fantom-getblockbyhash
        let result: Option<_> = self
            .http_client
            .request(
                "eth_getBlockByNumber",
                rpc_params![
                    block_number.to_hex(),
                    true // include details of transactions
                ],
            )
            .await?;
        result.ok_or(Error::NotFound)
    }

    async fn get_block_receipts(
        &self,
        block_number: u64,
    ) -> Result<Vec<TransactionReceipt>, Error> {
        // see: https://docs.chainstack.com/reference/ethereum-getblockreceipts
        let receipts: Option<Vec<TransactionReceipt>> = self
            .http_client
            .request("eth_getBlockReceipts", rpc_params![block_number.to_hex()])
            .await?;
        receipts.ok_or(Error::NotFound)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockHeaderWithTransactions {
    #[serde(flatten)]
    pub block_header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

#[cfg(test)]
mod tests {
    use bertha_types::TransactionType;
    use serde_json::json;
    use wiremock::MockServer;

    use super::*;
    use crate::json_rpc::test_utils::build_mock_server_request_handler_for_single_request;

    #[test]
    fn try_new_with_invalid_url_returns_error() {
        let network = NetworkSource::try_new("invalid_url");
        assert!(network.is_err());
    }

    #[tokio::test]
    async fn get_block_header_with_transactions_requests_and_deserializes_block_header() {
        let mock_server = MockServer::start().await;
        let network_source = NetworkSource::try_new(mock_server.uri()).unwrap();

        let block_header_with_transactions = BlockHeaderWithTransactions {
            block_header: BlockHeader::default(),
            transactions: Vec::new(),
        };

        let block_number = 123456;
        mock_server
            .register(build_mock_server_request_handler_for_single_request(
                "eth_getBlockByNumber",
                0,
                vec![json!(block_number.to_hex()), json!(true)],
                block_header_with_transactions.clone(),
            ))
            .await;

        let received_block = network_source
            .get_block_header_with_transactions(block_number)
            .await;
        assert!(received_block.is_ok());
        let received_block = received_block.unwrap();
        assert_eq!(received_block, block_header_with_transactions);
    }

    #[tokio::test]
    async fn get_block_receipts_requests_and_deserializes_receipts() {
        let mock_server = MockServer::start().await;

        let network_source = NetworkSource::try_new(mock_server.uri()).unwrap();

        let block_receipts = vec![TransactionReceipt {
            cumulative_gas_used: u64::default(),
            logs: Vec::default(),
            post_state_or_status: bertha_types::PostStateOrStatus::default(),
            transaction_type: TransactionType::Legacy,
        }];

        let block_number = 123456;
        mock_server
            .register(build_mock_server_request_handler_for_single_request(
                "eth_getBlockReceipts",
                0,
                vec![json!(block_number.to_hex())],
                block_receipts.clone(),
            ))
            .await;

        let received_block_receipts = network_source.get_block_receipts(block_number).await;
        assert!(received_block_receipts.is_ok());
        assert_eq!(received_block_receipts.unwrap(), block_receipts);
    }

    #[test]
    fn block_header_with_transactions_serializes_and_deserializes_correctly() {
        let block_header = BlockHeader::default();
        let transactions = vec![Transaction::default()];
        let block_header_with_transactions = BlockHeaderWithTransactions {
            block_header,
            transactions,
        };

        // serialize and deserialize = identity
        {
            let serialized = serde_json::to_string_pretty(&block_header_with_transactions).unwrap();
            let deserialized: BlockHeaderWithTransactions =
                serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, block_header_with_transactions);
        }

        // header fields are flattened
        {
            let serialized = serde_json::to_value(&block_header_with_transactions).unwrap();
            assert!(serialized.is_object());
            let serialized = serialized.as_object().unwrap();
            // test that the header fields are flattened
            assert!(serialized.get("number").is_some());
            // check that transactions are included
            assert!(serialized.get("transactions").is_some());
        }
    }
}
