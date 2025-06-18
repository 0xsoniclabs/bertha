#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("conversion from generic representation to Rust type failed")]
    TypeConversion,
}
