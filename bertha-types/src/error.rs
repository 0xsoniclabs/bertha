use thiserror::Error;

use crate::types::ReceiptVerificationError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("network error: {0}")]
    Rpc(#[from] jsonrpsee::core::client::Error),
    #[error("(de-)serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("receipt verification failed: {0}")]
    Verification(#[from] ReceiptVerificationError),
    #[error("failed to get majority")]
    NoMajority,
    #[error("source list cannot be empty")]
    EmptySourceList,
}
