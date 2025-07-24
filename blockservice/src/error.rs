use prost::DecodeError;

use crate::app_dir::AppDirError;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("application directory error: {0}")]
    AppDir(#[from] AppDirError),
    #[error("error in underlying storage layer: {0}")]
    StorageLayer(String),
    #[error("error during protobuf decoding: {0}")]
    Protobuf(#[from] DecodeError),
    #[error("conversion from generic representation to Rust type failed")]
    TypeConversion,
    // std::io::Error is not PartialEq + Eq, so we cannot wrap it directly
    #[error("I/O error: {0}")]
    Io(String),
    #[error("configuration error: {0}")]
    Config(String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<toml::ser::Error> for Error {
    fn from(err: toml::ser::Error) -> Self {
        Error::Config(format!("TOML serialization failed: {err}"))
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Error::Config(format!("TOML deserialization failed: {err}"))
    }
}

impl From<toml_edit::TomlError> for Error {
    fn from(err: toml_edit::TomlError) -> Self {
        Error::Config(format!("TOML edit failed: {err}"))
    }
}
