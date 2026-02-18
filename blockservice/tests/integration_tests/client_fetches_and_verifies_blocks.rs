// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

use std::vec;

use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config, make_snapshot_file,
};

/// Using multi-threaded runtime so that client and server are executed in parallel and not just
/// concurrently because this simulates the real world usage.
#[tokio::test(flavor = "multi_thread")]
async fn client_fetches_and_verifies_blocks() {
    const CHAIN_ID: u64 = 146;
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            CHAIN_ID,          // CHAIN_ID
            30,                // num_blocks
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
    assert!(result.is_ok(), "list should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
        [146] SONIC: SONIC test chain
        └── 0 - 29
        "}
    );

    // Fetch the first 10 blocks from the SONIC chain
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: CHAIN_ID,
            from: None,
            to: Some(9),
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

    // Verify the fetched SONIC blocks
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Verify {
            chain_id: CHAIN_ID,
            block_number: Some(0),
            block_hash: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "verify should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        "[chain ID 146] All blocks verified successfully.\n"
    );

    // Fetch the next 20 blocks from the SONIC chain
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: CHAIN_ID,
            from: Some(10),
            to: Some(29),
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
        "Fetched and wrote 20 blocks, total uncompressed size: 0 MiB\n"
    );

    // Verify the fetched SONIC blocks
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Verify {
            chain_id: CHAIN_ID,
            block_number: Some(10),
            block_hash: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "verify should succeed");
    assert_eq!(
        String::from_utf8_lossy(&log),
        "[chain ID 146] All blocks verified successfully.\n"
    );
}
