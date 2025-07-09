use std::vec::IntoIter;

use hyper_util::rt::TokioIo;
use mockall::mock;
use tokio::net::TcpListener;
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

        // NOTE: [GetBlockRangeStream] is defined in [BlockRpc] and used as the return type of the `get_block_range` function. However, the mock! macro is unable to find it because of some namespace issues. Because of this, we ignore this field and manually set the return type of `get_block_range` to the same value.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_correctly_handles_start_and_shutdown() {
        let url;
        {
            let server = TestServer::new(MockRpcServer::new()).await;
            url = server.address.clone();
            {
                let client = BlockRpcClient::connect(url.clone()).await;
                assert!(client.is_ok(), "Client should connect to the server");
            }
        }
        // Ensure the server has shut down
        tokio::task::yield_now().await;
        {
            let client = BlockRpcClient::connect(url.clone()).await;
            assert!(client.is_err(), "Client should not connect after shutdown");
        }
    }
}
