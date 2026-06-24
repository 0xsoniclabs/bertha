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
    CommandExecutionOutput, execute_command, init_blockservice, make_default_sonic_chain_config,
};

#[tokio::test]
async fn client_connect_to_unavailable_server_fails() {
    let client_dir = init_blockservice(None, &[make_default_sonic_chain_config()])
        .await
        .expect("blockservice should initialize");

    // Try to connect to an unavailable server
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain: None,
            url: Some("http://[::1]:0".to_string()), // Always refused connection
        },
        &client_dir,
        None,
        None,
        None,
    )
    .await;
    assert!(
        result.is_err(),
        "list should fail when server is unavailable"
    );
    let err = result.unwrap_err();
    assert!(err.to_string().contains("transport error"));
    assert!(log.is_empty(), "log should be empty on error");
}
