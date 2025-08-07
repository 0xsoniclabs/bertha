use std::vec;

use blockservice::{BlockRange, cli::Command};

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config, make_snapshot_file,
};

#[tokio::test(flavor = "multi_thread")]
async fn multiple_clients_fetch_blocks_from_the_same_server_concurrently() {
    const CHAIN_ID: u64 = 146;
    const BLOCK_COUNT: u64 = 10000;
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(),    // workdir
            CHAIN_ID,             // CHAIN_ID
            BLOCK_COUNT as usize, // num_blocks
            &[],                  // extra_blocks
        )],
        None, // Chain config
    )
    .await;

    let url = server.uri();
    let fetch_blocks = async |from, to| {
        let client_dir = init_blockservice(None, [make_default_sonic_chain_config()].as_slice())
            .await
            .expect("blockservice should initialize");

        // Fetch all SONIC blocks from the server
        let CommandExecutionOutput { result, log } = execute_command(
            Command::Fetch {
                url,
                chain_id: CHAIN_ID,
                from: Some(from),
                to: Some(to),
            },
            &client_dir,
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
            indoc::formatdoc! {"
            [146] SONIC: SONIC test chain
            └── {} - {}
            ", from, to}
        );
    };

    // Ranges to fetch. Each range is fetched by a different client.
    let cases = vec![
        // Non overlapping ranges
        [
            BlockRange::new(0, BLOCK_COUNT / 2),
            BlockRange::new(BLOCK_COUNT / 2 + 1, BLOCK_COUNT - 1),
        ],
        // Overlapping ranges
        [
            BlockRange::new(0, BLOCK_COUNT - 1),
            BlockRange::new(0, BLOCK_COUNT - 1),
        ],
    ];

    for case in cases {
        let mut clients = vec![];
        for block_range in case.iter() {
            let start = *block_range.start();
            let end = *block_range.end();
            let fetch_blocks = fetch_blocks.clone();
            clients.push((
                block_range,
                tokio::spawn(async move {
                    fetch_blocks(start, end).await;
                }),
            ));
        }
        // Await for all clients to make sure they all complete
        for (range_to_fetch, client) in clients {
            let result = client.await;
            assert!(
                result.is_ok(),
                "client fetching range {} - {} should complete successfully",
                range_to_fetch.start(),
                range_to_fetch.end()
            );
        }
    }
}
