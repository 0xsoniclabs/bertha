use thiserror::Error;

use crate::transaction::TransactionType;

#[derive(Debug, PartialEq, Eq, Error)]
pub enum TransactionError {
    #[error("couldn't convert transaction to type {0}")]
    ConversionError(TransactionType),
}
