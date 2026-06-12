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

use tonic::{
    Request, Streaming,
    metadata::{Ascii, MetadataValue},
    transport::Channel,
};

use crate::grpc::{
    GRPC_COMPRESSION_ALGORITHM,
    auth::AUTHORIZATION_HEADER_NAME,
    proto_rpc::{
        BlockRangeRequest, ChainRanges, EncodedBlock, ListRequest, Metadata, MetadataRequest,
        block_rpc_client::BlockRpcClient,
    },
};

/// A client for interacting with the Block RPC service.
pub struct RpcClient {
    client: BlockRpcClient<Channel>,
    auth_token: Option<MetadataValue<Ascii>>,
}

impl RpcClient {
    /// Creates a new [RpcClient] by connecting to the specified URL.
    pub async fn try_new(
        url: String,
        auth_token: Option<MetadataValue<Ascii>>,
    ) -> Result<Self, tonic::transport::Error> {
        let client = BlockRpcClient::connect(url)
            .await?
            .accept_compressed(GRPC_COMPRESSION_ALGORITHM);
        Ok(Self { client, auth_token })
    }

    #[cfg(test)]
    /// Creates a new [RpcClient] with the provided [BlockRpcClient].
    pub(crate) fn new(
        client: BlockRpcClient<Channel>,
        auth_token: Option<MetadataValue<Ascii>>,
    ) -> Self {
        Self { client, auth_token }
    }

    /// Query a range of blocks by chain ID, from block number to block number.
    pub async fn get_block_range(
        &mut self,
        chain_id: u64,
        from: u64,
        to: u64,
    ) -> Result<Streaming<EncodedBlock>, tonic::Status> {
        let mut request = Request::new(BlockRangeRequest { chain_id, from, to });

        if let Some(token) = &self.auth_token {
            request
                .metadata_mut()
                .insert(AUTHORIZATION_HEADER_NAME, token.clone());
        }

        let stream = self.client.get_block_range(request).await?.into_inner();

        Ok(stream)
    }

    /// Queries the available block ranges of all chains or a specific chain.
    pub async fn list(&mut self, chain_id: Option<u64>) -> Result<ChainRanges, tonic::Status> {
        let mut request = Request::new(ListRequest { chain_id });

        if let Some(token) = &self.auth_token {
            request
                .metadata_mut()
                .insert(AUTHORIZATION_HEADER_NAME, token.clone());
        }

        let response = self.client.list(request).await?;
        Ok(response.into_inner())
    }

    pub async fn get_metadata(&mut self, chain_id: u64) -> Result<Metadata, tonic::Status> {
        let mut request = Request::new(MetadataRequest { chain_id });

        if let Some(token) = &self.auth_token {
            request
                .metadata_mut()
                .insert(AUTHORIZATION_HEADER_NAME, token.clone());
        }

        let response = self.client.get_metadata(request).await?;
        Ok(response.into_inner())
    }
}

#[cfg(test)]
pub mod tests {
    use tokio_stream::StreamExt;

    use super::*;
    use crate::grpc::{
        auth,
        proto_rpc::{BlockRange, ChainRange, Metadata},
        test_utils::{MockRpcServer, TestServer, get_mock_server_and_client},
    };

    #[tokio::test]
    async fn try_new_connects_successfully() {
        let server = TestServer::new(MockRpcServer::new()).await;
        let rpc_client = RpcClient::try_new(server.address.clone(), None).await;
        assert!(rpc_client.is_ok(), "Failed to connect to RPC server");
    }

    #[tokio::test]
    async fn try_from_fails_on_invalid_server() {
        // Invalid URL
        {
            let url = "invalid_url".to_string();
            let rpc_client = RpcClient::try_new(url, None).await;
            assert!(rpc_client.is_err(), "Expected error for invalid URL");
        }
        // Non-existing server
        {
            let url = "http://[::1]:9999".to_string(); // Assuming no server is running on this port
            let rpc_client = RpcClient::try_new(url, None).await;
            assert!(
                rpc_client.is_err(),
                "Expected error for non-existing server"
            );
        }
    }

