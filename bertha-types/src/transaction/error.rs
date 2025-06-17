use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Error)]
pub enum TransactionError {
    #[error("couldn't convert transaction: {0}")]
    ConversionError(String),
    #[error("invalid transaction type: {0}")]
    InvalidTransactionType(u8),
}
