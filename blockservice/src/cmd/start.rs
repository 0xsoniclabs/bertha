use std::{
    ops::Deref,
    path::Path,
    sync::{Arc, atomic::AtomicU64},
};

use tokio_stream::{StreamExt, StreamMap};
use tokio_util::sync::CancellationToken;

use crate::{
    app_dir::open_app_dir,
    config::ChainConfig,
    db::BlockDb,
    grpc::RpcServer,
    json_rpc::{NetworkSource, subscribe_to_blocks},
};

/// Starts the block service server.
///
/// The `_test_notify_tasks_spawned` parameter is used in tests to notify when the internal async
/// tasks have been spawned; non-test code should simply pass [None] here.
pub async fn start(
    app_dir: impl AsRef<Path>,
    listener: tokio::net::TcpListener,
    cancellation_token: CancellationToken,
    _test_notify_tasks_spawned: Option<Arc<tokio::sync::Notify>>,
    _test_sync_block_count: Option<Arc<AtomicU64>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (cfg, db) = open_app_dir(app_dir, false)?;
    // Put the db in an Arc to share it between multiple tasks
    let db = Arc::new(db);

    // NOTE: This is a dummy task used to demonstrate graceful shutdown via cancellation token.
    // TODO: Replace with telemetry task (#66)
    tokio::spawn({
        let cancellation_token = cancellation_token.clone();
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {},
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {},
            }
        }
    });

    // Spawn task to sync blocks from the JSON-RPC servers.
    tokio::spawn({
        let cancellation_token = cancellation_token.clone();
        let db = Arc::clone(&db);
        let chain_configs = cfg.get_chain_configs().to_owned();
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {},
                // NOTE: Even though `sync` internally spawns another task (via `subscribe_to_blocks`),
                // we don't have to pass a cancellation token to it, as the task will exit once
                // the read-end of the stream is closed.
                r = sync(&chain_configs, db.deref(), _test_sync_block_count) => {
                    if let Err(err) = r {
                        println!("error in block sync task: {err}");
                    }
                },
            }
        }
    });

    #[cfg(test)]
    {
        if let Some(tasks_spawned) = _test_notify_tasks_spawned {
            // Notify the test that the task has been spawned
            tasks_spawned.notify_one();
        }
    }

    let server = RpcServer::new(db, cfg);
    server.serve(listener).await
}

