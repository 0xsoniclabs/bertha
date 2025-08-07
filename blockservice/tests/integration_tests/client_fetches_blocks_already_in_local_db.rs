use std::vec;

use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config, make_snapshot_file,
};

#[tokio::test(flavor = "multi_thread")]
async fn client_fetches_blocks_already_in_local_db() {
    const CHAIN_ID: u64 = 146;
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            CHAIN_ID,          // CHAIN_ID
            10,                // num_blocks
            &[],               // extra_blocks
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
    println!("Server URL: {}", server.uri());
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

    // Fetch the same blocks again, which should be skipped
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
        "No blocks to fetch for chain ID 146 in range 0 to 9: All blocks are already available locally\n"
    );
}
