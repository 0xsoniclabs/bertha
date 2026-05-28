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

use bertha_types::Block;
use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, IntegrationTestServer, execute_command, init_blockservice,
    make_default_sonic_chain_config, make_snapshot_file,
};

/// Using multi-threaded runtime so that client and server are executed in parallel and not just
/// concurrently because this simulates the real world usage.
#[tokio::test(flavor = "multi_thread")]
async fn client_fetches_and_prints_out_a_block() {
    const CHAIN_ID: u64 = 146;
    let server_dir = tempfile::tempdir().unwrap();
    let server = IntegrationTestServer::new(
        server_dir.path(),
        vec![make_snapshot_file(
            server_dir.path(), // workdir
            CHAIN_ID,          // CHAIN_ID
            5,                 // num_blocks
            &[Block {
                number: 5,
                ..Block::default_sonic()
            }], // extra_blocks
        )],
        None, // Chain config
    )
    .await;

    // Init client
    let client_dir = init_blockservice(None, &[make_default_sonic_chain_config()])
        .await
        .expect("blockservice should initialize");

    // List remote chains
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: Some(CHAIN_ID),
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
        └── 0 - 5
        "}
    );

    // Fetch block 5
    let CommandExecutionOutput { result, log } = execute_command(
        Command::Fetch {
            url: server.uri(),
            chain_id: CHAIN_ID,
            from: Some(5),
            to: Some(5),
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
        "Fetched and wrote 1 blocks, total uncompressed size: 0 MiB\n"
    );

    // Visualize the block
    let CommandExecutionOutput { result, log } = execute_command(
        Command::View {
            chain_id: CHAIN_ID,
            block_number: 5,
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "view should succeed");
    //
    assert_eq!(
        String::from_utf8_lossy(&log),
        r#"{
  "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
  "miner": "0x0000000000000000000000000000000000000000",
  "stateRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "difficulty": "0x0",
  "number": "0x5",
  "gasLimit": "0x0",
  "timestamp": "0x0",
  "extraData": "0x000000000000000000000000",
  "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "nonce": "0x0000000000000000",
  "transactions": [],
  "receipts": [],
  "baseFeePerGas": "0x0",
  "withdrawalsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
  "blobGasUsed": "0x0",
  "excessBlobGas": "0x0",
  "parentBeaconBlockRoot": null,
  "requestsHash": null,
  "withdrawals": []
}
"#
    );
}
