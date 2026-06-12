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

use std::sync::Arc;

use tokio_stream::wrappers::ReceiverStream;
use tonic::{service::interceptor::InterceptedService, transport::Server};

use crate::{
    config::Config,
    db::{BlockDb, IterationDirection},
    grpc::{
        GRPC_COMPRESSION_ALGORITHM, auth,
        proto_rpc::{
            BlockRangeRequest, ChainRange, ChainRanges, EncodedBlock, ListRequest, Metadata,
            MetadataRequest,
            block_rpc_server::{BlockRpc, BlockRpcServer},
        },
    },
};

// TODO: Benchmark this to determine optimal size (#78)
const STREAMING_RESPONSE_CHANNEL_SIZE: usize = 1000;

/// A gRPC server that provides access to block data stored in a database.
#[derive(Debug)]
pub struct RpcServer<Db: BlockDb + Send + Sync + 'static> {
    db: Arc<Db>,
    cfg: Config,
}

impl<Db> RpcServer<Db>
where
    Db: BlockDb + Send + Sync + 'static,
{
    /// Creates a new [RpcServer] instance with the provided database.
    pub fn new(db: Arc<Db>, cfg: Config) -> Self {
        RpcServer { db, cfg }
    }

    /// Starts the gRPC server on the specified port.
    pub async fn serve(
        self,
        listener: tokio::net::TcpListener,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Listening on {}...", listener.local_addr()?);

        let auth_token = self.cfg.get_auth_token().cloned();
        let block_server = BlockRpcServer::new(self).send_compressed(GRPC_COMPRESSION_ALGORITHM);
        let authenticated_block_server =
            InterceptedService::new(block_server, auth::check_token(auth_token));

        Server::builder()
            .add_service(authenticated_block_server)
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
        let (tx, rx) = tokio::sync::mpsc::channel(STREAMING_RESPONSE_CHANNEL_SIZE);

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
            for result in db.iterate_bytes(chain_id, from, IterationDirection::Forward) {
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
                                block_ranges: ranges.into_iter().map(From::from).collect(),
                            })
                    })
                    .collect()
            });

        match ranges {
            Ok(chain_ranges) => Ok(tonic::Response::new(ChainRanges { chain_ranges })),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }

    /// Returns the upgrade heights and corrections metadata for a given chain ID,
    /// if any are stored in the database.
    async fn get_metadata(
        &self,
        request: tonic::Request<MetadataRequest>,
    ) -> Result<tonic::Response<Metadata>, tonic::Status> {
        let remote_addr = request.remote_addr();
        let chain_id = request.into_inner().chain_id;

        match remote_addr {
            Some(addr) => {
                println!("Received metadata request for chain ID {chain_id} from {addr}");
            }
            None => println!("Received metadata request for chain ID {chain_id}"),
        }

        let upgrade_heights = self
            .db
            .get_upgrade_heights(chain_id)
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        let corrections = self
            .db
            .get_corrections(chain_id)
            .map_err(|e| tonic::Status::internal(e.to_string()))?;

        Ok(tonic::Response::new(Metadata {
            upgrade_heights,
            corrections,
        }))
    }
}

#[cfg(test)]
mod tests {

    use std::vec;

    use mockall::predicate::eq;
    use tokio_stream::StreamExt;
    use tonic::{Code, Request};

    use super::*;
    use crate::{
        Error,
        app_dir::{init_app_dir, open_app_dir},
        db::MockBlockDb,
        grpc::{
            auth,
            client::RpcClient,
            proto_rpc::{self, BlockRangeRequest, MetadataRequest, block_rpc_server::BlockRpc},
        },
        utils::test_dir::{Permissions, TestDir},
    };

