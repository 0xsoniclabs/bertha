use std::vec::IntoIter;

use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;

use crate::grpc::{
    client::RpcClient,
    proto_rpc::{
        BlockRangeRequest, BlockRequest, EncodedBlock, EncodedChainRanges, ListRequest,
        block_rpc_client::BlockRpcClient,
        block_rpc_server::{BlockRpc, BlockRpcServer},
    },
};

pub const SERVER_STARTUP_TIMER: u64 = 100; // milliseconds

/// A mock implementation of the BlockRpc service for testing purposes.
/// This server can be used to simulate responses for the BlockRpc trait
pub struct MockRpcServer {
    pub get_block_response: Result<Option<EncodedBlock>, tonic::Status>,
    pub get_block_range_response: Result<Vec<Result<EncodedBlock, tonic::Status>>, tonic::Status>,
    pub list_response: Result<EncodedChainRanges, tonic::Status>,
}

impl Default for MockRpcServer {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRpcServer {
    /// Construct a new MockRpcServer with default values.
    pub fn new() -> Self {
        MockRpcServer {
            get_block_response: Ok(None),
            get_block_range_response: Ok(vec![]),
            list_response: Ok(EncodedChainRanges {
                chain_ranges: vec![],
            }),
        }
    }
}

#[tonic::async_trait]
impl BlockRpc for MockRpcServer {
    /// Mock implementation of the `get_block` method.
    /// Returns the block response set in the server.
    async fn get_block(
        &self,
        _request: tonic::Request<BlockRequest>,
    ) -> Result<tonic::Response<EncodedBlock>, tonic::Status> {
        match &self.get_block_response {
            Ok(Some(block)) => Ok(tonic::Response::new(block.clone())),
            Ok(None) => Err(tonic::Status::not_found("")),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }

    type GetBlockRangeStream = futures::stream::Iter<IntoIter<Result<EncodedBlock, tonic::Status>>>;

    /// Mock implementation of the `get_block_range` method.
    /// Returns the stream of blocks set in the server.
    async fn get_block_range(
        &self,
        _request: tonic::Request<BlockRangeRequest>,
    ) -> Result<tonic::Response<Self::GetBlockRangeStream>, tonic::Status> {
        match &self.get_block_range_response {
            Ok(blocks) => Ok(tonic::Response::new(futures::stream::iter(blocks.clone()))),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }

    async fn list(
        &self,
        _request: tonic::Request<ListRequest>,
    ) -> Result<tonic::Response<EncodedChainRanges>, tonic::Status> {
        match &self.list_response {
            Ok(chain_ranges) => Ok(tonic::Response::new(chain_ranges.clone())),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }
}

/// A mock server that can be stopped and polled for readiness.
/// This is useful for testing commands where the client is constructed from an HTTP URI.
pub struct DestructibleServer {
    start_channel: tokio::sync::oneshot::Sender<()>,
    end_channel: tokio::sync::oneshot::Receiver<()>,
}

impl DestructibleServer {
    /// Construct a new `DestructibleServer` with the given mock server and initialize the
    /// communication channels.
    pub fn new(mock_server: MockRpcServer) -> Self {
        // Create a pair of duplex streams to simulate the server and client communication
        let (start_tx, mut start_rc) = tokio::sync::oneshot::channel();
        let (mut end_tx, end_rc) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            Server::builder()
                .add_service(BlockRpcServer::new(mock_server))
                .serve_with_shutdown("[::1]:50051".parse().unwrap(), async move {
                    // Notify that the server has started
                    start_rc.close();
                    // Wait for the shutdown signal
                    end_tx.closed().await;
                })
                .await
                .unwrap();
        });

        DestructibleServer {
            start_channel: start_tx,
            end_channel: end_rc,
        }
    }

    /// A function to wait for the server to start.
    pub async fn wait_for_start(&mut self) {
        // Wait for the server to start
        self.start_channel.closed().await;
    }

    /// Shutdown and consumes the server
    pub fn shutdown(mut self) {
        self.end_channel.close();
    }
}

/// Start a mock server and a client connected through a duplex stream.
/// The server will respond to a single request before terminating.
/// This is useful for scenarios where the client needs to be explicitly constructed.
pub async fn get_mock_server_and_client(mock_server: MockRpcServer) -> RpcClient {
    let (client, server) = tokio::io::duplex(1024);
    let mock_server = Server::builder()
        .add_service(BlockRpcServer::new(mock_server))
        .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)))
        .await;

    assert!(mock_server.is_ok(), "Server failed to start");

    let mut client = Some(client);
    let channel = Endpoint::try_from("http://[::]:50051")
        .unwrap()
        .connect_with_connector(service_fn(move |_: Uri| {
            let client = client.take();

            async move {
                if let Some(client) = client {
                    Ok(TokioIo::new(client))
                } else {
                    Err(std::io::Error::other("Client already taken"))
                }
            }
        }))
        .await
        .unwrap();

    RpcClient::new(BlockRpcClient::new(channel))
}
