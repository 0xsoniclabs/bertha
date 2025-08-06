use std::{io::Write, vec};

use blockservice::{cli::Command, config::ChainConfig};

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, add_chain_configs_to_config_file,
    check_init_output, execute_command, make_default_sonic_chain_config, make_snapshot_file,
};

#[tokio::test(flavor = "multi_thread")]
async fn client_fetches_state_updates() {
    let server_dir = tempfile::tempdir().unwrap();
    // Make a stub file for state updates
    let filepath = server_dir.path().join("state_updates.json");
    {
        let mut file = std::fs::File::create(filepath.as_path()).unwrap();
        file.write_all([1, 2, 3].as_slice()).unwrap();
    }

    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            146,               // chain_id
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

    // Fetch state updates for the SONIC chain
    let CommandExecutionOutput { result, log } = execute_command(
        Command::FetchStateUpdates {
            url: server.uri(),
            chain_id: 146,
        },
        client_dir.path().to_path_buf(),
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

    let file_content = std::fs::read(client_dir.path().join("state_updates.json")).unwrap();
    assert_eq!(file_content, vec![1, 2, 3]);
}
