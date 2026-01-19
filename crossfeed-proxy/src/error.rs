use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("proxy configuration error: {0}")]
    Config(String),
    #[error("proxy runtime error: {0}")]
    Runtime(String),
    #[error("proxy IO error: {0}")]
    Io(#[from] std::io::Error),
}
