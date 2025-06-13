use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VerificationError {
    #[error("the computed transaction root did not match the expected one")]
    TransactionVerificationError,
    #[error("the computed receipt root did not match the expected one")]
    ReceiptVerificationError,
}
