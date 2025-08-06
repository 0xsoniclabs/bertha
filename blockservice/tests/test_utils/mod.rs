use std::{
    io::Cursor,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
};

use bertha_types::Block;
use blockservice::{
    app_dir::CONFIG_FILE_NAME,
    cli::{Args, Command},
    cmd::{self, AddressBinder},
    config::{ChainConfig, Config},
};
use tokio::select;
use tokio_util::sync::{CancellationToken, DropGuard};

/// A server for integration tests which implements graceful shutdown
/// The implementation uses only the `blockservice` public interface.
/// NOTE: this will not stop request tasks spawned by Tonic as tasks spawned by other tasks are
/// detached. To be able to kill also those tasks, the server must be spawned in a separate
/// process.
pub struct IntegrationTestServer {
    addr: SocketAddr,
    _server_task_handle: DropGuard,
}

impl IntegrationTestServer {
    /// Initialize and starts a new [`IntegrationTestServer`] with the given app directory in a
    /// separate task.
    /// It imports the specified snapshot files and optionally save some chain configurations in the
    /// local config file.
    /// The server is started on a random available port.
    pub async fn new(
        app_dir: &Path,
        import_files: Vec<PathBuf>,
        chain_configs: Option<Vec<ChainConfig>>,
    ) -> IntegrationTestServer {
        let CommandExecutionOutput { result, log } =
            execute_command(Command::Init {}, app_dir, None, None, None).await;
        result.expect("initialization should succeed");
        check_init_output(&log, app_dir);

        // Set JSON RPC endpoints
        if let Some(json_rpc_endpoints) = chain_configs {
            set_chain_configs_to_config_file(&json_rpc_endpoints, app_dir).unwrap();
        }

        // Initialize the DB with the import files
        for file in import_files {
            let CommandExecutionOutput { result, log: _ } = execute_command(
                Command::Import {
                    snapshot_file: file,
                    verify: false,
                },
                app_dir,
                None,
                None,
                None,
            )
            .await;
            result.expect("import should succeed");
        }

        let cancellation_token = CancellationToken::new();
        let address_binder = RandomPortAddressBinder::try_new()
            .await
            .expect("address binding should succeed");
        let addr = address_binder.local_addr();

        tokio::spawn({
            let cancellation_token = cancellation_token.clone();
            let app_dir = app_dir.to_path_buf();
            async move {
                let CommandExecutionOutput { result, log: _ } = execute_command(
                    Command::Start,
                    app_dir,
                    Some(Cursor::new("")),
                    Some(cancellation_token),
                    Some(address_binder),
                )
                .await;
                result.expect("server should start successfully");
            }
        });

        IntegrationTestServer {
            addr,
            _server_task_handle: cancellation_token.drop_guard(),
        }
    }

    /// Returns the URL of the server.
    pub fn uri(&self) -> String {
        uri(self.addr)
    }
}

/// An [`AddressBinder`] that constructs a [`tokio::net::TcpListener`] bound to a random available
/// port
pub struct RandomPortAddressBinder {
    listener: tokio::net::TcpListener,
}

impl RandomPortAddressBinder {
    /// Attempts to create a new [`RandomPortAddressBinder`] that binds to a random available port
    /// on the unspecified address '::'.
    pub async fn try_new() -> std::io::Result<Self> {
        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        Ok(Self { listener })
    }

    /// Returns the bounded local address.
    pub fn local_addr(&self) -> SocketAddr {
        self.listener.local_addr().unwrap()
    }
}

#[tonic::async_trait]
impl AddressBinder for RandomPortAddressBinder {
    /// Returns the [`tokio::net::TcpListener`] bound to the random port.
    async fn bind_address(
        self,
    ) -> Result<tokio::net::TcpListener, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.listener)
    }
}

/// Constructs a snapshot file at the specified path with the given chain ID and number of blocks.
/// The extra blocks are appended to the generated genesis file.
/// The generated file name is `{chain_id}_{num_blocks}_snapshot`.
pub fn make_snapshot_file(
    path: &Path,
    chain_id: u64,
    num_blocks: usize,
    extra_blocks: &[Block],
) -> PathBuf {
    let genesis =
        genesis_parser::test_utils::generate_test_genesis(chain_id, num_blocks, extra_blocks);
    let filepath = path.join(format!(
        "{}_{}_snapshot",
        chain_id,
        num_blocks + extra_blocks.len()
    ));
    std::fs::write(filepath.clone(), genesis).unwrap();
    filepath
}

/// An helper struct to capture both the result of a command execution and its output log.
pub struct CommandExecutionOutput {
    pub result: Result<(), Box<dyn std::error::Error + Send + Sync>>,
    pub log: Vec<u8>,
}

