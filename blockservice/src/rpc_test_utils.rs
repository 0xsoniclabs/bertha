use std::{
    sync::atomic::{AtomicU16, AtomicU64},
    vec::IntoIter,
};

use hyper_util::rt::TokioIo;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;

use crate::{
    proto_rpc::{self, EncodedBlock, block_rpc_client::BlockRpcClient},
    rpc_client::RpcClient,
};

pub const SERVER_STARTUP_TIMER: u64 = 100; // milliseconds
static NEXT_TEST_PORT: AtomicU16 = AtomicU16::new(50051);

/// A mock implementation of the BlockRpc service for testing purposes.
/// This server can be used to simulate responses for the BlockRpc trait
pub struct MockRpcServer {
    pub get_block_response: Result<Option<EncodedBlock>, tonic::Status>,
    pub get_block_range_response:
        Result<Vec<Vec<Result<EncodedBlock, tonic::Status>>>, tonic::Status>,
    pub list_response: Result<proto_rpc::EncodedChainRanges, tonic::Status>,
    pub get_block_response_index: AtomicU64,
    pub get_block_range_response_index: AtomicU64,
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
            list_response: Ok(proto_rpc::EncodedChainRanges {
                chain_ranges: vec![],
            }),
            get_block_range_response_index: AtomicU64::new(0),
            get_block_response_index: AtomicU64::new(0),
            get_block_response: Ok(None),
            get_block_range_response: Ok(vec![]),
        }
    }
}

#[tonic::async_trait]
impl proto_rpc::block_rpc_server::BlockRpc for MockRpcServer {
    /// Mock implementation of the `get_block` method.
    /// Returns the block response set in the server.
    async fn get_block(
        &self,
        _request: tonic::Request<proto_rpc::BlockRequest>,
    ) -> Result<tonic::Response<proto_rpc::EncodedBlock>, tonic::Status> {
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
        _request: tonic::Request<proto_rpc::BlockRangeRequest>,
    ) -> Result<tonic::Response<Self::GetBlockRangeStream>, tonic::Status> {
        match &self.get_block_range_response {
            Ok(blocks) => {
                let blocks = blocks
                    .get(
                        self.get_block_range_response_index
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                            as usize,
                    )
                    .unwrap();
                Ok(tonic::Response::new(futures::stream::iter(blocks.clone())))
            }
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }

    async fn list(
        &self,
        _request: tonic::Request<proto_rpc::ListRequest>,
    ) -> Result<tonic::Response<proto_rpc::EncodedChainRanges>, tonic::Status> {
        match &self.list_response {
            Ok(chain_ranges) => Ok(tonic::Response::new(chain_ranges.clone())),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }
}

//TODO: add testing for the MockRpcServer

/// A mock server that can be stopped and polled for readiness.
/// This is useful for testing commands where the client is constructed from an HTTP URI.
pub struct DestructibleServer {
    start_channel: tokio::sync::oneshot::Sender<()>,
    end_channel: tokio::sync::oneshot::Receiver<()>,
    port: u16,
}

impl DestructibleServer {
    /// Construct a new `DestructibleServer` with the given mock server and initialize the
    /// communication channels.
    pub fn new(mock_server: MockRpcServer) -> Self {
        // Ensure no contention on the port
        let port = NEXT_TEST_PORT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // Create a pair of duplex streams to simulate the server and client communication
        let (start_tx, mut start_rc) = tokio::sync::oneshot::channel();
        let (mut end_tx, end_rc) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            Server::builder()
                .add_service(proto_rpc::block_rpc_server::BlockRpcServer::new(
                    mock_server,
                ))
                .serve_with_shutdown(format!("[::1]:{port}").parse().unwrap(), async move {
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
            port,
        }
    }

    /// Get the URL of the server in the format `http://[::1]:<port>`.
    pub fn get_url(&self) -> String {
        format!("http://[::1]:{}", self.port)
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

//TODO: add testing for the DestructibleServer

/// Start a mock server and a client connected through a duplex stream.
/// The server will respond to a single request before terminating.
/// This is useful for scenarios where the client needs to be explicitly constructed.
pub async fn get_mock_server_and_client(mock_server: MockRpcServer) -> RpcClient {
    let (client, server) = tokio::io::duplex(1024);
    let mock_server = Server::builder()
        .add_service(proto_rpc::block_rpc_server::BlockRpcServer::new(
            mock_server,
        ))
        .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server)))
        .await;

    assert!(mock_server.is_ok(), "Server failed to start");

    let mut client = Some(client);
    let channel = Endpoint::try_from("http://[::1]:50051")
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
