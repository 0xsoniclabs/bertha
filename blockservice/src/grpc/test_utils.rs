use std::vec::IntoIter;

use hyper_util::rt::TokioIo;
use mockall::mock;
use tonic::transport::{Endpoint, Server, Uri};
use tower::service_fn;

use crate::grpc::{
    client::RpcClient,
    proto_rpc::{
        BlockRangeRequest, BlockRequest, ChainRanges, EncodedBlock, ListRequest,
        block_rpc_client::BlockRpcClient,
        block_rpc_server::{BlockRpc, BlockRpcServer},
    },
};

pub const SERVER_STARTUP_TIMER: u64 = 100; // milliseconds

mock!(
    pub RpcServer {}

#[tonic::async_trait]
    impl BlockRpc for RpcServer {
    async fn get_block(
        &self,
            request: tonic::Request<BlockRequest>,
        ) -> Result<tonic::Response<EncodedBlock>, tonic::Status>;

        // NOTE: mock! cannot find this name, so we ignore it and manually add it to get_block_range
    type GetBlockRangeStream = futures::stream::Iter<IntoIter<Result<EncodedBlock, tonic::Status>>>;

        #[allow(clippy::type_complexity)]
    async fn get_block_range(
        &self,
            request: tonic::Request<BlockRangeRequest>,
        ) -> Result<tonic::Response<futures::stream::Iter<IntoIter<Result<EncodedBlock, tonic::Status>>>>, tonic::Status>;

    async fn list(
        &self,
            request: tonic::Request<ListRequest>,
        ) -> Result<tonic::Response<ChainRanges>, tonic::Status>;
    }
);

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
