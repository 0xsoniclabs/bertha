// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use std::{
    path::Path,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use tokio::task::JoinSet;
use tokio_stream::{StreamExt, StreamMap};
use tokio_util::sync::CancellationToken;

use crate::{
    app_dir::open_app_dir,
    config::ChainConfig,
    db::{BlockDb, IterationDirection},
    grpc::RpcServer,
    json_rpc::{NetworkSource, subscribe_to_blocks},
};

const CATCH_UP_INTERVAL: Duration = Duration::from_secs(1);

/// Starts the block service server.
///
/// The `_test_notify_tasks_spawned` parameter is used in tests to notify when the internal async
/// tasks have been spawned; non-test code should simply pass [None] here.
pub async fn start(
    app_dir: impl AsRef<Path>,
    listener: tokio::net::TcpListener,
    cancellation_token: CancellationToken,
    _test_notify: Option<tokio::sync::mpsc::Sender<StartCmdStatusMsg>>,
    _test_sync_block_count: Option<Arc<AtomicU64>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app_dir = app_dir.as_ref();
    let (cfg, mut write_db) = open_app_dir(app_dir, false)?;
    let (_, read_db) = open_app_dir(app_dir, true)?;
    let read_db = Arc::new(read_db);

    let mut join_set = JoinSet::new();

    // Spawn task to sync blocks from the JSON-RPC servers.
    join_set.spawn({
        let cancellation_token = cancellation_token.clone();
        let chain_configs = cfg.get_chain_configs().to_owned();
        let _test_notify = _test_notify.clone();
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => Ok(()),
                // NOTE: Even though `sync` internally spawns another task (via `subscribe_to_blocks`),
                // we don't have to pass a cancellation token to it, as the task will exit once
                // the read-end of the stream is closed.
                r = sync(&chain_configs, &mut write_db, _test_sync_block_count) => r
            }
        }
    });

    // Spawn task to sync the secondary db with the primary db.
    join_set.spawn({
        let cancellation_token = cancellation_token.clone();
        let read_db = Arc::clone(&read_db);
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => Ok(()),
                r = async {
                    loop {
                        read_db.try_catch_up_with_primary()?;
                        tokio::time::sleep(CATCH_UP_INTERVAL).await;
                    }
                } => r
            }
        }
    });

    // Spawn task to run the RPC server.
    join_set.spawn({
        let cancellation_token = cancellation_token.clone();
        let read_db = Arc::clone(&read_db);
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => Ok(()),
                r = {
                    let server = RpcServer::new(read_db, cfg);
                    server.serve(listener)
                } => r
            }
        }
    });

    #[cfg(test)]
    {
        if let Some(notify) = _test_notify {
            // Notify the test that the task has been spawned
            notify.send(StartCmdStatusMsg::SyncStarted).await.unwrap();
        }
    }

    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(Ok(())) => (),
            Ok(Err(e)) => {
                cancellation_token.cancel();
                return Err(e);
            }
            Err(join_err) => {
                cancellation_token.cancel();
                return Err(join_err.into());
            }
        }
    }

    Ok(())
}

