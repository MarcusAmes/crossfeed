use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("invalid base64: {0}")]
    Base64(String),
    #[error("invalid base64url: {0}")]
    Base64Url(String),
    #[error("invalid base32: {0}")]
    Base32(String),
    #[error("invalid base58: {0}")]
    Base58(String),
    #[error("invalid hex: {0}")]
    Hex(String),
    #[error("invalid url encoding: {0}")]
    Url(String),
    #[error("invalid utf-8: {0}")]
    Utf8(String),
    #[error("compression error: {0}")]
    Compression(String),
}
