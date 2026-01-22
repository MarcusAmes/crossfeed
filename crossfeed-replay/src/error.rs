use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("missing active version")]
    MissingActiveVersion,
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}
