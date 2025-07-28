use std::{io::Cursor, vec};

use bertha_types::Block;
use blockservice::{
    cli::Command,
    config::{ChainConfig, Config},
};

use crate::test_utils::*;

mod test_utils;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_fetches_multiple_blocks_and_format_output() {
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![
            make_snapshot_file(
                server_dir.path(), // workdir
                146,               // chain ID
                10,                // block count
                &[],               // extra blocks
            ),
            make_snapshot_file(
                server_dir.path(), // workdir
                1,                 // chain_id
                20,                // num_blocks
                &[],               // extra_blocks
            ),
        ],
    )
    .await;

    // Init client
    let client_dir = tempfile::tempdir().unwrap();
    let (res, output) = execute_command(
        Command::Init {},
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "init should succeed");
    check_init_output(&output, client_dir.path());

    // List remote chains
    let (res, output) = execute_command(
        Command::List {
            chain_id: None,
            url: Some(server.uri()),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"
        [1] (no name): (no description)
        └── 0 - 19
        [146] (no name): (no description)
        └── 0 - 9
        "}
    );

    // Fetch all SONIC blocks from the server
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: None,
            to: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n"
    );

    // Fetch the first 5 blocks from the Ethereum chain
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 1,
            from: Some(0),
            to: Some(4),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 5 blocks, total uncompressed size: 0 MiB\n"
    );

    // List blocks in the client
    let (res, output) = execute_command(
        Command::List {
            chain_id: None,
            url: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "list should succeed");

    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"[1] (no name): (no description)
        └── 0 - 4
        [146] (no name): (no description)
        └── 0 - 9
        "}
    );

    // Set pretty print for the output
    let config_path = client_dir.path().join("blockservice.toml");
    let mut config = Config::load(&config_path).unwrap();
    config
        .add_chain(ChainConfig {
            id: 1,
            name: "Ethereum".to_string(),
            description: "Ethereum chain".to_string(),
            state_updates: None,
        })
        .expect("chain should be added");
    config
        .add_chain(ChainConfig {
            id: 146,
            name: "SONIC".to_string(),
            description: "SONIC chain".to_string(),
            state_updates: None,
        })
        .expect("chain should be added");

    // List blocks in the client with pretty print
    let (res, output) = execute_command(
        Command::List {
            chain_id: None,
            url: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"
        [1] Ethereum: Ethereum chain
        └── 0 - 4
        [146] SONIC: SONIC chain
        └── 0 - 9
        "}
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_fetches_blocks_already_in_local_db() {
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            146,               // chain_id
            10,                // num_blocks
            &[],               // extra_blocks
        )],
    )
    .await;

    // Init client
    let client_dir = tempfile::tempdir().unwrap();
    let (res, output) = execute_command(
        Command::Init {},
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "init should succeed");
    check_init_output(&output, client_dir.path());

    // List remote chains
    let (res, output) = execute_command(
        Command::List {
            chain_id: None,
            url: Some(server.uri()),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    println!("Server URL: {}", server.uri());
    assert!(res.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"
        [146] (no name): (no description)
        └── 0 - 9
        "}
    );

    // Fetch all SONIC blocks from the server
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: None,
            to: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n"
    );

    // Fetch the same blocks again, which should be skipped
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: None,
            to: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "No blocks to fetch for chain ID 146 in range 0 to 9: All blocks are already available locally\n"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_fetches_and_verifies_blocks() {
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            146,               // chain_id
            30,                // num_blocks
            &[],               // extra_blocks
        )],
    )
    .await;

    // Init client
    let client_dir = tempfile::tempdir().unwrap();
    let (res, output) = execute_command(
        Command::Init {},
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "init should succeed");
    check_init_output(&output, client_dir.path());

    // List remote chains
    let (res, output) = execute_command(
        Command::List {
            chain_id: None,
            url: Some(server.uri()),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"
        [146] (no name): (no description)
        └── 0 - 29
        "}
    );

    // Fetch the first 10 blocks from the SONIC chain
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: None,
            to: Some(9),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n"
    );

    // Verify the fetched SONIC blocks
    let (res, output) = execute_command(
        Command::Verify {
            chain_id: 146,
            block_number: Some(0),
            block_hash: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "verify should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "[chain ID 146] All blocks verified successfully.\n"
    );

    // Fetch the next 20 blocks from the SONIC chain
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: Some(10),
            to: Some(29),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 20 blocks, total uncompressed size: 0 MiB\n"
    );

    // Verify the fetched SONIC blocks
    let (res, output) = execute_command(
        Command::Verify {
            chain_id: 146,
            block_number: Some(10),
            block_hash: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "verify should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "[chain ID 146] All blocks verified successfully.\n"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_fetches_and_prints_out_a_block() {
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            146,               // chain_id
            5,                 // num_blocks
            &[Block {
                number: 5,
                ..Block::default_sonic()
            }], // extra_blocks
        )],
    )
    .await;

    // Init client
    let client_dir = tempfile::tempdir().unwrap();
    let (res, output) = execute_command(
        Command::Init {},
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "init should succeed");
    check_init_output(&output, client_dir.path());

    // List remote chains
    let (res, output) = execute_command(
        Command::List {
            chain_id: Some(146),
            url: Some(server.uri()),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"
        [146] (no name): (no description)
        └── 0 - 5
        "}
    );

    // Fetch block 5
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: Some(5),
            to: Some(5),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 1 blocks, total uncompressed size: 0 MiB\n"
    );

    // Visualize the block
    let (res, output) = execute_command(
        Command::View {
            chain_id: 146,
            block_number: 5,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "view should succeed");
    //
    assert_eq!(
        String::from_utf8_lossy(&output),
        r#"{
  "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
  "miner": "0x0000000000000000000000000000000000000000",
  "stateRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "difficulty": "0x0",
  "number": "0x5",
  "gasLimit": "0x0",
  "timestamp": "0x0",
  "extraData": "0x000000000000000000000000",
  "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "nonce": "0x0000000000000000",
  "transactions": [],
  "receipts": [],
  "baseFeePerGas": "0x0",
  "withdrawalsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
  "blobGasUsed": "0x0",
  "excessBlobGas": "0x0",
  "parentBeaconBlockRoot": null,
  "requestsHash": null
}
"#
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_fetches_and_purges_data_and_fetches_again() {
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            146,               // chain_id
            10,                // num_blocks
            &[],               // extra_blocks
        )],
    )
    .await;

    // Init client
    let client_dir = tempfile::tempdir().unwrap();
    let (res, output) = execute_command(
        Command::Init {},
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "init should succeed");
    check_init_output(&output, client_dir.path());

    // List remote chains
    let (res, output) = execute_command(
        Command::List {
            chain_id: None,
            url: Some(server.uri()),
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"
        [146] (no name): (no description)
        └── 0 - 9
        "}
    );

    // Fetch all SONIC blocks from the server
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: None,
            to: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n"
    );

    // Purge blocks from the local database
    let (res, output) = execute_command(
        Command::Purge {
            chain_id: 146,
            from: Some(5),
            to: Some(8),
        },
        client_dir.path().to_path_buf(),
        Some(Cursor::new("y\n")), // Simulate user confirmation
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "purge should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Purging 4 blocks in range 5 - 8 for chain ID 146. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
    );

    // Repeat the fetch command to fix the local database
    let (res, output) = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: 146,
            from: None,
            to: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        "Fetched and wrote 4 blocks, total uncompressed size: 0 MiB\n"
    );

    // List blocks in the client
    let (res, output) = execute_command(
        Command::List {
            chain_id: Some(146),
            url: None,
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&output),
        indoc::indoc! {"
        [146] (no name): (no description)
        └── 0 - 9
        "}
    );
}

