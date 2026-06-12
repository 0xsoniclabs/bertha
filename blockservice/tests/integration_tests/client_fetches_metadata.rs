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

use blockservice::{app_dir::open_app_dir, cli::Command, db::BlockDb};

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config,
};

/// Using multi-threaded runtime so that client and server are executed in parallel and not just
/// concurrently because this simulates the real world usage.
#[tokio::test(flavor = "multi_thread")]
async fn client_fetches_metadata() {
    const CHAIN_ID: u64 = 146;
    let server_dir = tempfile::tempdir().unwrap();

    // Initialize the server blockservice
    init_blockservice(
        Some(server_dir.path()),
        &[make_default_sonic_chain_config()],
    )
    .await
    .expect("blockservice should initialize");

    // Write metadata directly into the server's block database
    {
        let (_cfg, mut db) = open_app_dir(server_dir.path(), false).unwrap();
        db.put_upgrade_heights(CHAIN_ID, b"upgrade-heights")
            .unwrap();
        db.put_corrections(CHAIN_ID, b"corrections").unwrap();
    }

    // Start the server
    let server = IntegrationTestServer::new(server_dir.path(), vec![], None).await;

    // Init client
    let client_dir = init_blockservice(None, [make_default_sonic_chain_config()].as_slice())
        .await
        .expect("blockservice should initialize");

    // Fetch metadata
    let CommandExecutionOutput { result, log } = execute_command(
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

    // Verify metadata was persisted in the client's block database
    {
        let (_cfg, db) = open_app_dir(client_dir, true).unwrap();
        assert_eq!(
            db.get_upgrade_heights(CHAIN_ID).unwrap(),
            Some(b"upgrade-heights".to_vec())
        );
        assert_eq!(
            db.get_corrections(CHAIN_ID).unwrap(),
            Some(b"corrections".to_vec())
        );
    }

    let log_str = String::from_utf8_lossy(&log);
    assert!(log_str.contains("Stored upgrade heights for chain ID 146"));
    assert!(log_str.contains("Stored corrections for chain ID 146"));
}
