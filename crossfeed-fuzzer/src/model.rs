use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FuzzTemplate {
    pub request_bytes: Vec<u8>,
    pub placeholders: Vec<Placeholder>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Placeholder {
    pub index: usize,
    pub token: String,
    pub ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Payload {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaceholderSpec {
    pub index: usize,
    pub payloads: Vec<Payload>,
    pub transforms: Vec<TransformStep>,
    pub prefix: Option<Vec<u8>>,
    pub suffix: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransformStep {
    UrlEncodeBytes,
    UrlEncodeStr,
    UrlDecodeBytes,
    UrlDecodeStr,
    Base64EncodeBytes,
    Base64EncodeStr,
    Base64DecodeBytes,
    Base64DecodeStr,
    Base64UrlEncodeBytes,
    Base64UrlEncodeStr,
    Base64UrlDecodeBytes,
    Base64UrlDecodeStr,
    HexEncodeBytes,
    HexEncodeStr,
    HexDecodeBytes,
    HexDecodeStr,
    Base32EncodeBytes,
    Base32EncodeStr,
    Base32DecodeBytes,
    Base32DecodeStr,
    Base58EncodeBytes,
    Base58EncodeStr,
    Base58DecodeBytes,
    Base58DecodeStr,
    HtmlEscapeStr,
    HtmlUnescapeStr,
    Rot13Str,
    GzipCompress,
    GzipDecompress,
    DeflateCompress,
    DeflateDecompress,
    Md5Hex,
    Md5Bytes,
    Sha1Hex,
    Sha1Bytes,
    Sha224Hex,
    Sha224Bytes,
    Sha256Hex,
    Sha256Bytes,
    Sha384Hex,
    Sha384Bytes,
    Sha512Hex,
    Sha512Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FuzzRunConfig {
    pub placeholder_prefix: String,
    pub concurrency: usize,
}

impl Default for FuzzRunConfig {
    fn default() -> Self {
        Self {
            placeholder_prefix: "<<CFUZZ".to_string(),
            concurrency: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisConfig {
    pub grep: Vec<String>,
    pub extract: Vec<String>,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            grep: Vec::new(),
            extract: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisResult {
    pub grep_matches: Vec<String>,
    pub extracts: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FuzzResult {
    pub timeline_request_id: i64,
    pub analysis: AnalysisResult,
}
