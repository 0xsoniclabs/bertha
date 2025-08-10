use std::{io::Write, vec};

use blockservice::{cli::Command, config::ChainConfig};

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config, make_snapshot_file,
};

/// Using multi-threaded runtime so that client and server are executed in parallel and not just
/// concurrently because this simulates the real world usage.
#[tokio::test(flavor = "multi_thread")]
async fn client_fetches_state_updates() {
    const CHAIN_ID: u64 = 146;
    let file_content = vec![1, 2, 3];
    let server_dir = tempfile::tempdir().unwrap();
    // Make a stub file for state updates
    let filepath = server_dir.path().join("state_updates.json");
    {
        let mut file = std::fs::File::create(filepath.as_path()).unwrap();
        file.write_all(file_content.clone().as_slice()).unwrap();
    }

    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            CHAIN_ID,          // CHAIN_ID
            10,                // num_blocks
            &[],               // extra_blocks
        )],
        Some(vec![ChainConfig {
            state_updates: Some(vec![filepath]),
            ..make_default_sonic_chain_config()
        }]), // Chain config
    )
    .await;

    // Init client
    let client_dir = init_blockservice(None, [make_default_sonic_chain_config()].as_slice())
        .await
        .expect("blockservice should initialize");

    // Fetch state updates for the SONIC chain
    let CommandExecutionOutput { result, log } = execute_command(
        Command::FetchStateUpdates {
            url: server.uri(),
            chain_id: CHAIN_ID,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "fetch state updates should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
        Received 1 state update files for chain ID 146
        state_updates.json
        "}
    );

    let result_file_content = std::fs::read(client_dir.join("state_updates.json")).unwrap();
    assert_eq!(result_file_content, file_content);
}