/// Helper function to execute a command in the specified path with the specified input and capture
/// the result and output.
/// It optionally uses a cancellation token for graceful shutdown and an [`AddressBinder`] to
/// control the address the command binds to (if applicable).
pub async fn execute_command(
    command: Command,
    path: impl AsRef<Path>,
    input: Option<Cursor<&'static str>>,
    cancellation_token: Option<CancellationToken>,
    address_binder: Option<RandomPortAddressBinder>,
) -> CommandExecutionOutput {
    let mut output = vec![];
    let cancellation_token = cancellation_token.unwrap_or_default();
    let address_binder = match address_binder {
        Some(binder) => binder,
        None => RandomPortAddressBinder::try_new().await.unwrap(),
    };

    let result = select! {
        _ = cancellation_token.cancelled() => Ok(()),
       result = cmd::execute(
            Args {
                command,
                dir: path.as_ref().to_path_buf(),
            },
            cancellation_token.clone(),
            address_binder,
            &mut output,
            input.unwrap_or_default(),
        ) => result,
    };

    CommandExecutionOutput {
        result,
        log: output,
    }
}

/// Constructs a HTTP URI string from a [`SocketAddr`].
fn uri(addr: SocketAddr) -> String {
    format!("http://[{}]:{}", addr.ip(), addr.port())
}

