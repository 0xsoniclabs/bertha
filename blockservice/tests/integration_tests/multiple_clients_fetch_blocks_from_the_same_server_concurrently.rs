use std::vec;

use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, add_chain_configs_to_config_file,
    check_init_output, execute_command, make_default_sonic_chain_config, make_snapshot_file,
};

#[tokio::test(flavor = "multi_thread")]
async fn multiple_clients_fetch_blocks_from_the_same_server_concurrently() {
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
        None, // Chain config
    )
    .await;

    let url = server.uri();
    let client = async |from, to| {
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

        // Fetch all SONIC blocks from the server
        let CommandExecutionOutput { result, log } = execute_command(
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
        assert!(result.is_ok(), "fetch should succeed");
        assert!(String::from_utf8_lossy(&log).contains(&format!(
            "Fetched and wrote {} blocks, total uncompressed size",
            to - from + 1
        )));

        // List blocks in the client
        let CommandExecutionOutput { result, log } = execute_command(
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
        assert!(result.is_ok(), "list should succeed");
        assert_eq!(
            String::from_utf8_lossy(&log),
            indoc::formatdoc! {"
            [146] SONIC: SONIC test chain
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
