use std::error::Error;

use tonic::{Request, Streaming, transport::Channel};

use crate::proto_rpc::{
    BlockRangeRequest, BlockRequest, EncodedBlock, block_rpc_client::BlockRpcClient,
};

/// A client for interacting with the Block RPC service.
pub struct RpcClient {
    client: BlockRpcClient<Channel>,
}

impl RpcClient {
    /// Creates a new [RpcClient] by connecting to the specified URL.
    pub async fn try_new(url: String) -> Result<Self, Box<dyn Error>> {
        let client = BlockRpcClient::connect(url).await?;
        Ok(Self { client })
    }

    #[cfg(test)]
    /// Creates a new [RpcClient] with the provided [BlockRpcClient].
    pub(crate) fn new(client: BlockRpcClient<Channel>) -> Self {
        Self { client }
    }

    /// Query a block by its chain ID and number.
    pub async fn get_block(
        &mut self,
        chain_id: u64,
        number: u64,
    ) -> Result<EncodedBlock, Box<dyn Error>> {
        let request = Request::new(BlockRequest { chain_id, number });
        let response = self.client.get_block(request).await?;
        Ok(response.into_inner())
    }

    /// Query a range of blocks by chain ID, from block number to block number.
    pub async fn get_block_range(
        &mut self,
        chain_id: u64,
        from: u64,
        to: u64,
    ) -> Result<Streaming<EncodedBlock>, Box<dyn Error>> {
        let range = BlockRangeRequest { chain_id, from, to };

        let stream = self
            .client
            .get_block_range(Request::new(range))
            .await?
            .into_inner();

        Ok(stream)
    }
}

#[cfg(test)]
pub mod tests {

    use tokio_stream::StreamExt;

    use super::*;
    use crate::rpc_test_utils::{DestructibleServer, MockRpcServer, get_mock_server_and_client};

    #[tokio::test]
    async fn try_new_connects_successfully() {
        let url = "http://[::1]:50051".to_string();
        let mut server = DestructibleServer::new(MockRpcServer::new());
        server.wait_for_start().await;
        let rpc_client = RpcClient::try_new(url).await;
        assert!(rpc_client.is_ok(), "Failed to connect to RPC server");
        server.shutdown();
    }

    #[tokio::test]
    async fn try_from_fails_on_invalid_server() {
        // Invalid URL
        {
            let url = "invalid_url".to_string();
            let rpc_client = RpcClient::try_new(url).await;
            assert!(rpc_client.is_err(), "Expected error for invalid URL");
        }
        // Non-existing server
        {
            let url = "http://[::1]:9999".to_string(); // Assuming no server is running on this port
            let rpc_client = RpcClient::try_new(url).await;
            assert!(
                rpc_client.is_err(),
                "Expected error for non-existing server"
            );
        }
    }

    #[tokio::test]
    async fn get_block_returns_block_successfully() {
        // Block exist
        {
            let encoded_block = EncodedBlock {
                data: vec![1, 2, 3, 4],
            };
            let mut mock_rpc_server = MockRpcServer::new();
            mock_rpc_server.get_block_response = Ok(Some(encoded_block.clone()));

            let mut rpc_client = get_mock_server_and_client(mock_rpc_server).await;
            let block = rpc_client.get_block(1, 1).await.unwrap();
            assert_eq!(block, encoded_block, "Block data does not match");
        }
        // Block not found
        {
            let mock_rpc_server = MockRpcServer::new();

            let mut rpc_client = get_mock_server_and_client(mock_rpc_server).await;
            let result = rpc_client.get_block(1, 1).await;
            assert!(result.is_err(), "Expected error for non-existent block");
        }
    }

    #[tokio::test]
    async fn get_block_propagates_error() {
        let mut mock_rpc_server = MockRpcServer::new();
        mock_rpc_server.get_block_response = Err(tonic::Status::internal("Internal error"));

        let mut rpc_client = get_mock_server_and_client(mock_rpc_server).await;
        let result = rpc_client.get_block(1, 1).await;
        assert!(result.is_err(), "Expected error for internal server error");
    }

    #[tokio::test]
    async fn get_block_range_returns_blocks_successfully() {
        let mut mock_server = MockRpcServer::new();
        mock_server.get_block_range_response = vec![
            Ok(EncodedBlock {
                data: vec![1, 2, 3],
            }),
            Ok(EncodedBlock {
                data: vec![4, 5, 6],
            }),
        ];

        let mut rpc_client = get_mock_server_and_client(mock_server).await;
        let mut stream = rpc_client.get_block_range(1, 0, 2).await.unwrap();
        assert!(stream.next().await.unwrap().unwrap().data == vec![1, 2, 3]);
        assert!(stream.next().await.unwrap().unwrap().data == vec![4, 5, 6]);
        assert!(
            stream.next().await.is_none(),
            "Stream should end after two blocks"
        );
    }

    #[tokio::test]
    async fn get_block_range_propagates_error() {
        let mut mock_server = MockRpcServer::new();
        mock_server.get_block_range_response = vec![
            Ok(EncodedBlock {
                data: vec![1, 2, 3],
            }),
            Err(tonic::Status::internal("Internal error")),
        ];

        let mut rpc_client = get_mock_server_and_client(mock_server).await;
        let mut stream = rpc_client.get_block_range(1, 0, 2).await.unwrap();
        assert!(stream.next().await.unwrap().unwrap().data == vec![1, 2, 3]);
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(error.code(), tonic::Code::Internal);
        assert_eq!(error.message(), "Internal error");
        assert!(
            stream.next().await.is_none(),
            "Stream should end after error"
        );
    }
}
