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
    fmt::Write,
    net::{IpAddr, Ipv6Addr, SocketAddr},
};

pub use fetch::fetch;
pub use fetch_state_updates::fetch_state_updates;
pub use import::{import_era, import_era1, import_gfile};
use indicatif::{ProgressBar, style::TemplateError};
pub use init::init;
pub use list::list;
pub use purge::purge;
pub use start::start;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
pub use verify::verify;
pub use view::view;

use crate::{
    app_dir::open_app_dir,
    cli::{Args, Command},
    cmd,
    utils::InputReader,
};

mod fetch;
mod fetch_state_updates;
mod import;
mod init;
mod list;
mod purge;
mod start;
mod verify;
mod view;

/// Creates a new progress bar with a custom style and an ETA display.
pub fn make_progress_bar(total: u64) -> Result<ProgressBar, TemplateError> {
    let bar = ProgressBar::new(total);
    bar.set_style(
        indicatif::ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} (ETA {eta})",
        )?
        .with_key(
            "eta",
            |state: &indicatif::ProgressState, w: &mut dyn Write| {
                // Since there is no way of propagating errors from this closure,
                // we just ignore the result (worst case the ETA will not be shown).
                let _ = write!(w, "{:.1}s", state.eta().as_secs_f64());
            },
        )
        .progress_chars("#>-"),
    );
    Ok(bar)
}

/// A trait to bind an address to a [`TcpListener`].
/// Different implementations can choose different strategies for constructing the address, e.g. by
/// reading a config file.
#[tonic::async_trait]
pub trait AddressBinder {
    async fn bind_address(self) -> Result<TcpListener, Box<dyn std::error::Error + Send + Sync>>;
}

/// An [`AddressBinder`] that uses the config file to bind the address.
pub struct ConfigFileAddressBinder {
    config_dir: std::path::PathBuf,
}

impl ConfigFileAddressBinder {
    /// Creates a new [`ConfigFileAddressBinder`] with the specified config directory.
    pub fn new(config_dir: std::path::PathBuf) -> Self {
        Self { config_dir }
    }
}

#[tonic::async_trait]
impl AddressBinder for ConfigFileAddressBinder {
    /// Binds the address using the port specified in the config file.
    /// Returns a [`TcpListener`] bound to the specified address.
    async fn bind_address(self) -> Result<TcpListener, Box<dyn std::error::Error + Send + Sync>> {
        let port = {
            let (cfg, _db) = open_app_dir(self.config_dir.clone(), true)?;
            cfg.get_port()
        };

        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port);
        TcpListener::bind(addr).await.map_err(Into::into)
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait CancelIndicator: Send {
    fn cancel(&self);
    fn is_cancelled(&self) -> bool;
    fn cancelled(&self) -> impl Future<Output = ()> + Send;
}

impl CancelIndicator for CancellationToken {
    fn cancel(&self) {
        CancellationToken::cancel(self);
    }

    fn is_cancelled(&self) -> bool {
        CancellationToken::is_cancelled(self)
    }

    fn cancelled(&self) -> impl Future<Output = ()> + Send {
        CancellationToken::cancelled(self)
    }
}

/// Executes a command with the provided arguments.
/// Arguments:
/// - `args`: the command arguments.
/// - `cancellation_token`: a token for gracefully task shutdown.
/// - `address_binder`: an `AddressBinder` to bind the address for the server.
/// - `output`: an output writer for command results.
/// - `input`: an input reader for command inputs.
pub async fn execute(
    args: Args,
    cancellation_token: CancellationToken,
    address_binder: impl AddressBinder,
    mut output: impl std::io::Write,
    input: impl InputReader,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match args.command {
        Command::Init => cmd::init(args.dir, &mut output),
        Command::ImportGfile { gfile, verify } => {
            cmd::import_gfile(args.dir, gfile, verify, &cancellation_token, &mut output)
        }
        Command::ImportEra1 {
            era1_dir,
            chain_id,
            verify,
        } => cmd::import_era1(
            args.dir,
            era1_dir,
            chain_id,
            verify,
            &cancellation_token,
            &mut output,
        ),
        Command::ImportEra { era_dir, chain_id } => cmd::import_era(
            args.dir,
            era_dir,
            chain_id,
            &cancellation_token,
            &mut output,
        ),
        Command::List { chain_id, url } => cmd::list(args.dir, chain_id, url, &mut output).await,
        Command::Fetch {
            url,
            chain_id,
            from,
            to,
        } => cmd::fetch(args.dir, url, chain_id, from, to, &mut output).await,
        Command::Purge { chain_id, from, to } => {
            cmd::purge(args.dir, chain_id, from, to, &mut output, &input)
        }
        Command::Verify {
            chain_id,
            block_number,
            block_hash,
        } => cmd::verify(
            args.dir,
            chain_id,
            block_number,
            block_hash,
            &cancellation_token,
            &mut output,
        ),
        Command::View {
            chain_id,
            block_number,
        } => cmd::view(args.dir, chain_id, block_number, &mut output),
        Command::FetchStateUpdates { url, chain_id } => {
            cmd::fetch_state_updates(args.dir, url, chain_id, &mut output).await
        }
        Command::Start => {
            cmd::start(
                args.dir,
                address_binder.bind_address().await?,
                cancellation_token.clone(),
                None,
                None,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rand::{RngExt, SeedableRng, rngs::SmallRng};

    use super::*;
    use crate::{
        app_dir::{CONFIG_FILE_NAME, init_app_dir},
        utils::test_dir::{Permissions, TestDir},
    };

    fn write_port_cfg(app_dir: impl AsRef<Path>, port: u16) {
        let config_path = app_dir.as_ref().join(CONFIG_FILE_NAME);
        std::fs::write(&config_path, format!("port = {port}")).unwrap();
    }

    #[tokio::test]
    async fn config_file_address_binder_bind_address_binds_address_with_config_file_port() {
        let temp_dir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(temp_dir.path(), std::io::sink()).unwrap();
        let mut rng = SmallRng::seed_from_u64(123);
        loop {
            let port = rng.random_range(1024..u16::MAX);
            write_port_cfg(temp_dir.path(), port);
            let binder = ConfigFileAddressBinder::new(temp_dir.path().to_path_buf());
            let result = binder.bind_address().await;
            match result {
                Ok(listener) => {
                    assert_eq!(listener.local_addr().unwrap().port(), port);
                    break;
                }
                Err(_) => continue,
            }
        }
    }

    #[tokio::test]
    async fn config_file_address_binder_fails_for_non_existing_config_dir() {
        let non_existing_dir = std::path::PathBuf::from("/non/existing/dir");
        let binder = ConfigFileAddressBinder::new(non_existing_dir);
        let result = binder.bind_address().await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No such file or directory")
        );
    }

    #[tokio::test]
    async fn config_file_address_binder_fails_for_port_already_in_use() {
        let temp_dir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        init_app_dir(temp_dir.path(), std::io::sink()).unwrap();

        // Bind an address to be sure the port is already taken
        let listener = TcpListener::bind("[::]:0").await.unwrap();
        // port is already in use
        write_port_cfg(temp_dir.path(), listener.local_addr().unwrap().port());
        let binder = ConfigFileAddressBinder::new(temp_dir.path().to_path_buf());
        let result = binder.bind_address().await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Address already in use")
        );
    }
}
