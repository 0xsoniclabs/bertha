use std::vec::IntoIter;

use hyper_util::rt::TokioIo;
use mockall::mock;
use tokio::net::TcpListener;
use tonic::{
    metadata::{Ascii, MetadataValue},
    transport::{Endpoint, Server, Uri},
};
use tower::service_fn;

use crate::grpc::{
    client::RpcClient,
    proto_rpc::{
        BlockRangeRequest, ChainRanges, EncodedBlock, ListRequest, StateUpdates,
        StateUpdatesRequest,
        block_rpc_client::BlockRpcClient,
        block_rpc_server::{BlockRpc, BlockRpcServer},
    },
};

pub const SERVER_STARTUP_TIMER: u64 = 100; // milliseconds

mock!(
    pub RpcServer {}

    #[tonic::async_trait]
    impl BlockRpc for RpcServer {

        // NOTE: [GetBlockRangeStream] is an associated type of [BlockRpc] and used as the return type of the `get_block_range` function. For some reason, the mock! macro appears to be unable to find this type. Because of this, we simply manually expand the return type of `get_block_range`.
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

        async fn get_state_updates(
            &self,
            request: tonic::Request<StateUpdatesRequest>,
        ) -> Result<tonic::Response<StateUpdates>, tonic::Status>;
    }
);

/// A server that can be used to spawn a mock gRPC server for testing purposes.
pub struct TestServer {
    end_channel: tokio::sync::oneshot::Receiver<()>,
    pub address: String,
}

impl TestServer {
    /// Start a new [TestServer] with the provided mock server on a random available port.
    pub async fn new(mock_server: MockRpcServer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (mut end_tx, end_rc) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            Server::builder()
                .add_service(BlockRpcServer::new(mock_server))
                .serve_with_incoming_shutdown(
                    tokio_stream::wrappers::TcpListenerStream::new(listener),
                    async move {
                        // Wait for the shutdown signal
                        end_tx.closed().await;
                    },
                )
                .await
                .unwrap();
        });

        TestServer {
            end_channel: end_rc,
            address: format!("http://{addr}"),
        }
    }
}

impl Drop for TestServer {
    /// Drop the [TestServer] and close the end channel to signal shutdown.
    fn drop(&mut self) {
        self.end_channel.close();
    }
}

/// Start a mock server and a client connected through a duplex stream.
/// The server will respond to a single request before terminating.
/// This is useful for scenarios where the client needs to be explicitly constructed.
pub async fn get_mock_server_and_client(
    mock_server: MockRpcServer,
    auth_token: Option<MetadataValue<Ascii>>,
) -> RpcClient {
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

    RpcClient::new(BlockRpcClient::new(channel), auth_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: This test requires to be run with the `current_thread` runtime flavor to be sure that
    // tokio::task::yield_now() gives execution back to the server task, allowing it to shut down
    // properly.
    #[tokio::test(flavor = "current_thread")]
    async fn test_server_does_not_leak_server_after_drop() {
        let server = TestServer::new(MockRpcServer::new()).await;
        let url = server.address.clone();
        {
            let res = BlockRpcClient::connect(url.clone()).await;
            assert!(res.is_ok(), "Client should connect to the server");
        }
        drop(server);

        // Ensure the server has shut down
        tokio::task::yield_now().await;
        {
            let client = BlockRpcClient::connect(url.clone()).await;
            assert!(client.is_err(), "Client should not connect after shutdown");
        }
    }
}
