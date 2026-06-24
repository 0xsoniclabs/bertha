// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use std::vec;

use blockservice::{
    BlockRange,
    cli::{Chain, Command},
};

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config, make_snapshot_file,
};

/// Using multi-threaded runtime so that client and server are executed in parallel and not just
/// concurrently because this simulates the real world usage.
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
        let client_dir = init_blockservice(None, &[make_default_sonic_chain_config()])
            .await
            .expect("blockservice should initialize");

        // Fetch all SONIC blocks from the server
        let CommandExecutionOutput { result, log } = execute_command(
            Command::Fetch {
                url,
                chain: Chain::Id(CHAIN_ID),
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
                chain: Some(Chain::Id(CHAIN_ID)),
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
            ├── rules update heights: no
            ├── corrections: no
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
