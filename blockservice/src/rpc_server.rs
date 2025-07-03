use std::{
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::Arc,
};

use tokio_stream::wrappers::ReceiverStream;
use tonic::{codec::CompressionEncoding, transport::Server};

use crate::{blockdb::BlockDb, proto_rpc};

/// A gRPC server that provides access to block data stored in a database.
#[derive(Debug)]
pub struct RpcServer<Db: BlockDb + Send + Sync + 'static> {
    db: Arc<Db>,
}

impl<Db> RpcServer<Db>
where
    Db: BlockDb + Send + Sync + 'static,
{
    /// Creates a new [RpcServer] instance with the provided database.
    pub fn new(db: Db) -> Self {
        RpcServer { db: Arc::new(db) }
    }

    /// Starts the gRPC server on the specified port.
    pub async fn serve(self, port: u16) -> Result<(), Box<dyn std::error::Error>> {
        // This allows both IPv4 and IPv6 connections
        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port);

        println!("Listening on {addr}...");

        Server::builder()
            .add_service(
                proto_rpc::block_rpc_server::BlockRpcServer::new(self)
                    .send_compressed(CompressionEncoding::Gzip),
            )
            .serve(addr)
            .await?;

        Ok(())
    }
}

#[tonic::async_trait]
impl<Db> proto_rpc::block_rpc_server::BlockRpc for RpcServer<Db>
where
    Db: BlockDb + Send + Sync + 'static,
{
    /// Returns a block by its chain ID and number.
    async fn get_block(
        &self,
        request: tonic::Request<proto_rpc::BlockRequest>,
    ) -> Result<tonic::Response<proto_rpc::EncodedBlock>, tonic::Status> {
        let remote_addr = request.remote_addr();
        let proto_rpc::BlockRequest { chain_id, number } = request.into_inner();
        let encoded_block = self.db.get_raw(chain_id, number);

        match remote_addr {
            Some(addr) => {
                println!("Received request for block {number} on chain {chain_id} from {addr}");
            }
            None => println!("Received request for block {number} on chain {chain_id}"),
        }

        match encoded_block {
            Ok(Some(block)) => Ok(tonic::Response::new(proto_rpc::EncodedBlock {
                data: block,
            })),
            Ok(None) => Err(tonic::Status::not_found(format!(
                "Block {number} not found for chain {chain_id}"
            ))),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }

    type GetBlockRangeStream = ReceiverStream<Result<proto_rpc::EncodedBlock, tonic::Status>>;

    /// Returns a stream of blocks in the specified range for a given chain ID.
    async fn get_block_range(
        &self,
        request: tonic::Request<proto_rpc::BlockRangeRequest>,
    ) -> Result<tonic::Response<Self::GetBlockRangeStream>, tonic::Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(1000);

        let remote_addr = request.remote_addr();
        let proto_rpc::BlockRangeRequest { chain_id, from, to } = request.into_inner();

        if from > to {
            return Err(tonic::Status::invalid_argument(
                "Invalid block range: 'from' must be less than or equal to 'to'",
            ));
        }

        match remote_addr {
            Some(remote_addr) => println!(
                "Received request for block range {from}-{to} on chain {chain_id} from {remote_addr}"
            ),
            None => println!("Received request for block range {from}-{to} on chain {chain_id}",),
        }

        let db = self.db.clone();
        for result in db.iterate_raw(chain_id, from) {
            match result {
                Ok((number, block)) => {
                    if number > to {
                        break;
                    }
                    let encoded_block = proto_rpc::EncodedBlock {
                        data: block.into_vec(),
                    };
                    if tx.send(Ok(encoded_block)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    // try to send the error
                    // because we always stop afterwards, we can ignore the result
                    let _ = tx.send(Err(tonic::Status::internal(e.to_string()))).await;
                    break;
                }
            }
        }

        Ok(tonic::Response::new(ReceiverStream::new(rx)))
    }
}

#[cfg(test)]
mod tests {

    use std::vec;

    use mockall::predicate::eq;
    use tokio_stream::StreamExt;
    use tonic::Request;

    use super::*;
    use crate::{
        Error,
        blockdb::MockBlockDb,
        proto_rpc::{BlockRangeRequest, BlockRequest, EncodedBlock, block_rpc_server::BlockRpc},
        rpc_client::RpcClient,
        rpc_test_utils::SERVER_STARTUP_TIMER,
    };

    #[tokio::test]
    async fn get_block_returns_raw_data_for_single_block() {
        let mut db = MockBlockDb::new();
        db.expect_get_raw()
            .with(eq(1), eq(2))
            .returning(|_, _| Ok(Some(vec![1, 2, 3, 4])));
        let server = RpcServer::new(db);

        // Existing block
        {
            let req = Request::new(BlockRequest {
                chain_id: 1,
                number: 2,
            });

            let res = server.get_block(req).await.unwrap();
            let EncodedBlock { data } = res.into_inner();
            assert_eq!(data, vec![1, 2, 3, 4]);
        }
    }

    #[tokio::test]
    async fn get_block_returns_not_found_for_non_existing_block() {
        let mut db = MockBlockDb::new();
        db.expect_get_raw()
            .with(eq(1), eq(123))
            .returning(|_, _| Ok(None));
        let server = RpcServer::new(db);
        let req = Request::new(BlockRequest {
            chain_id: 1,
            number: 123,
        });

        let res = server.get_block(req).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code(), tonic::Code::NotFound,);
    }

    #[tokio::test]
    async fn get_block_forwards_errors() {
        let mut db = MockBlockDb::new();
        db.expect_get_raw()
            .with(eq(1), eq(456))
            .returning(|_, _| Err(Error::StorageLayer("DB error".to_owned())));
        let server = RpcServer::new(db);
        let req = Request::new(BlockRequest {
            chain_id: 1,
            number: 456,
        });

        let res = server.get_block(req).await;
        assert!(res.is_err());
        let error = res.unwrap_err();
        assert_eq!(error.code(), tonic::Code::Internal);
        assert!(error.message().contains("DB error"));
    }

    #[tokio::test]
    async fn get_block_range_returns_stream_of_blocks() {
        let mut db = MockBlockDb::new();
        let data = vec![
            (1, 3, vec![3]),
            (1, 7, vec![7]),
            (1, 8, vec![8]),
            (1, 9, vec![9]),
            (1, 10, vec![10]),
        ];
        db.expect_iterate_raw().with(eq(1), eq(3)).returning({
            let data = data.clone();
            move |_, _| {
                Box::new(
                    data.clone()
                        .into_iter()
                        .map(|(_, number, block)| Ok((number, block.into_boxed_slice()))),
                )
            }
        });

        let server = RpcServer::new(db);

        let request = Request::new(BlockRangeRequest {
            chain_id: 1,
            from: 3,
            to: 9,
        });
        let response = server.get_block_range(request).await;
        assert!(response.is_ok());

        let results = response
            .unwrap()
            .into_inner()
            .collect::<Result<Vec<_>, _>>()
            .await
            .expect("The stream should not yield an error");

        assert_eq!(results.len(), 4);
        let results: Vec<_> = results.into_iter().map(|block| block.data).collect();

        let expected: Vec<_> = data.into_iter().map(|v| v.2).collect();
        assert_eq!(results, expected[0..4]); // last element not included
    }

    #[tokio::test]
    async fn get_block_range_returns_error_for_invalid_range() {
        // From greater than To
        let db = MockBlockDb::new();
        let server = RpcServer::new(db);
        let request = Request::new(BlockRangeRequest {
            chain_id: 1,
            from: 10,
            to: 5,
        });
        let response = server.get_block_range(request).await;
        assert!(response.is_err());
        assert_eq!(response.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn get_block_range_forwards_errors() {
        let mut db = MockBlockDb::new();
        db.expect_iterate_raw()
            .with(eq(1), eq(0))
            .returning(|_, _| {
                Box::new(std::iter::once(Err(Error::StorageLayer(
                    "DB error".to_owned(),
                ))))
            });

        let server = RpcServer::new(db);
        let request = Request::new(BlockRangeRequest {
            chain_id: 1,
            from: 0,
            to: 10,
        });
        let response = server.get_block_range(request).await;
        let mut response = response.unwrap().into_inner();

        let error = response.next().await.unwrap().unwrap_err();
        assert_eq!(error.code(), tonic::Code::Internal);
        assert!(error.message().contains("DB error"));

        assert!(
            response.next().await.is_none(),
            "No more items should be in the stream after an error"
        );
    }

    #[tokio::test]
    async fn serve_starts_server_on_specified_port() {
        let mut db = MockBlockDb::new();
        db.expect_get_raw()
            .with(eq(1), eq(1))
            .returning(|_, _| Ok(Some(vec![1, 2, 3])));

        let server = RpcServer::new(db);
        let job = tokio::spawn(async move {
            let _ = server.serve(8081).await;
            ()
        });

        // Wait for the server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(SERVER_STARTUP_TIMER)).await;

        let client = RpcClient::try_new("http://[::1]:8081".parse().unwrap()).await;
        assert!(client.is_ok());
        let mut client = client.unwrap();
        let res = client.get_block(1, 1).await.expect("Block should be found");
        assert_eq!(res.data, vec![1, 2, 3]);
        job.abort(); // Stop the server
    }

    #[tokio::test]
    async fn serve_returns_error_if_binding_to_port_fails() {
        let db = MockBlockDb::new();
        let server = RpcServer::new(db);

        // Reserved port leads to Transport error
        let res = server.serve(80).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("transport error"));
    }
}
