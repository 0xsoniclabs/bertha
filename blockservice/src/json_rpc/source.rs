use bertha_types::{BlockHeader, HexConvert, Transaction, TransactionReceipt};
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
    rpc_params,
};
use serde::{Deserialize, Serialize};

use crate::json_rpc::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlockHeaderWithTransactions {
    #[serde(flatten)]
    block_header: BlockHeader,
    transactions: Vec<Transaction>,
}

/// An abstraction which provides means to request blockchain data.
#[cfg_attr(test, mockall::automock)]
pub trait Source: Send + Sync {
    /// Returns the block with the specified block identifier.
    fn get_block_header_with_transactions(
        &self,
        block_number: u64,
    ) -> impl std::future::Future<Output = Result<(BlockHeader, Vec<Transaction>), Error>>
    + std::marker::Send;

    /// Returns the receipt for the block with the specified block identifier.
    fn get_block_receipt(
        &self,
        block_number: u64,
    ) -> impl std::future::Future<Output = Result<Vec<TransactionReceipt>, Error>> + std::marker::Send;
}

/// A source which requests data from a remote server using JSON RPC.
/// Using this source directly is not recommended, as it does not verify the
/// correctness of the returned data, thereby implicitly trusting the RPC server.
#[derive(Debug)]
pub struct NetworkSource {
    http_client: HttpClient,
}

impl NetworkSource {
    #[allow(dead_code)]
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
    ) -> Result<(BlockHeader, Vec<Transaction>), Error> {
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
        let BlockHeaderWithTransactions {
            block_header,
            transactions,
        } = result.ok_or(Error::DataDoesNotExist)?;
        Ok((block_header, transactions))
    }

    async fn get_block_receipt(&self, block_number: u64) -> Result<Vec<TransactionReceipt>, Error> {
        // see: https://docs.chainstack.com/reference/ethereum-getblockreceipts
        let receipts: Option<Vec<TransactionReceipt>> = self
            .http_client
            .request("eth_getBlockReceipts", rpc_params![block_number.to_hex()])
            .await?;
        receipts.ok_or(Error::DataDoesNotExist)
    }
}

#[cfg(test)]
mod tests {
    use bertha_types::TransactionType;
    use serde::{Deserialize, Serialize};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers};

    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct RpcRequest {
        jsonrpc: String,
        id: usize,
        method: String,
        params: Vec<serde_json::Value>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct RpcResponse {
        jsonrpc: String,
        id: usize,
        result: serde_json::Value,
    }

    fn build_mock_server_request_handler<T>(
        method: &str,
        id: usize,
        params: Vec<serde_json::Value>,
        result: T,
    ) -> Mock
    where
        T: Send + Sync + Serialize + 'static,
    {
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/"))
            .and(matchers::body_json(RpcRequest {
                jsonrpc: "2.0".to_owned(),
                id,
                method: method.to_owned(),
                params,
            }))
            .respond_with(move |req: &Request| {
                let req = serde_json::from_slice::<RpcRequest>(&req.body).unwrap();
                ResponseTemplate::new(200).set_body_json(RpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: serde_json::to_value(&result).unwrap(),
                })
            })
            .expect(1) // expect the request to be made once
    }

    #[test]
    fn new_with_invalid_url_returns_error() {
        let network = NetworkSource::try_new("invalid_url");
        assert!(network.is_err());
    }

    #[tokio::test]
    async fn get_block_by_identifier_requests_and_deserializes_block_header() {
        let mock_server = MockServer::start().await;
        let network_source = NetworkSource::try_new(mock_server.uri()).unwrap();

        let block_header_with_transactions = BlockHeaderWithTransactions {
            block_header: BlockHeader::default(),
            transactions: Vec::new(),
        };

        let block_number = 123456;
        mock_server
            .register(build_mock_server_request_handler(
                "eth_getBlockByNumber",
                0,
                vec![
                    serde_json::to_value(block_number.to_hex()).unwrap(),
                    serde_json::to_value(true).unwrap(),
                ],
                block_header_with_transactions.clone(),
            ))
            .await;

        let received_block = network_source
            .get_block_header_with_transactions(block_number)
            .await;
        assert!(received_block.is_ok());
        let received_block = received_block.unwrap();
        assert_eq!(
            received_block.0,
            block_header_with_transactions.block_header
        );
        assert_eq!(
            received_block.1,
            block_header_with_transactions.transactions
        );
    }

    #[tokio::test]
    async fn get_block_receipt_requests_and_deserializes_receipts() {
        let mock_server = MockServer::start().await;

        let network_source = NetworkSource::try_new(mock_server.uri()).unwrap();

        let block_receipt = vec![TransactionReceipt {
            cumulative_gas_used: u64::default(),
            logs: Vec::default(),
            status: u64::default(),
            transaction_type: TransactionType::Legacy,
        }];

        let block_number = 123456;
        mock_server
            .register(build_mock_server_request_handler(
                "eth_getBlockReceipts",
                0,
                vec![serde_json::to_value(block_number.to_hex()).unwrap()],
                block_receipt.clone(),
            ))
            .await;

        let received_block_receipt = network_source.get_block_receipt(block_number).await;
        assert!(received_block_receipt.is_ok());
        assert_eq!(received_block_receipt.unwrap(), block_receipt);
    }

    #[test]
    fn block_header_with_transactions_serializes_and_deserializes_correctly() {
        let block_header = BlockHeader::default();
        let transactions = vec![Transaction::default()];
        let block_header_with_transactions = BlockHeaderWithTransactions {
            block_header,
            transactions,
        };

        let serialized = serde_json::to_string_pretty(&block_header_with_transactions).unwrap();

        // test that the header fields are flattened
        assert!(serialized.contains("number"));
        assert!(serialized.contains("parentHash"));
        // check that transactions are included
        assert!(serialized.contains("transactions"));

        let deserialized: BlockHeaderWithTransactions = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, block_header_with_transactions);
    }
}
