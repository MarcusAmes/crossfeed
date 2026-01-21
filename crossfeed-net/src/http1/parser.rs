use super::types::{
    Header, HttpVersion, Limits, ParseError, ParseErrorKind, ParseWarning, ParseWarningKind,
    Request, RequestLine, Response, StatusLine,
};

const CRLF: &[u8] = b"\r\n";
const HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseStatus<T> {
    NeedMore {
        warnings: Vec<ParseWarning>,
    },
    Complete {
        message: T,
        warnings: Vec<ParseWarning>,
    },
    Error {
        error: ParseError,
        warnings: Vec<ParseWarning>,
    },
}

#[derive(Debug, Default)]
pub struct RequestParser {
    buffer: Vec<u8>,
    warnings: Vec<ParseWarning>,
    limits: Limits,
}

impl RequestParser {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        Self {
            buffer: Vec::new(),
            warnings: Vec::new(),
            limits,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> ParseStatus<Request> {
        self.buffer.extend_from_slice(bytes);
        self.try_parse_request()
    }

    fn try_parse_request(&mut self) -> ParseStatus<Request> {
        match parse_request_from_buffer(&self.buffer, self.limits, &mut self.warnings) {
            Ok(ParseResult::Complete { message, consumed }) => {
                self.buffer.drain(..consumed);
                let warnings = std::mem::take(&mut self.warnings);
                ParseStatus::Complete { message, warnings }
            }
            Ok(ParseResult::NeedMore) => ParseStatus::NeedMore {
                warnings: self.warnings.clone(),
            },
            Err(error) => {
                let warnings = std::mem::take(&mut self.warnings);
                ParseStatus::Error { error, warnings }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct ResponseParser {
    buffer: Vec<u8>,
    warnings: Vec<ParseWarning>,
    limits: Limits,
}

impl ResponseParser {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        Self {
            buffer: Vec::new(),
            warnings: Vec::new(),
            limits,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> ParseStatus<Response> {
        self.buffer.extend_from_slice(bytes);
        self.try_parse_response()
    }

    fn try_parse_response(&mut self) -> ParseStatus<Response> {
        match parse_response_from_buffer(&self.buffer, self.limits, &mut self.warnings) {
            Ok(ParseResult::Complete { message, consumed }) => {
                self.buffer.drain(..consumed);
                let warnings = std::mem::take(&mut self.warnings);
                ParseStatus::Complete { message, warnings }
            }
            Ok(ParseResult::NeedMore) => ParseStatus::NeedMore {
                warnings: self.warnings.clone(),
            },
            Err(error) => {
                let warnings = std::mem::take(&mut self.warnings);
                ParseStatus::Error { error, warnings }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ParseResult<T> {
    NeedMore,
    Complete { message: T, consumed: usize },
}

fn parse_request_from_buffer(
    buffer: &[u8],
    limits: Limits,
    warnings: &mut Vec<ParseWarning>,
) -> Result<ParseResult<Request>, ParseError> {
    let headers_end = match find_headers_end(buffer, limits, warnings)? {
        Some(index) => index,
        None => return Ok(ParseResult::NeedMore),
    };

    let mut cursor = 0;
    let line_end = find_line_end(buffer, cursor).ok_or(ParseError {
        kind: ParseErrorKind::UnexpectedEof,
        offset: buffer.len(),
    })?;
    let line = parse_request_line(&buffer[cursor..line_end], cursor, warnings)?;
    cursor = line_end + CRLF.len();

    let headers = parse_headers(&buffer[cursor..headers_end], cursor, warnings)?;
    cursor = headers_end + HEADER_TERMINATOR.len();

    let (body, body_consumed) = parse_body(buffer, cursor, limits, warnings)?;
    cursor += body_consumed;

    Ok(ParseResult::Complete {
        message: Request {
            line,
            headers,
            body,
        },
        consumed: cursor,
    })
}

fn parse_response_from_buffer(
    buffer: &[u8],
    limits: Limits,
    warnings: &mut Vec<ParseWarning>,
) -> Result<ParseResult<Response>, ParseError> {
    let headers_end = match find_headers_end(buffer, limits, warnings)? {
        Some(index) => index,
        None => return Ok(ParseResult::NeedMore),
    };

    let mut cursor = 0;
    let line_end = find_line_end(buffer, cursor).ok_or(ParseError {
        kind: ParseErrorKind::UnexpectedEof,
        offset: buffer.len(),
    })?;
    let line = parse_status_line(&buffer[cursor..line_end], cursor, warnings)?;
    cursor = line_end + CRLF.len();

    let headers = parse_headers(&buffer[cursor..headers_end], cursor, warnings)?;
    cursor = headers_end + HEADER_TERMINATOR.len();

    let (body, body_consumed) = parse_body(buffer, cursor, limits, warnings)?;
    cursor += body_consumed;

    Ok(ParseResult::Complete {
        message: Response {
            line,
            headers,
            body,
        },
        consumed: cursor,
    })
}

fn find_headers_end(
    buffer: &[u8],
    limits: Limits,
    _warnings: &mut Vec<ParseWarning>,
) -> Result<Option<usize>, ParseError> {
    match twoway::find_bytes(buffer, HEADER_TERMINATOR) {
        Some(index) => {
            if index > limits.max_header_bytes {
                return Err(ParseError {
                    kind: ParseErrorKind::HeaderTooLarge,
                    offset: limits.max_header_bytes,
                });
            }
            Ok(Some(index))
        }
        None => {
            if buffer.len() > limits.max_header_bytes {
                return Err(ParseError {
                    kind: ParseErrorKind::HeaderTooLarge,
                    offset: limits.max_header_bytes,
                });
            }
            Ok(None)
        }
    }
}

fn find_line_end(buffer: &[u8], start: usize) -> Option<usize> {
    twoway::find_bytes(&buffer[start..], CRLF).map(|offset| start + offset)
}

fn parse_request_line(
    line: &[u8],
    offset: usize,
    warnings: &mut Vec<ParseWarning>,
) -> Result<RequestLine, ParseError> {
    let text = std::str::from_utf8(line).map_err(|_| ParseError {
        kind: ParseErrorKind::InvalidStartLine,
        offset,
    })?;

    let mut parts = text.split_whitespace();
    let method = parts.next().ok_or(ParseError {
        kind: ParseErrorKind::InvalidStartLine,
        offset,
    })?;
    let target = parts.next().ok_or(ParseError {
        kind: ParseErrorKind::InvalidStartLine,
        offset,
    })?;
    let version_raw = parts.next().unwrap_or("HTTP/1.1");

    if parts.next().is_some() {
        return Err(ParseError {
            kind: ParseErrorKind::InvalidStartLine,
            offset,
        });
    }

    let version = parse_http_version(version_raw, offset, warnings);

    Ok(RequestLine {
        method: method.to_string(),
        target: target.to_string(),
        version,
    })
}

fn parse_status_line(
    line: &[u8],
    offset: usize,
    warnings: &mut Vec<ParseWarning>,
) -> Result<StatusLine, ParseError> {
    let text = std::str::from_utf8(line).map_err(|_| ParseError {
        kind: ParseErrorKind::InvalidStatusLine,
        offset,
    })?;

    let mut parts = text.splitn(3, ' ');
    let version_raw = parts.next().unwrap_or("HTTP/1.1");
    let status_raw = parts.next().ok_or(ParseError {
        kind: ParseErrorKind::InvalidStatusLine,
        offset,
    })?;
    let reason = parts.next().unwrap_or("");

    let status_code = status_raw.parse::<u16>().map_err(|_| ParseError {
        kind: ParseErrorKind::InvalidStatusLine,
        offset,
    })?;

    let version = parse_http_version(version_raw, offset, warnings);

    Ok(StatusLine {
        version,
        status_code,
        reason: reason.to_string(),
    })
}

fn parse_http_version(
    version_raw: &str,
    offset: usize,
    warnings: &mut Vec<ParseWarning>,
) -> HttpVersion {
    match version_raw {
        "HTTP/1.0" => HttpVersion::Http10,
        "HTTP/1.1" => HttpVersion::Http11,
        other => {
            warnings.push(ParseWarning {
                kind: ParseWarningKind::UnknownVersion(other.to_string()),
                offset,
            });
            HttpVersion::Other(other.to_string())
        }
    }
}

fn parse_headers(
    bytes: &[u8],
    base_offset: usize,
    warnings: &mut Vec<ParseWarning>,
) -> Result<Vec<Header>, ParseError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let text = std::str::from_utf8(bytes).map_err(|_| ParseError {
        kind: ParseErrorKind::InvalidStartLine,
        offset: base_offset,
    })?;

    let mut headers = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_raw_name: Option<String> = None;
    let mut current_value = String::new();
    let mut offset = base_offset;

    for line in text.split("\r\n") {
        if line.is_empty() {
            continue;
        }

        if let Some(first) = line.as_bytes().first() {
            if *first == b' ' || *first == b'\t' {
                warnings.push(ParseWarning {
                    kind: ParseWarningKind::ObsFoldDetected,
                    offset,
                });
                if current_name.is_some() {
                    current_value.push(' ');
                    current_value.push_str(line.trim());
                    offset += line.len() + CRLF.len();
                    continue;
                }
            }
        }

        if let Some(name) = current_name.take() {
            headers.push(Header {
                name,
                raw_name: current_raw_name.take().unwrap_or_default(),
                value: current_value.trim().to_string(),
            });
            current_value.clear();
        }

        let mut parts = line.splitn(2, ':');
        let raw_name = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");

        if raw_name.trim().is_empty() {
            warnings.push(ParseWarning {
                kind: ParseWarningKind::InvalidHeaderName,
                offset,
            });
        }

        if !value.is_empty()
            && value
                .as_bytes()
                .iter()
                .any(|byte| *byte == b'\r' || *byte == b'\n')
        {
            warnings.push(ParseWarning {
                kind: ParseWarningKind::InvalidHeaderValue,
                offset,
            });
        }

        current_name = Some(raw_name.trim().to_string());
        current_raw_name = Some(raw_name.to_string());
        current_value.push_str(value.trim_start());
        offset += line.len() + CRLF.len();
    }

    if let Some(name) = current_name {
        headers.push(Header {
            name,
            raw_name: current_raw_name.unwrap_or_default(),
            value: current_value.trim().to_string(),
        });
    }

    Ok(headers)
}

fn parse_body(
    buffer: &[u8],
    body_start: usize,
    limits: Limits,
    _warnings: &mut Vec<ParseWarning>,
) -> Result<(Vec<u8>, usize), ParseError> {
    let headers_bytes = &buffer[..body_start];
    let headers_text = std::str::from_utf8(headers_bytes).unwrap_or("");
    let headers_lower = headers_text.to_ascii_lowercase();

    if let Some(length) = parse_content_length(&headers_lower) {
        let total_needed = body_start + length;
        if length > limits.max_body_bytes {
            return Err(ParseError {
                kind: ParseErrorKind::BodyTooLarge,
                offset: body_start,
            });
        }
        if buffer.len() < total_needed {
            return Err(ParseError {
                kind: ParseErrorKind::UnexpectedEof,
                offset: buffer.len(),
            });
        }
        let body = buffer[body_start..total_needed].to_vec();
        return Ok((body, length));
    }

    if has_chunked_transfer_encoding(headers_text) {
        return parse_chunked_body(buffer, body_start, limits);
    }

    Ok((Vec::new(), 0))
}

fn parse_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        if let Some(value) = line.strip_prefix("content-length:") {
            return value.trim().parse::<usize>().ok();
        }
        None
    })
}

fn has_chunked_transfer_encoding(headers: &str) -> bool {
    for line in headers.lines() {
        let line = line.trim_end_matches('\r');
        let mut parts = line.splitn(2, ':');
        let name = parts.next().unwrap_or("").trim();
        if !name.eq_ignore_ascii_case("transfer-encoding") {
            continue;
        }
        let value = parts.next().unwrap_or("");
        if value
            .split(',')
            .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
        {
            return true;
        }
    }
    false
}

fn parse_chunked_body(
    buffer: &[u8],
    body_start: usize,
    limits: Limits,
) -> Result<(Vec<u8>, usize), ParseError> {
    let mut cursor = body_start;
    let mut body = Vec::new();

    loop {
        let line_end = find_line_end(buffer, cursor).ok_or(ParseError {
            kind: ParseErrorKind::UnexpectedEof,
            offset: buffer.len(),
        })?;
        let line = std::str::from_utf8(&buffer[cursor..line_end]).map_err(|_| ParseError {
            kind: ParseErrorKind::InvalidChunkSize,
            offset: cursor,
        })?;
        let chunk_size = line
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches("0x");

        let size = usize::from_str_radix(chunk_size, 16).map_err(|_| ParseError {
            kind: ParseErrorKind::InvalidChunkSize,
            offset: cursor,
        })?;
        cursor = line_end + CRLF.len();

        if size == 0 {
            if buffer.len() < cursor + CRLF.len() {
                return Err(ParseError {
                    kind: ParseErrorKind::InvalidChunkTerminator,
                    offset: cursor,
                });
            }
            cursor += CRLF.len();
            break;
        }

        let next = cursor + size;
        if next + CRLF.len() > buffer.len() {
            return Err(ParseError {
                kind: ParseErrorKind::UnexpectedEof,
                offset: buffer.len(),
            });
        }
        if body.len() + size > limits.max_body_bytes {
            return Err(ParseError {
                kind: ParseErrorKind::BodyTooLarge,
                offset: cursor,
            });
        }
        body.extend_from_slice(&buffer[cursor..next]);
        cursor = next;

        if &buffer[cursor..cursor + CRLF.len()] != CRLF {
            return Err(ParseError {
                kind: ParseErrorKind::InvalidChunkTerminator,
                offset: cursor,
            });
        }
        cursor += CRLF.len();
    }

    Ok((body, cursor - body_start))
}

#[cfg(test)]
mod tests {
    use super::{ParseStatus, RequestParser, ResponseParser};
    use crate::http1::{Limits, ParseWarningKind};

    #[test]
    fn parses_http10_request() {
        let mut parser = RequestParser::new();
        let input = b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n";
        let status = parser.push(input);

        match status {
            ParseStatus::Complete { message, .. } => {
                assert_eq!(message.line.method, "GET");
                assert_eq!(message.line.target, "/");
                assert_eq!(message.headers.len(), 1);
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn parses_request_across_buffers() {
        let mut parser = RequestParser::new();
        let part1 = b"GET /abc HTTP/1.1\r\nHost:";
        let part2 = b" example.com\r\nUser-Agent: test\r\n\r\n";

        let status = parser.push(part1);
        assert!(matches!(status, ParseStatus::NeedMore { .. }));

        let status = parser.push(part2);
        match status {
            ParseStatus::Complete { message, .. } => {
                assert_eq!(message.line.target, "/abc");
                assert_eq!(message.headers.len(), 2);
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn parses_content_length_body() {
        let mut parser = RequestParser::new();
        let input = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let status = parser.push(input);

        match status {
            ParseStatus::Complete { message, .. } => {
                assert_eq!(message.body, b"hello");
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn parses_chunked_response() {
        let mut parser = ResponseParser::new();
        let input = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let status = parser.push(input);

        match status {
            ParseStatus::Complete { message, .. } => {
                assert_eq!(message.body, b"hello");
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn parses_absolute_form_target() {
        let mut parser = RequestParser::new();
        let input = b"GET http://example.com/path HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let status = parser.push(input);

        match status {
            ParseStatus::Complete { message, .. } => {
                assert_eq!(message.line.target, "http://example.com/path");
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn warns_on_obs_fold() {
        let mut parser = RequestParser::new();
        let input = b"GET / HTTP/1.1\r\nHeader: one\r\n\tcontinued\r\n\r\n";
        let status = parser.push(input);

        match status {
            ParseStatus::Complete { warnings, .. } => {
                assert!(
                    warnings
                        .iter()
                        .any(|warning| matches!(warning.kind, ParseWarningKind::ObsFoldDetected))
                );
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn supports_header_limit() {
        let mut parser = RequestParser::with_limits(Limits {
            max_header_bytes: 10,
            max_body_bytes: 1024,
        });
        let input = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let status = parser.push(input);

        assert!(matches!(status, ParseStatus::Error { .. }));
    }

    #[test]
    fn warns_on_unknown_version() {
        let mut parser = ResponseParser::new();
        let input = b"HTTP/9.9 200 OK\r\nContent-Length: 0\r\n\r\n";
        let status = parser.push(input);

        match status {
            ParseStatus::Complete { warnings, .. } => {
                assert!(
                    warnings
                        .iter()
                        .any(|warning| matches!(warning.kind, ParseWarningKind::UnknownVersion(_)))
                );
            }
            other => panic!("unexpected status {other:?}"),
        }
    }
}
