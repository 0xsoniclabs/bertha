#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rlp decoding failed: {0}")]
    Rlp(#[from] alloy_rlp::Error),
    #[error("an io error occurred: {0}")]
    Io(#[from] std::io::Error),
    #[error("gzip decompression failed: {0}")]
    Decompression(#[from] flate2::DecompressError),
    #[error("`.g` file validation failed: {0}")]
    GFile(#[from] GFileError),
    #[error("era file parsing failed: {0}")]
    Era(String),
}

#[derive(Debug, thiserror::Error)]
pub enum GFileError {
    #[error("header missing")]
    HeaderMissing,
    #[error("invalid header: got {got:?}, expected {expected:?}")]
    InvalidHeader { got: [u8; 4], expected: [u8; 4] },
    #[error("header mismatch")]
    HeaderMismatch,
    #[error("invalid file version: got {got:?}, expected {expected:?}")]
    InvalidFileVersion { got: [u8; 4], expected: [u8; 4] },
    #[error("blocks unit missing")]
    BlocksUnitMissing,
    #[error("piece size too large: got {got}, max {max}")]
    PieceSizeTooLarge { got: usize, max: usize },
}