/// Helper function to check the output of the `init` command is correct.
/// NOTE: It assumes neither the config file or the db exists prior to the command execution.
#[track_caller]
pub fn check_init_output(output: &[u8], path: impl AsRef<Path>) {
    assert_eq!(
        String::from_utf8_lossy(output),
        indoc::formatdoc! {"
                Initializing new blockservice directory at: {path}
                Creating new configuration at: {path}/blockservice.toml
                Creating new block database at: {path}/.blockdb
        ", path = path.as_ref().display()}
    );
}

/// Save the chain configurations to the config file at the specified path.
pub fn set_chain_configs_to_config_file(
    chain_configs: &[ChainConfig],
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = path.join(CONFIG_FILE_NAME);
    let mut config = Config::load(&config_path)?;

    for chain_config in chain_configs {
        config.add_chain(chain_config.clone())?;
    }

    Ok(())
}

/// Creates a default chain configuration for the SONIC chain.
pub fn make_sonic_default_chain_config() -> ChainConfig {
    ChainConfig {
        id: 146,
        name: "SONIC".to_string(),
        description: "SONIC test chain".to_string(),
        json_rpc: None,
        state_updates: None,
    }
}

#[cfg(test)]
mod tests {

    use std::{fs::File, io::BufReader, str::FromStr};

    use genesis_parser::Genesis;

    use super::*;

    #[test]
    fn uri_constructs_correct_http_uri() {
        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 8080);
        let expected_uri = "http://[::]:8080";
        assert_eq!(uri(addr), expected_uri);
    }

    #[tokio::test]
    async fn integration_test_server_new_starts_server() {
        let server_dir = tempfile::tempdir().unwrap();
        let server = IntegrationTestServer::new(
            server_dir.path(),
            vec![make_snapshot_file(server_dir.path(), 1, 10, &[])],
            Some(vec![make_sonic_default_chain_config()]),
        )
        .await;

        // Check the config file is correctly initialized
        let config_path = server_dir.path().join(CONFIG_FILE_NAME);
        let config = Config::load(&config_path).expect("Config should load successfully");
        let chain_configs = config.get_chain_configs();
        assert_eq!(chain_configs.len(), 2); // One for SONIC and one for the example one
        assert_eq!(chain_configs[0].id, 146);
        assert_eq!(chain_configs[0].name, "SONIC");
        assert_eq!(chain_configs[0].description, "SONIC test chain");

        // Query the server to check if it is running
        let client_dir = tempfile::tempdir().unwrap();
        let CommandExecutionOutput { result, log: _ } =
            execute_command(Command::Init {}, client_dir.path(), None, None, None).await;
        result.expect("Initialization should succeed");
        let CommandExecutionOutput { result, log } = execute_command(
            Command::List {
                chain_id: Some(1),
                url: Some(server.uri()),
            },
            server_dir.path(),
            None,
            None,
            None,
        )
        .await;
        result.expect("List command should succeed");
        assert_eq!(
            String::from_utf8(log).unwrap(),
            indoc::indoc! {"
            [1] (no name): (no description)
            └── 0 - 9
            "},
        );
    }

    #[tokio::test]
    async fn integration_test_server_stops_on_drop() {
        let server_dir = tempfile::tempdir().unwrap();
        let server = IntegrationTestServer::new(server_dir.path(), vec![], None).await;
        let server_url = server.uri();
        // Drop the server and check if it stops
        drop(server);
        // The server task should have been cancelled, so no further checks are needed

        let client_dir = tempfile::tempdir().unwrap();
        let CommandExecutionOutput { result, log: _ } =
            execute_command(Command::Init {}, client_dir.path(), None, None, None).await;
        result.expect("Initialization should succeed");

        let CommandExecutionOutput { result, log: _ } = execute_command(
            Command::List {
                chain_id: Some(1),
                url: Some(server_url),
            },
            server_dir.path(),
            None,
            None,
            None,
        )
        .await;
        assert!(
            result.is_err(),
            "List command should fail after server is dropped"
        );
    }

    #[tokio::test]
    async fn integration_test_server_uri_returns_valid_uri() {
        let server_dir = tempfile::tempdir().unwrap();
        let server = IntegrationTestServer::new(server_dir.path(), vec![], None).await;
        assert_eq!(
            &server.uri()[..7],
            "http://",
            "Server URI should start with http://"
        );
        assert!(
            SocketAddr::from_str(&server.uri()[7..]).is_ok(),
            "Server URI should be a valid SocketAddr"
        );
    }

    #[tokio::test]
    async fn make_snapshot_file_creates_correct_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let chain_id = 1;
        let num_blocks = 10;
        let extra_blocks = vec![Block::default_sonic(); 5];

        let snapshot_path =
            make_snapshot_file(temp_dir.path(), chain_id, num_blocks, &extra_blocks);
        assert!(snapshot_path.exists());
        assert_eq!(snapshot_path.file_name().unwrap(), "1_15_snapshot");

        // Parse the genesis file to verify its contents
        let file = File::open(snapshot_path).unwrap();
        let mut reader = BufReader::new(file);
        let mut genesis = Genesis::parse(&mut reader).expect("Genesis should parse successfully");

        let blocks = genesis.blocks().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(blocks.len(), num_blocks + extra_blocks.len());

        // First extra blocks
        for block in blocks.iter().take(extra_blocks.len()) {
            assert_eq!(block, &Block::default_sonic());
        }
        // Then the generated blocks
        for (i, block) in blocks.iter().skip(extra_blocks.len()).rev().enumerate() {
            assert_eq!(block.number, i as u64);
        }
    }

    #[tokio::test]
    async fn execute_command_executes_command() {
        let temp_dir = tempfile::tempdir().unwrap();
        let CommandExecutionOutput { result, log } =
            execute_command(Command::Init {}, temp_dir.path(), None, None, None).await;
        result.expect("Initialization should succeed");
        assert_eq!(
            String::from_utf8_lossy(&log),
            indoc::formatdoc! {"
            Initializing new blockservice directory at: {path}
            Creating new configuration at: {path}/blockservice.toml
            Creating new block database at: {path}/.blockdb
            ",
                path = temp_dir.path().display(),
            }
        );
    }

    #[tokio::test]
    async fn execute_terminates_command_on_cancellation_token_cancellation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cancellation_token = CancellationToken::new();
        let address_binder = RandomPortAddressBinder::try_new()
            .await
            .expect("Address binding should succeed");
        let address = address_binder.local_addr();

        let CommandExecutionOutput { result, log: _ } =
            execute_command(Command::Init, temp_dir.path(), None, None, None).await;
        result.expect("Initialization should succeed");

        let server_task = tokio::spawn({
            let cancellation_token = cancellation_token.clone();
            let path = temp_dir.path().to_path_buf();
            async move {
                let CommandExecutionOutput { result, log: _ } = execute_command(
                    Command::Start,
                    path,
                    Some(Cursor::new("")),
                    Some(cancellation_token),
                    Some(address_binder),
                )
                .await;
                result.expect("IntegrationTestServer should start successfully");
            }
        });

        // Check if the server is running by executing a command
        let CommandExecutionOutput { result, log: _ } = execute_command(
            Command::List {
                chain_id: Some(146),
                url: Some(uri(address)),
            },
            temp_dir.path(),
            None,
            Some(cancellation_token.clone()),
            None,
        )
        .await;
        assert!(result.is_ok(), "Command should complete successfully");

        // drop the server by canceling the cancellation token
        cancellation_token.cancel();
        select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                panic!("IntegrationTestServer task should complete before timeout");
            },
            result = server_task => {
                result.expect("IntegrationTestServer task should complete successfully");
            }
        }
    }

    #[test]
    fn set_chain_configs_to_config_file_saves_configs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE_NAME);
        {
            Config::create_default(&config_path).unwrap();
        }

        let chain_configs = vec![
            ChainConfig {
                id: 1,
                name: "Chain1".to_string(),
                description: "Test Chain 1".to_string(),
                json_rpc: None,
                state_updates: None,
            },
            ChainConfig {
                id: 2,
                name: "Chain2".to_string(),
                description: "Test Chain 2".to_string(),
                json_rpc: None,
                state_updates: None,
            },
        ];

        set_chain_configs_to_config_file(&chain_configs, temp_dir.path()).unwrap();

        let config = Config::load(config_path).unwrap();
        let res_chain_configs = config.get_chain_configs();
        assert_eq!(res_chain_configs.len(), 2);
        assert_eq!(res_chain_configs[0], chain_configs[0]);
        assert_eq!(res_chain_configs[1], chain_configs[1]);
    }

    #[test]
    fn make_sonic_default_chain_config_returns_correct_config() {
        let config = make_sonic_default_chain_config();
        assert_eq!(config.id, 146);
        assert_eq!(config.name, "SONIC");
        assert_eq!(config.description, "SONIC test chain");
        assert!(config.json_rpc.is_none());
        assert!(config.state_updates.is_none());
    }
}
