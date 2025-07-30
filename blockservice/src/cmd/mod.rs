use std::{
    fmt::Write,
    net::{IpAddr, Ipv6Addr, SocketAddr},
};

pub use fetch::fetch;
pub use fetch_state_updates::fetch_state_updates;
pub use import::import;
use indicatif::{ProgressBar, style::TemplateError};
pub use init::init;
pub use list::list;
pub use purge::purge;
pub use start::start;
use tokio::net::TcpListener;
pub use verify::verify;
pub use view::view;

use crate::app_dir::open_app_dir;

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

/// Interface for binding an address to a TCP listener.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_dir::{CONFIG_FILE_NAME, init_app_dir};

    #[tokio::test]
    async fn config_file_address_binder_bind_address_binds_address_with_config_file_port() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_app_dir(temp_dir.path(), std::io::sink()).unwrap();
        let binder = ConfigFileAddressBinder::new(temp_dir.path().to_path_buf());
        let listener = binder.bind_address().await.unwrap();
        assert_eq!(listener.local_addr().unwrap().port(), 8080); // Default port
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
    async fn config_file_address_binder_fails_for_invalid_port() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_app_dir(temp_dir.path(), std::io::sink()).unwrap();

        // Bind an address to be sure the port is already taken
        let listener = TcpListener::bind("[::]:0").await.unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE_NAME);
        std::fs::write(
            &config_path,
            format!("port = {}", listener.local_addr().unwrap().port()),
        )
        .unwrap(); // port is already in use
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
