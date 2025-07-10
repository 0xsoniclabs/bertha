#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Rpc(#[from] jsonrpsee::core::client::Error),
    #[error("(de-)serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("the queried data was not found")]
    NotFound,
}
