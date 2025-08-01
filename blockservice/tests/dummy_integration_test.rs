use blockservice::cli::Command;

use crate::test_utils::{
    IntegrationTestServer, check_init_output, execute_command, make_snapshot_file,
};

mod test_utils;

#[tokio::test]
async fn dummy_client_server_integration() {
    // Start a server
    let server_dir = tempfile::tempdir().unwrap();
    let snapshot_files = vec![make_snapshot_file(
        server_dir.path(),
        146, // chain_id
        10,  // num blocks
        &[], // extra blocks
    )];
    let server = IntegrationTestServer::new(server_dir.path(), snapshot_files).await;

    // To start a client, we need to initialize it.
    // NOTE: While this logic could be extracted into a separate "init client" function, this is not
    // advisable as we would loose track of where the function asserted if it did. Unfortunately,
    // #[track_caller] is not available for async functions.
    let client_dir = tempfile::tempdir().unwrap();
    let (result, output) = execute_command(
        Command::Init,     // Command to execute
        client_dir.path(), // Working directory
        None,              // Optional input to the command
        None,              // Optional cancellation token for lifetime management
        None,              // Optional address binder selector
    )
    .await;
    assert!(result.is_ok(), "Failed to initialize client");
    check_init_output(&output, client_dir.path());

    // Execute a command that connects to the server
    let (result, output) = execute_command(
        Command::List {
            chain_id: Some(146),
            url: Some(server.uri()),
        },
        client_dir.path(),
        None,
        None,
        None,
    )
    .await;
    assert!(result.is_ok(), "Failed to list blocks");
    // Now we use the command output to verify that the command worked as expected
    let output_str = String::from_utf8(output).unwrap();
    assert_eq!(
        output_str,
        indoc::indoc! {"
            [146] (no name): (no description)
            └── 0 - 9
            "},
    );
}
