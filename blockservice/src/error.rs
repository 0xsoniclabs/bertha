use prost::DecodeError;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("error in underlying storage layer: {0}")]
    StorageLayer(String),
    #[error("error during protobuf decoding: {0}")]
    Protobuf(#[from] DecodeError),
    #[error("conversion from generic representation to Rust type failed")]
    TypeConversion,
    #[error("I/O error")]
    Io,
}