async fn sync(
    chain_configs: &[ChainConfig],
    db: &mut impl BlockDb,
    _test_num_blocks_written: Option<Arc<AtomicU64>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut streams = StreamMap::new();
    for chain_config in chain_configs {
        let Some(server) = chain_config.json_rpc.as_ref() else {
            continue;
        };
        let start_block = db
            .iterate(chain_config.id, u64::MAX, IterationDirection::Reverse)
            .next()
            .transpose()?
            .map(|(_, block)| block.number + 1)
            .unwrap_or_default();

        let source = NetworkSource::try_new(server)?;
        let block_stream = subscribe_to_blocks(start_block, source);
        streams.insert(chain_config.id, block_stream);
    }

    while let Some((chain_id, block)) = streams.next().await {
        let block = block?;
        // If insertion should become a bottleneck, we can consider batching multiple blocks
        // together before writing to the db.
        db.put(chain_id, block)?;
        #[cfg(test)]
        {
            if let Some(num_blocks_written) = &_test_num_blocks_written {
                num_blocks_written.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    Ok(())
}

/// A message send over the channel supplied to the start command to notify about the status of the
/// start command.
/// Messages of this type are sent over the `_test_notify` channel of the start command.
#[allow(clippy::enum_variant_names)]
pub enum StartCmdStatusMsg {
    SyncStarted,
    SyncFinished,
    SyncError(String),
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    use bertha_types::{Block, BlockHeader, HexConvert, TransactionReceipt};
    use prost::Message;
    use tokio::time::Instant;
    use tonic::metadata::{Ascii, MetadataValue};
    use wiremock::MockServer;

    use super::*;
    use crate::{
        app_dir::{init_app_dir, open_app_dir},
        cmd::start,
        db::{BlockDb, proto},
        grpc::{RpcClient, auth},
        json_rpc::{
            BlockHeaderWithTransactionsAndWithdrawals,
            test_utils::build_mock_server_request_handler_for_infinitely_many_requests,
        },
        test_templates::auth_token,
        utils::test_dir::{Permissions, TestDir},
    };

    #[rstest_reuse::apply(auth_token)]
    #[tokio::test]
    async fn starts_server_successfully_and_forwards_auth_token(
        auth_token: Option<MetadataValue<Ascii>>,
    ) {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        {
            let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.put_bytes(1, 1, &[1, 2, 3]).unwrap();
        }

        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let token = CancellationToken::new();
        let task = tokio::spawn({
            let token = token.clone();
            async move {
                start(tmpdir.path(), listener, token, None, None)
                    .await
                    .unwrap();
            }
        });

        let client =
            RpcClient::try_new(format!("http://{addr}").parse().unwrap(), auth_token).await;
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
        token.cancel();
        task.abort(); // Stop the server
    }

    #[tokio::test]
    async fn start_starts_sync_and_rpc_clients_can_query_synchronized_blocks() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mock_server = MockServer::start().await;

        let chain_id = 1;
        let block_number: u64 = 0;
        let block_header = BlockHeader::default();
        let transactions = Vec::new();
        let withdrawals = Vec::new();
        let block_receipts = vec![TransactionReceipt::default()];
        let block_header_with_transactions = BlockHeaderWithTransactionsAndWithdrawals {
            block_header: block_header.clone(),
            transactions: transactions.clone(),
            withdrawals: withdrawals.clone(),
        };

        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockByNumber",
                    vec![
                        serde_json::to_value(block_number.to_hex()).unwrap(),
                        serde_json::to_value(true).unwrap(),
                    ],
                    block_header_with_transactions.clone(),
                ),
            )
            .await;
        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockReceipts",
                    vec![serde_json::to_value(block_number.to_hex()).unwrap()],
                    block_receipts.clone(),
                ),
            )
            .await;

        // Register a mock for block 1 that returns null, so the sync task retries
        // (interprets it as NotFound) instead of failing with a 404.
        let next_block_number: u64 = 1;
        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockByNumber",
                    vec![
                        serde_json::to_value(next_block_number.to_hex()).unwrap(),
                        serde_json::to_value(true).unwrap(),
                    ],
                    None::<()>,
                ),
            )
            .await;

        let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();

        let chain_config = ChainConfig {
            json_rpc: Some(mock_server.uri()),
            ..ChainConfig::new(chain_id)
        };

        cfg.add_chain(chain_config).unwrap();

        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token = CancellationToken::new();
        let sync_block_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let task = tokio::spawn({
            let token = token.clone();
            let sync_block_count = sync_block_count.clone();
            async move {
                start(tmpdir.path(), listener, token, None, Some(sync_block_count))
                    .await
                    .unwrap();
            }
        });
        // wait for the sync task to fetch the header, transactions and receipts for the first block
        let start = Instant::now();
        while sync_block_count.load(std::sync::atomic::Ordering::Relaxed) < 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if start.elapsed() >= Duration::from_secs(1) {
                panic!("sync task did not fetch block within 1 second");
            }
        }

        let mut client = RpcClient::try_new(format!("http://{addr}").parse().unwrap(), None)
            .await
            .unwrap();
        let start = Instant::now();
        let block = loop {
            let mut blocks = client
                .get_block_range(chain_id, block_number, block_number)
                .await
                .unwrap();
            if let Some(block) = blocks.next().await {
                break block.unwrap();
            }
            if start.elapsed() >= 2 * CATCH_UP_INTERVAL {
                panic!("RPC server did not return block within 2*CATCH_UP_INTERVAL");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        };
        let block = Block::try_from(proto::Block::decode(block.data.as_slice()).unwrap()).unwrap();
        assert_eq!(
            block,
            Block::from_parts(block_header, transactions, block_receipts, withdrawals)
        );
        token.cancel();
        task.abort();
    }

    #[tokio::test]
    async fn start_returns_error_if_sync_fails() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        // Configure a chain with an invalid URL so sync fails
        let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();
        cfg.add_chain(ChainConfig {
            json_rpc: Some("invalid_url".to_string()),
            ..ChainConfig::new(1)
        })
        .unwrap();

        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let result = start(
            tmpdir.path(),
            listener,
            CancellationToken::new(),
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn start_allows_internal_tasks_to_be_cancelled() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();

        let metrics = tokio::runtime::Handle::current().metrics();
        let num_tasks_before = metrics.num_alive_tasks();

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let token = CancellationToken::new();
        tokio::spawn({
            let token = token.clone();
            async move {
                start(tmpdir.path(), listener, token, Some(tx), None)
                    .await
                    .unwrap();
            }
        });

        // Wait for internal tasks to be spawned
        loop {
            match rx.recv().await {
                Some(StartCmdStatusMsg::SyncStarted) => break,
                Some(_) => continue,
                _ => panic!("Expected SyncStarted message"),
            }
        }

        let metrics = tokio::runtime::Handle::current().metrics();
        assert!(metrics.num_alive_tasks() > num_tasks_before);
        token.cancel();

        let start = Instant::now();
        while metrics.num_alive_tasks() > num_tasks_before {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if start.elapsed() >= Duration::from_secs(1) {
                panic!("task did not stop after 1 second");
            }
        }
    }

    #[tokio::test]
    async fn start_fails_if_app_dir_is_not_initialized() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let result = start(
            tmpdir.path(),
            listener,
            CancellationToken::new(),
            None,
            None,
        )
        .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(&format!(
            "no blockservice.toml found at {} - did you forget to run init?",
            tmpdir.path().display()
        )));
    }

    #[tokio::test]
    async fn sync_fails_if_server_url_is_invalid() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), false).unwrap();

        let chain_configs = vec![ChainConfig {
            json_rpc: Some("invalid_url".to_string()),
            ..ChainConfig::new(1)
        }];

        let result = sync(&chain_configs, &mut db, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn sync_forwards_db_error() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, mut db) = open_app_dir(tmpdir.path(), true).unwrap();

        let mock_server = MockServer::start().await;

        let block_number: u64 = 0;
        let block_receipts = vec![TransactionReceipt::default()];
        let block_header_with_transactions = BlockHeaderWithTransactionsAndWithdrawals {
            block_header: BlockHeader::default(),
            transactions: Vec::new(),
            withdrawals: Vec::new(),
        };

        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockByNumber",
                    vec![
                        serde_json::to_value(block_number.to_hex()).unwrap(),
                        serde_json::to_value(true).unwrap(),
                    ],
                    block_header_with_transactions.clone(),
                ),
            )
            .await;
        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockReceipts",
                    vec![serde_json::to_value(block_number.to_hex()).unwrap()],
                    block_receipts.clone(),
                ),
            )
            .await;

        let chain_configs = vec![ChainConfig {
            json_rpc: Some(mock_server.uri()),
            ..ChainConfig::new(1)
        }];

        let result = sync(&chain_configs, &mut db, None).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Not supported operation in secondary mode.")
        );
    }

    #[tokio::test]
    async fn sync_fetches_blocks_and_stores_them_in_db() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();

        let mock_server = MockServer::start().await;

        let block_number: u64 = 0;
        let block_receipts = vec![TransactionReceipt::default()];
        let block_header_with_transactions = BlockHeaderWithTransactionsAndWithdrawals {
            block_header: BlockHeader::default(),
            transactions: Vec::new(),
            withdrawals: Vec::new(),
        };

        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockByNumber",
                    vec![
                        serde_json::to_value(block_number.to_hex()).unwrap(),
                        serde_json::to_value(true).unwrap(),
                    ],
                    block_header_with_transactions.clone(),
                ),
            )
            .await;
        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockReceipts",
                    vec![serde_json::to_value(block_number.to_hex()).unwrap()],
                    block_receipts.clone(),
                ),
            )
            .await;

        // Register a mock for block 1 that returns null, so the sync task retries
        // (interprets it as NotFound) instead of failing with a 404.
        let next_block_number: u64 = 1;
        mock_server
            .register(
                build_mock_server_request_handler_for_infinitely_many_requests(
                    "eth_getBlockByNumber",
                    vec![
                        serde_json::to_value(next_block_number.to_hex()).unwrap(),
                        serde_json::to_value(true).unwrap(),
                    ],
                    None::<()>,
                ),
            )
            .await;

        let chain_configs = vec![
            ChainConfig {
                json_rpc: Some(mock_server.uri()),
                ..ChainConfig::new(1)
            },
            ChainConfig {
                json_rpc: Some(mock_server.uri()),
                ..ChainConfig::new(2)
            },
            ChainConfig {
                json_rpc: None, // this chain will be ignored
                ..ChainConfig::new(2)
            },
        ];

        let block_count = Arc::new(AtomicU64::new(0));
        let task = tokio::spawn({
            let (_, mut write_db) = open_app_dir(tmpdir.path(), false).unwrap();
            let block_count = block_count.clone();
            let chain_configs = chain_configs.clone();
            async move {
                sync(&chain_configs, &mut write_db, Some(block_count))
                    .await
                    .unwrap();
            }
        });
        // wait for the sync task to fetch the header, transactions and receipts for the first block
        while block_count.load(Ordering::Relaxed) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        task.abort();
        let _ = task.await; // wait for the task to finish so the write db is dropped

        // check that block 0 for chain 1 and 2 is in the db
        let (_, read_db) = open_app_dir(tmpdir.path(), false).unwrap();
        let block = read_db.get(1, 0);
        assert!(block.is_ok_and(|b| b.is_some()));
        let block = read_db.get(2, 0);
        assert!(block.is_ok_and(|b| b.is_some()));
    }
}
