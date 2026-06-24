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

use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config,
};

/// Using multi-threaded runtime so that client and server are executed in parallel and not just
/// concurrently because this simulates the real world usage.
#[tokio::test(flavor = "multi_thread")]
async fn client_list_remote_then_fetch_metadata_then_list_then_view_metadata() {
    const CHAIN_ID: u64 = 146;
    let server_dir = tempfile::tempdir().unwrap();

    // Initialize the server blockservice
    init_blockservice(
        Some(server_dir.path()),
        &[make_default_sonic_chain_config()],
    )
    .await
    .expect("blockservice should initialize");

    // Import rules update heights
    let file = server_dir.path().join("rules-update-heights");
    std::fs::write(&file, b"rules-update-heights").unwrap();
    let CommandExecutionOutput { result, .. } = execute_command(
        Command::ImportRulesUpdateHeights {
            chain_id: CHAIN_ID,
            file,
        },
        server_dir.path(),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());

    // Import corrections
    let file = server_dir.path().join("corrections");
    std::fs::write(&file, b"corrections").unwrap();
    let CommandExecutionOutput { result, .. } = execute_command(
        Command::ImportCorrections {
            chain_id: CHAIN_ID,
            file,
        },
        server_dir.path(),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());

    // Start the server
    let server = IntegrationTestServer::new(server_dir.path(), vec![], None).await;

    // Init client
    let client_dir = init_blockservice(None, &[make_default_sonic_chain_config()])
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
    assert!(result.is_ok());
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
        [146] SONIC: SONIC test chain
        ├── rules update heights: yes
        ├── corrections: yes
        └── no blocks
        "}
    );

    // Fetch metadata
    let CommandExecutionOutput { result, .. } = execute_command(
        Command::FetchMetadata {
            url: server.uri(),
            chain_id: CHAIN_ID,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());

    // List local chains
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: None,
            url: None,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(
        String::from_utf8_lossy(&log),
        indoc::indoc! {"
        [146] SONIC: SONIC test chain
        ├── rules update heights: yes
        ├── corrections: yes
        └── no blocks
        "}
    );

    // Print the rules update heights
    let CommandExecutionOutput { result, log } = execute_command(
        Command::ViewRulesUpdateHeights { chain_id: CHAIN_ID },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(log, b"rules-update-heights\n");

    // Print the corrections
    let CommandExecutionOutput { result, log } = execute_command(
        Command::ViewCorrections { chain_id: CHAIN_ID },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(log, b"corrections\n");
}