#[tokio::test]
async fn client_connects_to_unavailable_server() {
    let client_dir = tempfile::tempdir().unwrap();
    let (res, output) = execute_command(
        Command::Init {},
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_ok(), "init should succeed");
    check_init_output(&output, client_dir.path());

    // Try to connect to an unavailable server
    let (res, _output) = execute_command(
        Command::List {
            chain_id: None,
            url: Some("http://[::1]:0".to_string()), // Always refused connection
        },
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(res.is_err(), "list should fail when server is unavailable");
    let err = res.unwrap_err();
    assert!(err.to_string().contains("transport error"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn multiple_client_fetch_the_same_server() {
    const BLOCK_COUNT: u64 = 10000;
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(),    // workdir
            146,                  // chain_id
            BLOCK_COUNT as usize, // num_blocks
            &[],                  // extra_blocks
        )],
    )
    .await;

    let url = server.uri();
    let client = async |from, to| {
        let client_dir = tempfile::tempdir().unwrap();
        let (res, output) = execute_command(
            Command::Init {},
            client_dir.path().to_path_buf(),
            None,
            None,
            None,
        )
        .await;
        assert!(res.is_ok(), "init should succeed");
        check_init_output(&output, client_dir.path());

        // Fetch all SONIC blocks from the server
        let (res, output) = execute_command(
            Command::Fetch {
                url,
                chain_id: 146,
                from: Some(from),
                to: Some(to),
            },
            client_dir.path().to_path_buf(),
            None,
            None,
            None,
        )
        .await;
        assert!(res.is_ok(), "fetch should succeed");
        assert!(String::from_utf8_lossy(&output).contains(&format!(
            "Fetched and wrote {} blocks, total uncompressed size",
            to - from + 1
        )));

        // List blocks in the client
        let (res, output) = execute_command(
            Command::List {
                chain_id: Some(146),
                url: None,
            },
            client_dir.path().to_path_buf(),
            None,
            None,
            None,
        )
        .await;
        assert!(res.is_ok(), "list should succeed");
        assert_eq!(
            String::from_utf8_lossy(&output),
            indoc::formatdoc! {"
            [146] (no name): (no description)
            └── {} - {}
            ", from, to}
        );
    };

    // Spawn multiple clients to fetch different ranges of blocks
    let client_1 = tokio::spawn({
        let client = client.clone();
        async move {
            client(0, BLOCK_COUNT / 2).await;
        }
    });

    let client_2 = tokio::spawn(async move {
        client(BLOCK_COUNT / 2 + 1, BLOCK_COUNT - 1).await;
    });

    client_1
        .await
        .expect("client 1 should complete successfully");
    client_2
        .await
        .expect("client 2 should complete successfully");
}
