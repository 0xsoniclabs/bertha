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
    /// It imports the specified snapshot files and optionally saves some chain configurations in
    /// the local config file.
    /// The server is started on a random available port.
    pub async fn new(
        app_dir: impl AsRef<Path>,
        import_files: Vec<PathBuf>,
        chain_configs: Option<Vec<ChainConfig>>,
    ) -> IntegrationTestServer {
        init_blockservice(Some(app_dir.as_ref()), &chain_configs.unwrap_or_default())
            .await
            .expect("blockservice should initialize");

        // Initialize the DB with the import files
        for file in import_files {
            let CommandExecutionOutput { result, log: _ } = execute_command(
                Command::ImportGfile {
                    gfile: file,
                    verify: false,
                },
                app_dir.as_ref(),
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
            let app_dir = app_dir.as_ref().to_path_buf();
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

/// Helper function to initialize a blockservice instance in the specified path, or in a temporary
/// directory if no path is provided.
/// It sets up the configuration file with the provided chain
/// configurations.
pub async fn init_blockservice(
    path: Option<&Path>,
    chain_configs: &[ChainConfig],
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let dir = match path {
        Some(p) => p.to_path_buf(),
        None => tempfile::tempdir()?.keep(),
    };
    let CommandExecutionOutput { result, .. } =
        execute_command(Command::Init {}, &dir, None, None, None).await;
    result?;
    add_chain_configs_to_config_file(chain_configs, &dir)?;
    Ok(dir)
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

/// Adds the chain configurations to the config file at the specified path.
pub fn add_chain_configs_to_config_file(
    chain_configs: &[ChainConfig],
    path: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config_path = path.as_ref().join(CONFIG_FILE_NAME);
    let mut config = Config::load(&config_path)?;

    for chain_config in chain_configs {
        config.add_chain(chain_config.clone())?;
    }

    Ok(())
}

/// Creates a default chain configuration for the SONIC chain.
pub fn make_default_sonic_chain_config() -> ChainConfig {
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

    use std::{fs::File, io::BufReader, slice, str::FromStr};

    use blockservice::app_dir::BLOCK_DB_NAME;
    use genesis_parser::GFile;

    use super::*;

    #[tokio::test]
    async fn init_blockservice_initializes_correctly() {
        let temp_dir = tempfile::tempdir().unwrap();
        let dir = init_blockservice(
            Some(temp_dir.path()),
            &[ChainConfig {
                description: "A Test Chain".to_string(),
                id: 1,
                name: "Test Chain".to_string(),
                json_rpc: None,
                state_updates: None,
            }],
        )
        .await
        .expect("blockservice should initialize");
        assert!(dir.exists(), "Directory should be created");
        assert!(
            dir.join(CONFIG_FILE_NAME).exists(),
            "Config file should be created"
        );
        assert!(
            dir.join(BLOCK_DB_NAME).exists(),
            "Block database should be created"
        );

        // Check the config file is correctly initialized
        let config_path = dir.join(CONFIG_FILE_NAME);
        let config = Config::load(&config_path).expect("Config should load successfully");
        let chain_configs = config.get_chain_configs();
        assert_eq!(chain_configs.len(), 1);
        assert_eq!(chain_configs[0].id, 1);
        assert_eq!(chain_configs[0].name, "Test Chain");
        assert_eq!(chain_configs[0].description, "A Test Chain");
    }

    #[tokio::test]
    async fn init_blockservice_succeeds_if_dir_is_already_initialized() {
        let temp_dir = tempfile::tempdir().unwrap();
        let dir = temp_dir.path();
        init_blockservice(Some(dir), &[])
            .await
            .expect("blockservice should initialize");

        let result = init_blockservice(Some(dir), &[]).await;
        assert!(result.is_ok(), "Re-initialization should succeed");
    }

    #[tokio::test]
    async fn init_blockservice_fails_if_chain_config_already_exists() {
        let chain_config = ChainConfig {
            id: 1,
            name: "Test Chain".to_string(),
            description: "A Test Chain".to_string(),
            json_rpc: None,
            state_updates: None,
        };
        let temp_dir = tempfile::tempdir().unwrap();
        let dir = temp_dir.path();
        init_blockservice(Some(dir), slice::from_ref(&chain_config))
            .await
            .expect("blockservice should initialize");

        // Try to add the same chain config again, it should fail
        let result = add_chain_configs_to_config_file(&[chain_config], dir);
        assert!(result.is_err(), "Adding existing chain config should fail");
    }

    #[tokio::test]
    async fn integration_test_server_new_starts_server() {
        const CHAIN_ID: u64 = 146;
        let server_dir = tempfile::tempdir().unwrap();
        let server = IntegrationTestServer::new(
            server_dir.path(),
            vec![make_snapshot_file(server_dir.path(), CHAIN_ID, 10, &[])],
            Some(vec![make_default_sonic_chain_config()]),
        )
        .await;

        // Check the config file is correctly initialized
        let config_path = server_dir.path().join(CONFIG_FILE_NAME);
        let config = Config::load(&config_path).expect("Config should load successfully");
        let chain_configs = config.get_chain_configs();
        assert_eq!(chain_configs.len(), 1); // One for SONIC and one for the example one
        assert_eq!(chain_configs[0].id, CHAIN_ID);
        assert_eq!(chain_configs[0].name, "SONIC");
        assert_eq!(chain_configs[0].description, "SONIC test chain");

        // Query the server to check if it is running
        let client_dir = tempfile::tempdir().unwrap();
        let CommandExecutionOutput { result, log: _ } =
            execute_command(Command::Init {}, client_dir.path(), None, None, None).await;
        result.expect("Initialization should succeed");
        let CommandExecutionOutput { result, log } = execute_command(
            Command::List {
                chain_id: Some(CHAIN_ID),
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
            [146] SONIC: SONIC test chain
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
                chain_id: None,
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

    #[test]
    fn uri_constructs_correct_http_uri() {
        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 8080);
        let expected_uri = "http://[::]:8080";
        assert_eq!(uri(addr), expected_uri);
    }

    #[tokio::test]
    async fn make_snapshot_file_creates_correct_file() {
        const CHAIN_ID: u64 = 146;
        let temp_dir = tempfile::tempdir().unwrap();
        let num_blocks = 10;
        let extra_blocks = vec![Block::default_sonic(); 5];

        let snapshot_path =
            make_snapshot_file(temp_dir.path(), CHAIN_ID, num_blocks, &extra_blocks);
        assert!(snapshot_path.exists());
        assert_eq!(
            snapshot_path.file_name().unwrap(),
            format!("{CHAIN_ID}_15_snapshot").as_str()
        );

        // Parse the genesis file to verify its contents
        let file = File::open(snapshot_path).unwrap();
        let mut reader = BufReader::new(file);
        let mut genesis = GFile::parse(&mut reader).expect("Genesis should parse successfully");

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
            Initializing blockservice directory at: {path}
            Creating new configuration at: {path}/blockservice.toml
            Creating new block database at: {path}/.blockdb
            ",
                path = temp_dir.path().display(),
            }
        );
    }

    #[tokio::test]
    async fn execute_terminates_command_on_cancellation_token_cancellation() {
        const CHAIN_ID: u64 = 146;
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
                chain_id: Some(CHAIN_ID),
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
    fn add_chain_configs_to_config_file_saves_configs() {
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

        add_chain_configs_to_config_file(&chain_configs, temp_dir.path()).unwrap();

        let config = Config::load(config_path).unwrap();
        let res_chain_configs = config.get_chain_configs();
        assert_eq!(res_chain_configs.len(), 2);
        assert_eq!(res_chain_configs[0], chain_configs[0]);
        assert_eq!(res_chain_configs[1], chain_configs[1]);
    }

    #[test]
    fn add_chain_configs_to_config_file_fails_if_config_not_found() {
        let temp_dir = tempfile::tempdir().unwrap();
        // The config file does not exist, so it should return an error
        let result = add_chain_configs_to_config_file(&[], temp_dir.path());
        assert!(
            result.is_err(),
            "adding chain configs should fail if config file does not exist"
        );
    }

    #[test]
    fn add_chain_configs_to_config_file_fails_if_config_is_invalid() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE_NAME);
        // Create an invalid config file
        std::fs::write(&config_path, "invalid config").unwrap();

        let result = add_chain_configs_to_config_file(&[], temp_dir.path());
        assert!(
            result.is_err(),
            "adding chain configs should fail with invalid config"
        );
    }

    #[test]
    fn add_chain_configs_to_config_file_fails_if_config_already_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE_NAME);
        {
            Config::create_default(&config_path).unwrap();
        }

        // Add a chain config to the file
        let chain_config = ChainConfig {
            id: 1,
            name: "Chain1".to_string(),
            description: "Test Chain 1".to_string(),
            json_rpc: None,
            state_updates: None,
        };
        add_chain_configs_to_config_file(slice::from_ref(&chain_config), temp_dir.path())
            .expect("adding chain config should succeed");

        // Try to add the same chain config again, which should fail
        let result = add_chain_configs_to_config_file(&[chain_config], temp_dir.path());
        assert!(result.is_err(), "adding existing chain config should fail");
    }

    #[test]
    fn make_default_sonic_chain_config_returns_correct_config() {
        let config = make_default_sonic_chain_config();
        assert_eq!(config.id, 146);
        assert_eq!(config.name, "SONIC");
        assert_eq!(config.description, "SONIC test chain");
        assert!(config.json_rpc.is_none());
        assert!(config.state_updates.is_none());
    }
}
