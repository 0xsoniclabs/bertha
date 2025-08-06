use std::vec;

use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, add_chain_configs_to_config_file,
    check_init_output, execute_command, make_default_sonic_chain_config, make_snapshot_file,
};

#[tokio::test(flavor = "multi_thread")]
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
        None, // Chain config
    )
    .await;

    // Init client
    let client_dir = tempfile::tempdir().unwrap();
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Init {},
        client_dir.path().to_path_buf(),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "init should succeed");
    check_init_output(&log, client_dir.path());
    add_chain_configs_to_config_file(
        [make_default_sonic_chain_config()].as_slice(),
        client_dir.path(),
    )
    .unwrap();

    // List remote chains
    let CommandExecutionOutput { result, log } = execute_command(
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
    assert!(result.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        "Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n"
    );

    // Fetch the same blocks again, which should be skipped
    let CommandExecutionOutput { result, log } = execute_command(
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
    assert!(result.is_ok(), "fetch should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        "No blocks to fetch for chain ID 146 in range 0 to 9: All blocks are already available locally\n"
    );
}
