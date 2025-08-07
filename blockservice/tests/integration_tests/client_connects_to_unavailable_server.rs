use blockservice::cli::Command;

use crate::test_utils::{
    CommandExecutionOutput, execute_command, init_blockservice, make_default_sonic_chain_config,
};

#[tokio::test]
async fn client_connects_to_unavailable_server() {
    let client_dir = init_blockservice(None, &[make_default_sonic_chain_config()])
        .await
        .expect("blockservice should initialize");

    // Try to connect to an unavailable server
    let CommandExecutionOutput { result, log } = execute_command(
        Command::List {
            chain_id: None,
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