    #[tokio::test]
    async fn get_block_range_returns_stream_of_blocks() {
        let mut db = MockBlockDb::new();
        // Request more than the buffer size to check that the channel works
        // properly (is filled/consumed asynchronously and does not block)
        let request_count = STREAMING_RESPONSE_CHANNEL_SIZE + 10;
        let mut data = vec![];
        for i in 1..=request_count {
            data.push((i as u64, vec![i as u8]));
        }
        db.expect_iterate_bytes()
            .with(eq(1), eq(1), eq(IterationDirection::Forward))
            .returning({
                let data = data.clone();
                move |_, _, _| {
                    Box::new(
                        data.clone()
                            .into_iter()
                            .map(|(number, block)| Ok((number, block.into_boxed_slice()))),
                    )
                }
            });

        let server = RpcServer::new(Arc::new(db), Config::default());

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
        let server = RpcServer::new(Arc::new(db), Config::default());
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
        db.expect_iterate_bytes()
            .with(eq(1), eq(0), eq(IterationDirection::Forward))
            .returning(|_, _, _| {
                Box::new(std::iter::once(Err(Error::StorageLayer(
                    "DB error".to_owned(),
                ))))
            });

        let server = RpcServer::new(Arc::new(db), Config::default());
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
        db.expect_iterate_bytes()
            .with(eq(1), eq(1), eq(IterationDirection::Forward))
            .returning(|_, _, _| {
                Box::new(vec![Ok((1, vec![1, 2, 3].into_boxed_slice()))].into_iter())
            });

        let config = Config::default();
        let server = RpcServer::new(Arc::new(db), config.clone());
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let job = tokio::spawn(async move {
            server.serve(listener).await.expect("Server should start");
        });

        let client = RpcClient::try_new(
            format!("http://{addr}").parse().unwrap(),
            config.get_auth_token().cloned(),
        )
        .await;
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
    async fn serve_authenticates_user_if_token_specified() {
        let mut db = MockBlockDb::new();
        db.expect_iterate_bytes()
            .with(eq(1), eq(1), eq(IterationDirection::Forward))
            .returning(|_, _, _| {
                Box::new(vec![Ok((1, vec![1, 2, 3].into_boxed_slice()))].into_iter())
            });
        let db = Arc::new(db);

        let auth_token = Some("xyz");
        let auth_token = auth_token
            .map(auth::token_to_metadata_value)
            .transpose()
            .unwrap();

        // request without token should fail
        {
            let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
            init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
            let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();
            cfg.set_auth_token(auth_token.clone()).unwrap();

            let server = RpcServer::new(db.clone(), cfg);
            let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let job = tokio::spawn(async move {
                server.serve(listener).await.expect("Server should start");
            });

            let client = RpcClient::try_new(format!("http://{addr}").parse().unwrap(), None).await;
            assert!(client.is_ok());
            let mut client = client.unwrap();
            let res = client.get_block_range(1, 1, 1).await;
            assert!(res.is_err());
            let res = res.unwrap_err();
            assert_eq!(res.code(), Code::Unauthenticated);
            assert_eq!(res.message(), "Missing auth token");
            job.abort(); // Stop the server
        }
        // request with token should succeed
        {
            let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
            init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
            let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();
            cfg.set_auth_token(auth_token.clone()).unwrap();

            let server = RpcServer::new(db, cfg);
            let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let job = tokio::spawn(async move {
                server.serve(listener).await.expect("Server should start");
            });

            let client =
                RpcClient::try_new(format!("http://{addr}").parse().unwrap(), auth_token).await;
            assert!(client.is_ok());
            let mut client = client.unwrap();
            let res = client.get_block_range(1, 1, 1).await;
            assert!(res.is_ok());
            assert_eq!(
                res.unwrap()
                    .next()
                    .await
                    .expect("stream should not be empty")
                    .expect("not an error")
                    .data,
                vec![1, 2, 3]
            );
            job.abort(); // Stop the server
        }
    }

    #[tokio::test]
    async fn list_returns_ranges_for_chains() {
        // single chain ID
        {
            let mut db = MockBlockDb::new();
            db.expect_get_ranges_of_chain_id()
                .with(eq(1))
                .returning(|_| Ok(vec![1..=2, 3..=4]));
            let server = RpcServer::new(Arc::new(db), Config::default());

            let req = Request::new(ListRequest { chain_id: Some(1) });
            let res = server.list(req).await.unwrap();
            let chain_ranges = res.into_inner().chain_ranges;
            assert_eq!(
                chain_ranges,
                vec![ChainRange {
                    chain_id: 1,
                    block_ranges: vec![
                        proto_rpc::BlockRange { from: 1, to: 2 },
                        proto_rpc::BlockRange { from: 3, to: 4 }
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
            let server = RpcServer::new(Arc::new(db), Config::default());

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
                .returning(|_| Ok(vec![1..=2, 3..=4]));
            db.expect_get_ranges_of_chain_id()
                .with(eq(2))
                .returning(|_| Ok(vec![5..=6]));
            let server = RpcServer::new(Arc::new(db), Config::default());

            let req = Request::new(ListRequest { chain_id: None });
            let res = server.list(req).await.unwrap();
            let chain_ranges = res.into_inner().chain_ranges;
            assert_eq!(
                chain_ranges,
                vec![
                    ChainRange {
                        chain_id: 1,
                        block_ranges: vec![
                            proto_rpc::BlockRange { from: 1, to: 2 },
                            proto_rpc::BlockRange { from: 3, to: 4 }
                        ]
                    },
                    ChainRange {
                        chain_id: 2,
                        block_ranges: vec![proto_rpc::BlockRange { from: 5, to: 6 }]
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
        let server = RpcServer::new(Arc::new(db), Config::default());
        let req = Request::new(ListRequest { chain_id: None });

        let res = server.list(req).await;
        assert!(res.is_err());
        let error = res.unwrap_err();
        assert_eq!(error.code(), tonic::Code::Internal);
        assert!(error.message().contains("DB error"));
    }

    #[rstest::rstest]
    #[case::no_metadata(None, None)]
    #[case::only_upgrade_heights(Some(b"upgrade-heights".to_vec()), None)]
    #[case::only_corrections(None, Some(b"corrections".to_vec()))]
    #[case::both_metadata(Some(b"upgrade-heights".to_vec()), Some(b"corrections".to_vec()))]
    #[tokio::test]
    async fn get_metadata_returns_expected_data(
        #[case] upgrade_heights: Option<Vec<u8>>,
        #[case] corrections: Option<Vec<u8>>,
    ) {
        let chain_id = 1;
        let mut db = MockBlockDb::new();
        let expected_upgrade_heights = upgrade_heights.clone();
        let expected_corrections = corrections.clone();
        db.expect_get_upgrade_heights()
            .with(eq(chain_id))
            .returning(move |_| Ok(upgrade_heights.clone()));
        db.expect_get_corrections()
            .with(eq(chain_id))
            .returning(move |_| Ok(corrections.clone()));

        let server = RpcServer::new(Arc::new(db), Config::default());

        let req = Request::new(MetadataRequest { chain_id });
        let res = server.get_metadata(req).await.unwrap();
        let metadata = res.into_inner();
        assert_eq!(metadata.upgrade_heights, expected_upgrade_heights);
        assert_eq!(metadata.corrections, expected_corrections);
    }

    #[tokio::test]
    async fn get_metadata_forwards_db_errors() {
        let chain_id = 1;
        let mut db = MockBlockDb::new();
        db.expect_get_upgrade_heights()
            .with(eq(chain_id))
            .returning(|_| Err(Error::StorageLayer("DB error".to_owned())));

        let server = RpcServer::new(Arc::new(db), Config::default());
        let req = Request::new(MetadataRequest { chain_id });
        let res = server.get_metadata(req).await;
        let err = res.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Internal);
        assert!(err.message().contains("DB error"));
    }
}
