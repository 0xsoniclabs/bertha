use std::sync::Arc;

use tokio_stream::wrappers::ReceiverStream;
use tonic::{codec::CompressionEncoding, transport::Server};

use crate::{
    db::BlockDb,
    grpc::proto_rpc::{
        BlockRange, BlockRangeRequest, ChainRange, ChainRanges, EncodedBlock, ListRequest,
        block_rpc_server::{BlockRpc, BlockRpcServer},
    },
};

// TODO: Benchmark this to determine optimal size (#78)
const SERVER_RESPONSE_BUFFER_SIZE: usize = 1000;

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
    pub async fn serve(
        self,
        listener: tokio::net::TcpListener,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("Listening on {}...", listener.local_addr()?);

        Server::builder()
            .add_service(BlockRpcServer::new(self).send_compressed(CompressionEncoding::Gzip))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await?;
        Ok(())
    }
}

#[tonic::async_trait]
impl<Db> BlockRpc for RpcServer<Db>
where
    Db: BlockDb + Send + Sync + 'static,
{
    type GetBlockRangeStream = ReceiverStream<Result<EncodedBlock, tonic::Status>>;

    /// Returns a stream of blocks in the specified range for a given chain ID.
    async fn get_block_range(
        &self,
        request: tonic::Request<BlockRangeRequest>,
    ) -> Result<tonic::Response<Self::GetBlockRangeStream>, tonic::Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(SERVER_RESPONSE_BUFFER_SIZE);

        let remote_addr = request.remote_addr();
        let BlockRangeRequest { chain_id, from, to } = request.into_inner();

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
        tokio::spawn(async move {
            for result in db.iterate_raw(chain_id, from) {
                match result {
                    Ok((number, block)) => {
                        if number > to {
                            break;
                        }
                        let encoded_block = EncodedBlock {
                            data: block.into_vec(),
                            number,
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
        });

        Ok(tonic::Response::new(ReceiverStream::new(rx)))
    }

    async fn list(
        &self,
        request: tonic::Request<ListRequest>,
    ) -> Result<tonic::Response<ChainRanges>, tonic::Status> {
        let remote_addr = request.remote_addr();
        let chain_id = request.into_inner().chain_id;

        match (remote_addr, chain_id) {
            (Some(addr), Some(chain_id)) => {
                println!("Received list request for chain ID {chain_id} from {addr}");
            }
            (Some(addr), None) => println!("Received list request for all chain IDs from {addr}"),
            (None, Some(chain_id)) => println!("Received list request for chain ID {chain_id}"),
            (None, None) => println!("Received list request for all chains IDs"),
        }

        let ranges = chain_id
            .map(|id| Ok(vec![id]))
            .unwrap_or_else(|| self.db.get_chain_ids())
            .and_then(|chain_ids| {
                chain_ids
                    .into_iter()
                    .map(|chain_id| {
                        self.db
                            .get_ranges_of_chain_id(chain_id)
                            .map(|ranges| ChainRange {
                                chain_id,
                                block_ranges: ranges
                                    .into_iter()
                                    .map(|(from, to)| BlockRange { from, to })
                                    .collect(),
                            })
                    })
                    .collect()
            });

        match ranges {
            Ok(chain_ranges) => Ok(tonic::Response::new(ChainRanges { chain_ranges })),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
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
        db::MockBlockDb,
        grpc::{
            client::RpcClient,
            proto_rpc::{BlockRangeRequest, block_rpc_server::BlockRpc},
        },
    };

    #[tokio::test]
    async fn get_block_range_returns_stream_of_blocks() {
        let mut db = MockBlockDb::new();
        // Request more than the buffer size to check that the channel works
        // properly (is filled/consumed asynchronously and does not block)
        let request_count = SERVER_RESPONSE_BUFFER_SIZE + 10;
        let mut data = vec![];
        for i in 1..=request_count {
            data.push((i as u64, vec![i as u8]));
        }
        db.expect_iterate_raw().with(eq(1), eq(1)).returning({
            let data = data.clone();
            move |_, _| {
                Box::new(
                    data.clone()
                        .into_iter()
                        .map(|(number, block)| Ok((number, block.into_boxed_slice()))),
                )
            }
        });

        let server = RpcServer::new(db);

        let request = Request::new(BlockRangeRequest {
            chain_id: 1,
            from: 1,
            to: request_count as u64,
        });
        let response = server.get_block_range(request).await;
        assert!(response.is_ok());

        let results = response
            .unwrap()
            .into_inner()
            .collect::<Result<Vec<_>, _>>()
            .await
            .expect("The stream should not yield an error");

        assert_eq!(results.len(), request_count);
        let results: Vec<_> = results.into_iter().map(|block| block.data).collect();

        let expected: Vec<_> = data.into_iter().map(|v| v.1).collect();
        assert_eq!(results, expected[..request_count]); // last element not included
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
    async fn serve_starts_server_on_specified_listener() {
        let mut db = MockBlockDb::new();
        db.expect_iterate_raw().with(eq(1), eq(1)).returning({
            move |_, _| Box::new(vec![Ok((1, vec![1, 2, 3].into_boxed_slice()))].into_iter())
        });

        let server = RpcServer::new(db);
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let job = tokio::spawn(async move {
            server.serve(listener).await.expect("Server should start");
        });

        let client = RpcClient::try_new(format!("http://{addr}").parse().unwrap()).await;
        assert!(client.is_ok());
        let mut client = client.unwrap();
        let mut res = client.get_block_range(1, 1, 1).await.unwrap();
        assert_eq!(
            res.next()
                .await
                .expect("stream should not be empty")
                .expect("not an error")
                .data,
            vec![1, 2, 3]
        );
        job.abort(); // Stop the server
    }

    #[tokio::test]
    async fn list_returns_ranges_for_chains() {
        // single chain ID
        {
            let mut db = MockBlockDb::new();
            db.expect_get_ranges_of_chain_id()
                .with(eq(1))
                .returning(|_| Ok(vec![(1, 2), (3, 4)]));
            let server = RpcServer::new(db);

            let req = Request::new(ListRequest { chain_id: Some(1) });
            let res = server.list(req).await.unwrap();
            let chain_ranges = res.into_inner().chain_ranges;
            assert_eq!(
                chain_ranges,
                vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![
                        BlockRange { from: 1, to: 2 },
                        BlockRange { from: 3, to: 4 }
                    ]
                }]
            );
        }
        // non-existing chain ID
        {
            let mut db = MockBlockDb::new();
            db.expect_get_ranges_of_chain_id()
                .with(eq(1))
                .returning(|_| Ok(vec![]));
            let server = RpcServer::new(db);

            let req = Request::new(ListRequest { chain_id: Some(1) });
            let res = server.list(req).await.unwrap();
            let chain_ranges = res.into_inner().chain_ranges;
            assert_eq!(
                chain_ranges,
                vec![ChainRange {
                    chain_id: 1,
                    block_ranges: Vec::new()
                }]
            );
        }
        // all chain ID
        {
            let mut db = MockBlockDb::new();
            db.expect_get_chain_ids().returning(|| Ok(vec![1, 2]));
            db.expect_get_ranges_of_chain_id()
                .with(eq(1))
                .returning(|_| Ok(vec![(1, 2), (3, 4)]));
            db.expect_get_ranges_of_chain_id()
                .with(eq(2))
                .returning(|_| Ok(vec![(5, 6)]));
            let server = RpcServer::new(db);

            let req = Request::new(ListRequest { chain_id: None });
            let res = server.list(req).await.unwrap();
            let chain_ranges = res.into_inner().chain_ranges;
            assert_eq!(
                chain_ranges,
                vec![
                    ChainRange {
                        chain_id: 1,
                        block_ranges: vec![
                            BlockRange { from: 1, to: 2 },
                            BlockRange { from: 3, to: 4 }
                        ]
                    },
                    ChainRange {
                        chain_id: 2,
                        block_ranges: vec![BlockRange { from: 5, to: 6 }]
                    }
                ]
            );
        }
    }

    #[tokio::test]
    async fn list_forwards_errors() {
        let mut db = MockBlockDb::new();
        db.expect_get_chain_ids()
            .returning(|| Err(Error::StorageLayer("DB error".to_owned())));
        let server = RpcServer::new(db);
        let req = Request::new(ListRequest { chain_id: None });

        let res = server.list(req).await;
        assert!(res.is_err());
        let error = res.unwrap_err();
        assert_eq!(error.code(), tonic::Code::Internal);
        assert!(error.message().contains("DB error"));
    }
}
