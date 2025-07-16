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
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}