    #[tokio::test]
    async fn get_block_range_returns_blocks_successfully() {
        let mut mock_server = MockRpcServer::new();
        mock_server.expect_get_block_range().returning(|_| {
            let blocks = vec![
                Ok(EncodedBlock {
                    data: vec![1, 2, 3],
                    number: 1,
                }),
                Ok(EncodedBlock {
                    data: vec![4, 5, 6],
                    number: 2,
                }),
            ];
            Ok(tonic::Response::new(futures::stream::iter(blocks)))
        });

        let mut rpc_client = get_mock_server_and_client(mock_server, None).await;
        let mut stream = rpc_client.get_block_range(1, 0, 2).await.unwrap();
        assert!(stream.next().await.unwrap().unwrap().data == vec![1, 2, 3]);
        assert!(stream.next().await.unwrap().unwrap().data == vec![4, 5, 6]);
        assert!(
            stream.next().await.is_none(),
            "Stream should end after two blocks"
        );
    }

    #[tokio::test]
    async fn get_block_range_sets_auth_token() {
        let auth_token = Some(auth::token_to_metadata_value("my-token").unwrap());
        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_get_block_range()
            .withf({
                let auth_token = auth_token.clone();
                move |request| {
                    if auth_token.is_some() {
                        let req_token = request.metadata().get(AUTHORIZATION_HEADER_NAME);
                        auth_token.as_ref() == req_token
                    } else {
                        true
                    }
                }
            })
            .returning(|_| {
                let blocks = vec![
                    Ok(EncodedBlock {
                        data: vec![1, 2, 3],
                        number: 1,
                    }),
                    Ok(EncodedBlock {
                        data: vec![4, 5, 6],
                        number: 2,
                    }),
                ];
                Ok(tonic::Response::new(futures::stream::iter(blocks)))
            });

        let mut rpc_client = get_mock_server_and_client(mock_server, auth_token).await;
        let result = rpc_client.get_block_range(1, 0, 2).await;
        assert!(result.is_ok(), "Failed to get block range with auth token");
    }

    #[tokio::test]
    async fn get_block_range_propagates_error() {
        // Error from the server
        {
            let mut mock_server = MockRpcServer::new();
            mock_server
                .expect_get_block_range()
                .returning(|_| Err(tonic::Status::internal("Internal error")));

            let mut rpc_client = get_mock_server_and_client(mock_server, None).await;
            let result = rpc_client.get_block_range(1, 0, 2).await;
            assert!(result.is_err(), "Expected error for internal server error");
            let err = result.unwrap_err();
            assert_eq!(err.code(), tonic::Code::Internal);
            assert!(err.message().contains("Internal error"));
        }
        // Error from the db
        {
            let mut mock_server = MockRpcServer::new();
            mock_server.expect_get_block_range().returning(|_| {
                let blocks = vec![
                    Ok(EncodedBlock {
                        data: vec![1, 2, 3],
                        number: 1,
                    }),
                    Err(tonic::Status::internal("Internal error")),
                ];
                Ok(tonic::Response::new(futures::stream::iter(blocks)))
            });

            let mut rpc_client = get_mock_server_and_client(mock_server, None).await;
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

    #[tokio::test]
    async fn list_returns_chain_ranges_successfully() {
        // ranges exist
        {
            let encoded_chain_ranges = ChainRanges {
                chain_ranges: vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![BlockRange { from: 0, to: 10 }],
                }],
            };
            let mut mock_rpc_server = MockRpcServer::new();
            mock_rpc_server.expect_list().returning({
                let encoded_chain_ranges = encoded_chain_ranges.clone();
                move |_| Ok(tonic::Response::new(encoded_chain_ranges.clone()))
            });

            let mut rpc_client = get_mock_server_and_client(mock_rpc_server, None).await;
            let ranges = rpc_client.list(None).await.unwrap();
            assert_eq!(ranges, encoded_chain_ranges, "Chain ranges do not match");
        }
        // ranges do not exist = empty
        {
            let mut mock_rpc_server = MockRpcServer::new();
            mock_rpc_server.expect_list().returning(|_| {
                Ok(tonic::Response::new(ChainRanges {
                    chain_ranges: Vec::new(),
                }))
            });
            let mut rpc_client = get_mock_server_and_client(mock_rpc_server, None).await;
            let ranges = rpc_client.list(Some(1)).await.unwrap();
            assert_eq!(
                ranges,
                ChainRanges {
                    chain_ranges: Vec::new()
                },
                "Chain ranges should be empty"
            );
        }
    }