async fn sync(
    chain_configs: &[ChainConfig],
    db: &impl BlockDb,
    _test_num_blocks_written: Option<Arc<AtomicU64>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut streams = StreamMap::new();
    for chain_config in chain_configs {
        let chain_id = chain_config.id;
        let Some(server) = chain_config.json_rpc.as_ref() else {
            continue;
        };
        let start_block = db
            .iterate_reverse(chain_id, u64::MAX)
            .next()
            .transpose()?
            .map(|block| block.number + 1)
            .unwrap_or_default();

        let source = NetworkSource::try_new(server)?;
        let block_stream = subscribe_to_blocks(start_block, source);
        streams.insert(chain_id, block_stream);
    }

    while let Some((chain_id, block)) = streams.next().await {
        match block {
            Ok(block) => {
                db.put(chain_id, block)?;
                #[cfg(test)]
                {
                    if let Some(num_blocks_written) = &_test_num_blocks_written {
                        num_blocks_written.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
            Err(err) => println!("[chain ID {chain_id}] error fetching next block: {err}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    use bertha_types::{Block, BlockHeader, HexConvert, TransactionReceipt};
    use prost::Message;
    use wiremock::MockServer;

    use super::*;
    use crate::{
        app_dir::{init_app_dir, open_app_dir},
        cmd::start,
        db::{BlockDb, proto},
        grpc::RpcClient,
        json_rpc::{
            BlockHeaderWithTransactions,
            test_utils::build_mock_server_request_handler_for_infinitely_many_requests,
        },
        utils::test_dir::{Permissions, TestDir},
    };

    #[tokio::test]
    async fn start_starts_server_successfully() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        {
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.put_raw(1, 1, vec![1, 2, 3].as_slice()).unwrap();
        }

        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let token = CancellationToken::new();
        let job = tokio::spawn({
            let token = token.clone();
            async move {
                start(tmpdir.path(), listener, token, None, None)
                    .await
                    .unwrap();
            }
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
        token.cancel();
        job.abort(); // Stop the server
    }

    #[tokio::test]
    async fn start_allows_internal_tasks_to_be_cancelled() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();

        let metrics = tokio::runtime::Handle::current().metrics();
        let num_tasks_before = metrics.num_alive_tasks();

        let tasks_spawned = Arc::new(tokio::sync::Notify::new());
        let token = CancellationToken::new();
        let job = tokio::spawn({
            let token = token.clone();
            let tasks_spawned = tasks_spawned.clone();
            async move {
                start(tmpdir.path(), listener, token, Some(tasks_spawned), None)
                    .await
                    .unwrap();
            }
        });

        // Wait for internal tasks to be spawned
        tasks_spawned.notified().await;

        let metrics = tokio::runtime::Handle::current().metrics();
        let num_tasks_now = metrics.num_alive_tasks();
        assert!(num_tasks_now > num_tasks_before);
        // Aborting the local task does not suffice, as we spawn additional tasks internally.
        job.abort();
        job.await.unwrap_err(); // JoinError
        let num_tasks_now = metrics.num_alive_tasks();
        assert!(num_tasks_now > num_tasks_before);
        token.cancel();

        let mut elapsed_time = Duration::from_millis(0);
        while metrics.num_alive_tasks() > num_tasks_before {
            tokio::time::sleep(Duration::from_millis(10)).await;
            elapsed_time += Duration::from_millis(10);
            if elapsed_time >= Duration::from_secs(1) {
                panic!("task did not stop after 1 second");
            }
        }
    }

    #[tokio::test]
    async fn fails_if_app_dir_is_not_initialized() {
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
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();

        let chain_configs = vec![ChainConfig {
            json_rpc: Some("invalid_url".to_string()),
            ..ChainConfig::new(1)
        }];

        let result = sync(&chain_configs, &db, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn sync_forwards_db_error() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();

        init_app_dir(tmpdir.path(), std::io::sink()).unwrap();
        let (_, db) = open_app_dir(tmpdir.path(), true).unwrap();

        let mock_server = MockServer::start().await;

        let block_number: u64 = 0;
        let block_receipts = vec![TransactionReceipt::default()];
        let block_header_with_transactions = BlockHeaderWithTransactions {
            block_header: BlockHeader::default(),
            transactions: Vec::new(),
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
            json_rpc: Some(mock_server.uri().to_string()),
            ..ChainConfig::new(1)
        }];

        let result = sync(&chain_configs, &db, None).await;
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
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
        let db = Arc::new(db);

        let mock_server = MockServer::start().await;

        let block_number: u64 = 0;
        let block_receipts = vec![TransactionReceipt::default()];
        let block_header_with_transactions = BlockHeaderWithTransactions {
            block_header: BlockHeader::default(),
            transactions: Vec::new(),
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

        let chain_configs = vec![
            ChainConfig {
                json_rpc: Some(mock_server.uri().to_string()),
                ..ChainConfig::new(1)
            },
            ChainConfig {
                json_rpc: Some(mock_server.uri().to_string()),
                ..ChainConfig::new(2)
            },
        ];

        let block_count = Arc::new(AtomicU64::new(0));
        let task = tokio::spawn({
            let db = Arc::clone(&db);
            let block_count = block_count.clone();
            async move {
                sync(&chain_configs, db.deref(), Some(block_count))
                    .await
                    .unwrap();
            }
        });
        // wait for the sync task to fetch the header, transactions and receipts for the first block
        while block_count.load(Ordering::Relaxed) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // check that block 0 for chain 1 and 2 is in the db
        let block = db.get(1, 0);
        assert!(block.is_ok_and(|b| b.is_some()));
        let block = db.get(2, 0);
        assert!(block.is_ok_and(|b| b.is_some()));
        task.abort();
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
        let block_receipts = vec![TransactionReceipt::default()];
        let block_header_with_transactions = BlockHeaderWithTransactions {
            block_header: block_header.clone(),
            transactions: transactions.clone(),
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

        let (mut cfg, _) = open_app_dir(tmpdir.path(), true).unwrap();

        let chain_config = ChainConfig {
            json_rpc: Some(mock_server.uri().to_string()),
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
        while sync_block_count.load(std::sync::atomic::Ordering::Relaxed) < 1 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let client = RpcClient::try_new(format!("http://{addr}").parse().unwrap()).await;
        assert!(client.is_ok());
        let mut client = client.unwrap();
        let mut res = client
            .get_block_range(chain_id, block_number, block_number)
            .await
            .unwrap();
        let block = res
            .next()
            .await
            .expect("stream should not be empty")
            .expect("block should be valid");
        let block = Block::try_from(proto::Block::decode(block.data.as_slice()).unwrap()).unwrap();
        assert_eq!(
            block,
            Block::from_header_and_transactions_and_receipts(
                block_header,
                transactions,
                block_receipts
            )
        );
        token.cancel();
        task.abort();
    }
}
