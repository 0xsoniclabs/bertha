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
    make_default_sonic_chain_config, make_snapshot_file,
};

/// Using multi-threaded runtime so that client and server are executed in parallel and not just
/// concurrently because this simulates the real world usage.
#[tokio::test(flavor = "multi_thread")]
async fn client_fetch_then_fetch_again_is_noop() {
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
    let client_dir = init_blockservice(None, &[make_default_sonic_chain_config()])
        .await
        .expect("blockservice should initialize");

    // Fetch blocks from the server
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
        log,
        b"Fetched and wrote 10 blocks, total uncompressed size: 0 MiB\n"
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
        log,
        b"No blocks to fetch for chain ID 146 in range 0 to 9: All blocks are already available locally\n"
    );
}