    #[tokio::test]
    async fn list_sets_auth_token() {
        let auth_token = Some(auth::token_to_metadata_value("my-token").unwrap());
        let encoded_chain_ranges = ChainRanges {
            chain_ranges: vec![ChainRange {
                chain_id: 1,
                block_ranges: vec![BlockRange { from: 0, to: 10 }],
            }],
        };
        let mut mock_rpc_server = MockRpcServer::new();
        mock_rpc_server
            .expect_list()
            .withf({
                let auth_token = auth_token.clone();
                move |request| {
                    if auth_token.is_some() {
                        let req_token = request.metadata().get(AUTHORIZATION_HEADER_NAME);
                        auth_token.as_ref() == req_token
                    } else {
                        true
                    }
                }
            })
            .returning(move |_| Ok(tonic::Response::new(encoded_chain_ranges.clone())));

        let mut rpc_client = get_mock_server_and_client(mock_rpc_server, auth_token).await;
        let result = rpc_client.list(None).await;
        assert!(
            result.is_ok(),
            "Failed to list block ranges with auth token"
        );
    }

    #[tokio::test]
    async fn list_propagates_error() {
        let mut mock_rpc_server = MockRpcServer::new();
        mock_rpc_server
            .expect_list()
            .returning(|_| Err(tonic::Status::internal("Internal error")));

        let mut rpc_client = get_mock_server_and_client(mock_rpc_server, None).await;
        let result = rpc_client.list(None).await;
        assert!(result.is_err(), "Expected error for internal server error");
    }

    #[tokio::test]
    async fn get_metadata_returns_metadata_successfully() {
        let expected_metadata = Metadata {
            upgrade_heights: Some(b"upgrade-heights".to_vec()),
            corrections: Some(b"corrections".to_vec()),
        };

        let mut mock_server = MockRpcServer::new();
        mock_server.expect_get_metadata().returning({
            let metadata = expected_metadata.clone();
            move |_| Ok(tonic::Response::new(metadata.clone()))
        });
        let mut rpc_client = get_mock_server_and_client(mock_server, None).await;
        let metadata = rpc_client.get_metadata(1).await.unwrap();
        assert_eq!(metadata, expected_metadata);
    }

    #[tokio::test]
    async fn get_metadata_propagates_error() {
        let mut mock_server = MockRpcServer::new();
        mock_server
            .expect_get_metadata()
            .returning(|_| Err(tonic::Status::internal("Internal error")));
        let mut rpc_client = get_mock_server_and_client(mock_server, None).await;
        let result = rpc_client.get_metadata(1).await;
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("Internal error"));
    }

    #[tokio::test]
    async fn get_metadata_sets_auth_token() {
        let auth_token = Some(auth::token_to_metadata_value("my-token").unwrap());
        let mut mock_rpc_server = MockRpcServer::new();
        mock_rpc_server
            .expect_get_metadata()
            .withf({
                let auth_token = auth_token.clone();
                move |request| {
                    if auth_token.is_some() {
                        let req_token = request.metadata().get(AUTHORIZATION_HEADER_NAME);
                        auth_token.as_ref() == req_token
                    } else {
                        true
                    }
                }
            })
            .returning({
                move |_| {
                    Ok(tonic::Response::new(Metadata {
                        upgrade_heights: None,
                        corrections: None,
                    }))
                }
            });

        let mut rpc_client = get_mock_server_and_client(mock_rpc_server, auth_token).await;
        let result = rpc_client.get_metadata(1).await;
        assert!(result.is_ok(), "Failed to get metadata with auth token");
    }
}
