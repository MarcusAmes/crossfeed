use thiserror::Error;

#[derive(Debug, Error)]
pub enum FuzzError {
    #[error("template error: {0}")]
    Template(String),
    #[error("transform error: {0}")]
    Transform(String),
    #[error("analysis error: {0}")]
    Analysis(String),
    #[error("storage error: {0}")]
    Storage(String),
}
