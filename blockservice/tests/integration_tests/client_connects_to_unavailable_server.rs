use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, add_chain_configs_to_config_file, check_init_output, execute_command,
    make_default_sonic_chain_config,
};

#[tokio::test]
async fn client_connects_to_unavailable_server() {
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

    // Try to connect to an unavailable server
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: None,
            url: Some("http://[::1]:0".to_string()), // Always refused connection
        },
        client_dir.path().to_path_buf(),
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
