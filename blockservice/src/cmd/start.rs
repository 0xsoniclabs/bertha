use std::{collections::HashMap, ops::Deref, path::Path, sync::Arc};

use tokio_stream::{StreamExt, StreamMap};
use tokio_util::sync::CancellationToken;

use crate::{
    app_dir::open_app_dir,
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
    config: HashMap<u64, String>,
    cancellation_token: CancellationToken,
    _test_notify_tasks_spawned: Option<Arc<tokio::sync::Notify>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (_cfg, db) = open_app_dir(app_dir, false)?;
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
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {},
                // NOTE: Even though `sync` internally spawns another task (via `subscribe_to_blocks`),
                // we don't have to pass a cancellation token to it, as the task will exit once
                // the read-end of the stream is closed.
                r = sync(&config, db.deref()) => {
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

    let server = RpcServer::new(db);
    server.serve(listener).await
}

async fn sync(
    json_rpc_config: &HashMap<u64, String>,
    db: &impl BlockDb,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut streams = StreamMap::new();
    for (&chain_id, server) in json_rpc_config {
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
            Ok(block) => db.put(chain_id, block)?,
            Err(err) => println!("[chain ID {chain_id}] error fetching next block: {err}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

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
    };

    #[tokio::test]
    async fn start_starts_server_successfully() {
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        {
            let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();
            db.put_raw(1, 1, vec![1, 2, 3].as_slice()).unwrap();
        }

        let config = HashMap::new();
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let token = CancellationToken::new();
        let job = tokio::spawn({
            let token = token.clone();
            async move {
                start(tmpdir.path(), listener, config, token, None)
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
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();

        let metrics = tokio::runtime::Handle::current().metrics();
        let num_tasks_before = metrics.num_alive_tasks();

        let tasks_spawned = Arc::new(tokio::sync::Notify::new());
        let token = CancellationToken::new();
        let job = tokio::spawn({
            let token = token.clone();
            let tasks_spawned = tasks_spawned.clone();
            async move {
                start(
                    tmpdir.path(),
                    listener,
                    HashMap::new(),
                    token,
                    Some(tasks_spawned),
                )
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
        let tmpdir = tempfile::tempdir().unwrap();
        let config = HashMap::new();
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let result = start(
            tmpdir.path(),
            listener,
            config,
            CancellationToken::new(),
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
        let tmpdir = tempfile::tempdir().unwrap();
        init_app_dir(tmpdir.path()).unwrap();
        let (_, db) = open_app_dir(tmpdir.path(), false).unwrap();

        let json_rpc_config = [(1, "invalid_url".to_string())].into_iter().collect();

        let result = sync(&json_rpc_config, &db).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid URL"));
    }

    #[tokio::test]
    async fn sync_forwards_db_error() {
        let tmpdir = tempfile::tempdir().unwrap();

        init_app_dir(tmpdir.path()).unwrap();
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

        let json_rpc_config = [(1, mock_server.uri())].into_iter().collect();

        let result = sync(&json_rpc_config, &db).await;
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
        let tmpdir = tempfile::tempdir().unwrap();

        init_app_dir(tmpdir.path()).unwrap();
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

        let json_rpc_config = [(1, mock_server.uri()), (2, mock_server.uri())]
            .into_iter()
            .collect();

        let task = tokio::spawn({
            let db = Arc::clone(&db);
            async move {
                sync(&json_rpc_config, db.deref()).await.unwrap();
            }
        });
        // wait for the sync task to fetch the header, transactions and receipts for the first block
        while mock_server.received_requests().await.unwrap().len() < 4 {
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
        let tmpdir = tempfile::tempdir().unwrap();

        init_app_dir(tmpdir.path()).unwrap();

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

        let config = HashMap::from([(chain_id, mock_server.uri())]);
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let token = CancellationToken::new();
        let task = tokio::spawn({
            let token = token.clone();
            async move {
                start(tmpdir.path(), listener, config, token, None)
                    .await
                    .unwrap();
            }
        });
        // wait for the sync task to fetch the header, transactions and receipts for the first block
        while mock_server.received_requests().await.unwrap().len() < 2 {
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
