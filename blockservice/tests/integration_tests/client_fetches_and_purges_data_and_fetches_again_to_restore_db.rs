use std::{io::Cursor, vec};

use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config, make_snapshot_file,
};

#[tokio::test(flavor = "multi_thread")]
async fn client_fetches_and_purges_data_and_fetches_again() {
    const CHAIN_ID: u64 = 146;
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            CHAIN_ID,
            10,  // num_blocks
            &[], // extra_blocks
        )],
        None, // Chain config
    )
    .await;

    // Init client
    let client_dir = init_blockservice(None, [make_default_sonic_chain_config()].as_slice())
        .await
        .expect("blockservice should initialize");

    // List remote chains
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: None,
            url: Some(server.uri()),
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
        [146] SONIC: SONIC test chain
        └── 0 - 9
        "}
    );

    // Fetch all SONIC blocks from the server
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: CHAIN_ID,
            from: None,
            to: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n"
    );

    // List blocks in the client
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: Some(CHAIN_ID),
            url: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
        [146] SONIC: SONIC test chain
        └── 0 - 9
        "}
    );

    // Purge blocks from the local database
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Purge {
            chain_id: CHAIN_ID,
            from: Some(5),
            to: Some(8),
        },
        &client_dir,
        Some(Cursor::new("y\n")), // Simulate user confirmation
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "purge should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        "Purging 4 blocks in range 5 - 8 for chain ID 146. Are you sure you want to continue? (y/n): Blocks successfully purged\n"
    );

    // List blocks in the client after purge
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: Some(CHAIN_ID),
            url: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
            [146] SONIC: SONIC test chain
            ├── 0 - 4
            └── 9 - 9
        "}
    );

    // Repeat the fetch command to fix the local database
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: CHAIN_ID,
            from: None,
            to: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        "Fetched and wrote 4 blocks, total uncompressed size: 0 MiB\n"
    );

    // List blocks in the client
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: Some(CHAIN_ID),
            url: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
        [146] SONIC: SONIC test chain
        └── 0 - 9
        "}
    );
}
