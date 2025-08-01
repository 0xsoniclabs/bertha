use std::{
    io::Cursor,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
};

use bertha_types::Block;
use blockservice::{
    cli::{Args, Command},
    cmd::{self, AddressBinder},
};
use tokio::select;
use tokio_util::sync::CancellationToken;

/// A server for integration tests which implements graceful shutdown
/// The implementation uses only the `blockservice` public interface.
pub struct IntegrationTestServer {
    addr: SocketAddr,
    cancellation_token: CancellationToken,
}

impl IntegrationTestServer {
    /// Creates and start a new [`IntegrationTestServer`] with the given app directory and imports
    /// the specified snapshot files in a separate task. It initializes the DB and config file,
    /// and starts the server on a random available port.
    pub async fn new(app_dir: &Path, import_files: Vec<PathBuf>) -> IntegrationTestServer {
        let (res, output) = execute_command(Command::Init {}, app_dir, None, None, None).await;
        res.expect("initialization should succeed");
        check_init_output(&output, app_dir);

        // Initialize the DB with the import files
        for file in import_files {
            let (res, _) = execute_command(
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
            res.expect("import should succeed");
        }

        let cancellation_token = CancellationToken::new();
        let address_binder = RandomAddressBinder::try_new()
            .await
            .expect("address binding should succeed");
        let addr = address_binder.local_addr();

        tokio::spawn({
            let cancellation_token = cancellation_token.clone();
            let app_dir = app_dir.to_path_buf();
            async move {
                let (res, _) = execute_command(
                    Command::Start,
                    app_dir,
                    Some(Cursor::new("")),
                    Some(cancellation_token),
                    Some(address_binder),
                )
                .await;
                res.expect("server should start successfully");
            }
        });

        IntegrationTestServer {
            addr,
            cancellation_token,
        }
    }

    /// Returns the URL of the server.
    pub fn uri(&self) -> String {
        uri(self.addr)
    }
}

impl Drop for IntegrationTestServer {
    /// Gracefully shuts down the server.
    /// NOTE: this will not stop request tasks spawned by Tonic as tasks spawned by other tasks are
    /// detached. To be able to kill also those tasks, the server must be spawned in a separate
    /// process.
    fn drop(&mut self) {
        self.cancellation_token.cancel();
    }
}

/// An [`AddressBinder`] that binds on a random available port.
pub struct RandomAddressBinder {
    listener: tokio::net::TcpListener,
}

impl RandomAddressBinder {
    /// Attempts to create a new [`RandomAddressBinder`] that binds to a random available port on
    /// the unspecified address. Returns a [`tokio::net::TcpListener`] bound to an unspecified IPv6
    /// address with a random port
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
impl AddressBinder for RandomAddressBinder {
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

/// Helper function to execute a command in the specified path with the specified input and capture
/// the result and output.
/// It optionally uses a cancellation token for graceful shutdown and an [`AddressBinder`] to
/// control the address the command binds to (if applicable).
pub async fn execute_command(
    command: Command,
    path: impl AsRef<Path>,
    input: Option<Cursor<&'static str>>,
    cancellation_token: Option<CancellationToken>,
    address_binder: Option<RandomAddressBinder>,
) -> (
    Result<(), Box<dyn std::error::Error + Send + Sync>>,
    Vec<u8>,
) {
    let mut output = vec![];
    let cancellation_token = cancellation_token.unwrap_or_default();
    let address_binder = match address_binder {
        Some(binder) => binder,
        None => RandomAddressBinder::try_new().await.unwrap(),
    };
    (
        select! {
            _ = cancellation_token.cancelled() => {
                Ok(())
            },
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
        },
        output,
    )
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

#[cfg(test)]
mod tests {

    use std::str::FromStr;

    use super::*;

    #[test]
    fn uri_constructs_correct_http_uri() {
        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 8080);
        let expected_uri = "http://[::]:8080";
        assert_eq!(uri(addr), expected_uri);
    }

    #[tokio::test]
    async fn integration_test_server_stops_on_drop() {
        let server_dir = tempfile::tempdir().unwrap();
        let server = IntegrationTestServer::new(server_dir.path(), vec![]).await;
        let server_url = server.uri();
        // Drop the server and check if it stops
        drop(server);
        // The server task should have been cancelled, so no further checks are needed

        let client_dir = tempfile::tempdir().unwrap();
        let (res, _) = execute_command(Command::Init {}, client_dir.path(), None, None, None).await;
        res.expect("Initialization should succeed");

        let (res, _) = execute_command(
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
            res.is_err(),
            "List command should fail after server is dropped"
        );
    }

    #[tokio::test]
    async fn integration_test_server_uri_returns_valid_uri() {
        let server_dir = tempfile::tempdir().unwrap();
        let server = IntegrationTestServer::new(server_dir.path(), vec![]).await;
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
    async fn integration_server_server_new_starts_server() {
        let server_dir = tempfile::tempdir().unwrap();
        let server = IntegrationTestServer::new(
            server_dir.path(),
            vec![make_snapshot_file(server_dir.path(), 1, 10, &[])],
        )
        .await;

        let client_dir = tempfile::tempdir().unwrap();
        let (res, _) = execute_command(Command::Init {}, client_dir.path(), None, None, None).await;
        res.expect("Initialization should succeed");
        let (res, output) = execute_command(
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
        res.expect("List command should succeed");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            indoc::indoc! {"
            [1] (no name): (no description)
            └── 0 - 9
            "},
        );
    }

    #[tokio::test]
    async fn make_snapshot_file_creates_correct_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let chain_id = 1;
        let num_blocks = 10;
        let extra_blocks = vec![Block::default_sonic(); 5];

        let snapshot_file =
            make_snapshot_file(temp_dir.path(), chain_id, num_blocks, &extra_blocks);
        assert!(snapshot_file.exists());
        assert_eq!(snapshot_file.file_name().unwrap(), "1_15_snapshot");
    }

    #[tokio::test]
    async fn execute_command_executes_command() {
        let temp_dir = tempfile::tempdir().unwrap();
        let (res, output) =
            execute_command(Command::Init {}, temp_dir.path(), None, None, None).await;
        res.expect("Initialization should succeed");
        assert_eq!(
            String::from_utf8_lossy(&output),
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
    async fn execute_handles_graceful_shutdown() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cancellation_token = CancellationToken::new();
        let address_binder = RandomAddressBinder::try_new()
            .await
            .expect("Address binding should succeed");
        let address = address_binder.local_addr();

        let (res, _) = execute_command(Command::Init, temp_dir.path(), None, None, None).await;
        res.expect("Initialization should succeed");

        let server_task = tokio::spawn({
            let cancellation_token = cancellation_token.clone();
            let path = temp_dir.path().to_path_buf();
            async move {
                let (res, _) = execute_command(
                    Command::Start,
                    path,
                    Some(Cursor::new("")),
                    Some(cancellation_token),
                    Some(address_binder),
                )
                .await;
                res.expect("IntegrationTestServer should start successfully");
            }
        });

        // Check if the server is running by executing a command
        let (res, _) = execute_command(
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
        assert!(res.is_ok(), "Command should complete successfully");

        // drop the server by canceling the cancellation token
        cancellation_token.cancel();
        select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                panic!("IntegrationTestServer task should complete before timeout");
            },
            res = server_task => {
                res.expect("IntegrationTestServer task should complete successfully");
            }
        }
    }
}
