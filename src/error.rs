#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("evolution failed: {0}")]
    Evolution(String),
}

pub type Result<T> = std::result::Result<T, Error>;
