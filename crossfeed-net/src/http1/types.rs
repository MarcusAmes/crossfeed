#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
    pub raw_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLine {
    pub method: String,
    pub target: String,
    pub version: HttpVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub version: HttpVersion,
    pub status_code: u16,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpVersion {
    Http10,
    Http11,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub line: RequestLine,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub line: StatusLine,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_header_bytes: 64 * 1024,
            max_body_bytes: 10 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseWarning {
    pub kind: ParseWarningKind,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseWarningKind {
    UnknownVersion(String),
    ObsFoldDetected,
    InvalidHeaderName,
    InvalidHeaderValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
    InvalidStartLine,
    InvalidStatusLine,
    HeaderTooLarge,
    BodyTooLarge,
    InvalidChunkSize,
    InvalidChunkTerminator,
    UnexpectedEof,
}
