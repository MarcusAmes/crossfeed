use super::{Header, HttpVersion, Limits, ParseError, ParseErrorKind};

const CRLF: &[u8] = b"\r\n";
const HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";

#[derive(Debug, Clone)]
pub struct ResponseFrameInfo {
    pub version: HttpVersion,
    pub status_code: u16,
    pub headers: Vec<Header>,
    pub content_length: Option<usize>,
    pub chunked: bool,
    pub close_delimited: bool,
    pub connection_close: bool,
}

#[derive(Debug, Clone)]
pub struct RequestFrameInfo {
    pub method: String,
    pub target: String,
    pub version: HttpVersion,
    pub headers: Vec<Header>,
    pub content_length: Option<usize>,
    pub chunked: bool,
    pub close_delimited: bool,
    pub connection_close: bool,
}

#[derive(Debug, Clone)]
pub enum ResponseStreamEvent {
    Headers(ResponseFrameInfo),
    BodyBytes { len: usize },
    EndOfMessage,
}

#[derive(Debug, Clone)]
pub enum RequestStreamEvent {
    Headers(RequestFrameInfo),
    BodyBytes { len: usize },
    EndOfMessage,
    ExpectContinue,
}

pub struct ResponseStreamParser {
    state: MessageState,
    buffer: Vec<u8>,
    limits: Limits,
    chunk_state: ChunkState,
    remaining: usize,
    close_delimited: bool,
}

pub struct RequestStreamParser {
    state: MessageState,
    buffer: Vec<u8>,
    limits: Limits,
    chunk_state: ChunkState,
    remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageState {
    Headers,
    Body,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChunkState {
    Size { line: Vec<u8> },
    Data { remaining: usize },
    DataCrlf { remaining: usize },
    Trailer { line: Vec<u8> },
    Done,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyMode {
    ContentLength,
    Chunked,
    CloseDelimited,
    NoBody,
}

impl ResponseStreamParser {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        Self {
            state: MessageState::Headers,
            buffer: Vec::new(),
            limits,
            chunk_state: ChunkState::None,
            remaining: 0,
            close_delimited: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<ResponseStreamEvent>, ParseError> {
        let mut events = Vec::new();
        let mut cursor = 0;

        while cursor < bytes.len() {
            match self.state {
                MessageState::Headers => {
                    self.buffer.extend_from_slice(&bytes[cursor..]);
                    if self.buffer.len() > self.limits.max_header_bytes {
                        return Err(ParseError {
                            kind: ParseErrorKind::HeaderTooLarge,
                            offset: self.limits.max_header_bytes,
                        });
                    }
                    let Some(header_end) = find_header_end(&self.buffer) else {
                        break;
                    };
                    let header_bytes = self.buffer[..header_end].to_vec();
                    let body_start = header_end + HEADER_TERMINATOR.len();
                    let body_bytes = self.buffer[body_start..].to_vec();
                    self.buffer.clear();

                    let (frame_info, body_mode) = parse_response_headers(&header_bytes)?;
                    self.close_delimited = frame_info.close_delimited;
                    events.push(ResponseStreamEvent::Headers(frame_info.clone()));

                    match body_mode {
                        BodyMode::NoBody => {
                            events.push(ResponseStreamEvent::EndOfMessage);
                            self.state = MessageState::Done;
                        }
                        BodyMode::ContentLength => {
                            self.remaining = frame_info.content_length.unwrap_or(0);
                            self.chunk_state = ChunkState::None;
                            self.state = MessageState::Body;
                        }
                        BodyMode::Chunked => {
                            self.chunk_state = ChunkState::Size { line: Vec::new() };
                            self.state = MessageState::Body;
                        }
                        BodyMode::CloseDelimited => {
                            self.chunk_state = ChunkState::None;
                            self.state = MessageState::Body;
                        }
                    }

                    cursor = bytes.len();
                    if !body_bytes.is_empty() && self.state == MessageState::Body {
                        let body_events = self.consume_body(&body_bytes)?;
                        events.extend(body_events);
                    }
                }
                MessageState::Body => {
                    let body_events = self.consume_body(&bytes[cursor..])?;
                    events.extend(body_events);
                    cursor = bytes.len();
                }
                MessageState::Done => break,
            }
        }

        Ok(events)
    }

    pub fn push_eof(&mut self) -> Result<Vec<ResponseStreamEvent>, ParseError> {
        let mut events = Vec::new();
        if self.state == MessageState::Body && self.close_delimited {
            events.push(ResponseStreamEvent::EndOfMessage);
            self.state = MessageState::Done;
            return Ok(events);
        }

        if self.state != MessageState::Done {
            return Err(ParseError {
                kind: ParseErrorKind::UnexpectedEof,
                offset: 0,
            });
        }

        Ok(events)
    }

    fn consume_body(&mut self, bytes: &[u8]) -> Result<Vec<ResponseStreamEvent>, ParseError> {
        let mut events = Vec::new();

        if self.chunk_state == ChunkState::None {
            if self.remaining > 0 {
                let to_take = bytes.len().min(self.remaining);
                self.remaining -= to_take;
                if to_take > 0 {
                    events.push(ResponseStreamEvent::BodyBytes { len: to_take });
                }
                if self.remaining == 0 {
                    events.push(ResponseStreamEvent::EndOfMessage);
                    self.state = MessageState::Done;
                }
                return Ok(events);
            }

            if self.close_delimited {
                if !bytes.is_empty() {
                    events.push(ResponseStreamEvent::BodyBytes { len: bytes.len() });
                }
                return Ok(events);
            }

            return Ok(events);
        }

        let mut data_bytes = 0usize;
        for &byte in bytes {
            match &mut self.chunk_state {
                ChunkState::Size { line } => {
                    line.push(byte);
                    if line.len() >= 2 && line[line.len() - 2..] == *CRLF {
                        let line_str = std::str::from_utf8(&line[..line.len() - 2]).map_err(|_| {
                            ParseError {
                                kind: ParseErrorKind::InvalidChunkSize,
                                offset: 0,
                            }
                        })?;
                        let size_str = line_str
                            .split(';')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .trim_start_matches("0x");
                        if size_str.is_empty() {
                            line.clear();
                            continue;
                        }
                        let size = usize::from_str_radix(size_str, 16).map_err(|_| ParseError {
                            kind: ParseErrorKind::InvalidChunkSize,
                            offset: 0,
                        })?;
                        if size == 0 {
                            self.chunk_state = ChunkState::Trailer { line: Vec::new() };
                        } else {
                            self.chunk_state = ChunkState::Data { remaining: size };
                        }
                    }
                }
                ChunkState::Data { remaining } => {
                    if *remaining > 0 {
                        *remaining -= 1;
                        data_bytes += 1;
                        if *remaining == 0 {
                            if data_bytes > 0 {
                                events.push(ResponseStreamEvent::BodyBytes { len: data_bytes });
                                data_bytes = 0;
                            }
                            self.chunk_state = ChunkState::DataCrlf { remaining: 2 };
                        }
                    }
                }
                ChunkState::DataCrlf { remaining } => {
                    if *remaining == 2 && byte != b'\r' {
                        return Err(ParseError {
                            kind: ParseErrorKind::InvalidChunkTerminator,
                            offset: 0,
                        });
                    }
                    if *remaining == 1 && byte != b'\n' {
                        return Err(ParseError {
                            kind: ParseErrorKind::InvalidChunkTerminator,
                            offset: 0,
                        });
                    }
                    *remaining -= 1;
                    if *remaining == 0 {
                        self.chunk_state = ChunkState::Size { line: Vec::new() };
                    }
                }
                ChunkState::Trailer { line } => {
                    line.push(byte);
                    if line.len() >= 2 && line[line.len() - 2..] == *CRLF {
                        let trimmed = &line[..line.len() - 2];
                        if trimmed.is_empty() {
                            self.chunk_state = ChunkState::Done;
                            events.push(ResponseStreamEvent::EndOfMessage);
                            self.state = MessageState::Done;
                            return Ok(events);
                        }
                        line.clear();
                    }
                }
                ChunkState::Done => break,
                ChunkState::None => break,
            }
        }

        if data_bytes > 0 {
            events.push(ResponseStreamEvent::BodyBytes { len: data_bytes });
        }

        Ok(events)
    }
}

impl RequestStreamParser {
    pub fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    pub fn with_limits(limits: Limits) -> Self {
        Self {
            state: MessageState::Headers,
            buffer: Vec::new(),
            limits,
            chunk_state: ChunkState::None,
            remaining: 0,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<RequestStreamEvent>, ParseError> {
        let mut events = Vec::new();
        let mut cursor = 0;

        while cursor < bytes.len() {
            match self.state {
                MessageState::Headers => {
                    self.buffer.extend_from_slice(&bytes[cursor..]);
                    if self.buffer.len() > self.limits.max_header_bytes {
                        return Err(ParseError {
                            kind: ParseErrorKind::HeaderTooLarge,
                            offset: self.limits.max_header_bytes,
                        });
                    }
                    let Some(header_end) = find_header_end(&self.buffer) else {
                        break;
                    };
                    let header_bytes = self.buffer[..header_end].to_vec();
                    let body_start = header_end + HEADER_TERMINATOR.len();
                    let body_bytes = self.buffer[body_start..].to_vec();
                    self.buffer.clear();

                    let (frame_info, body_mode, expect_continue) =
                        parse_request_headers(&header_bytes)?;
                    events.push(RequestStreamEvent::Headers(frame_info.clone()));
                    if expect_continue {
                        events.push(RequestStreamEvent::ExpectContinue);
                    }

                    match body_mode {
                        BodyMode::NoBody => {
                            events.push(RequestStreamEvent::EndOfMessage);
                            self.state = MessageState::Done;
                        }
                        BodyMode::ContentLength => {
                            self.remaining = frame_info.content_length.unwrap_or(0);
                            self.chunk_state = ChunkState::None;
                            self.state = MessageState::Body;
                        }
                        BodyMode::Chunked => {
                            self.chunk_state = ChunkState::Size { line: Vec::new() };
                            self.state = MessageState::Body;
                        }
                        BodyMode::CloseDelimited => {
                            self.chunk_state = ChunkState::None;
                            self.state = MessageState::Body;
                        }
                    }

                    cursor = bytes.len();
                    if !body_bytes.is_empty() && self.state == MessageState::Body {
                        let body_events = self.consume_body(&body_bytes)?;
                        events.extend(body_events);
                    }
                }
                MessageState::Body => {
                    let body_events = self.consume_body(&bytes[cursor..])?;
                    events.extend(body_events);
                    cursor = bytes.len();
                }
                MessageState::Done => break,
            }
        }

        Ok(events)
    }

    pub fn push_eof(&mut self) -> Result<Vec<RequestStreamEvent>, ParseError> {
        let events = Vec::new();
        if self.state == MessageState::Body {
            return Err(ParseError {
                kind: ParseErrorKind::UnexpectedEof,
                offset: 0,
            });
        }

        Ok(events)
    }

    fn consume_body(&mut self, bytes: &[u8]) -> Result<Vec<RequestStreamEvent>, ParseError> {
        let mut events = Vec::new();

        if self.chunk_state == ChunkState::None {
            if self.remaining > 0 {
                let to_take = bytes.len().min(self.remaining);
                self.remaining -= to_take;
                if to_take > 0 {
                    events.push(RequestStreamEvent::BodyBytes { len: to_take });
                }
                if self.remaining == 0 {
                    events.push(RequestStreamEvent::EndOfMessage);
                    self.state = MessageState::Done;
                }
                return Ok(events);
            }

            return Ok(events);
        }

        let mut data_bytes = 0usize;
        for &byte in bytes {
            match &mut self.chunk_state {
                ChunkState::Size { line } => {
                    line.push(byte);
                    if line.len() >= 2 && line[line.len() - 2..] == *CRLF {
                        let line_str = std::str::from_utf8(&line[..line.len() - 2]).map_err(|_| {
                            ParseError {
                                kind: ParseErrorKind::InvalidChunkSize,
                                offset: 0,
                            }
                        })?;
                        let size_str = line_str
                            .split(';')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .trim_start_matches("0x");
                        if size_str.is_empty() {
                            line.clear();
                            continue;
                        }
                        let size = usize::from_str_radix(size_str, 16).map_err(|_| ParseError {
                            kind: ParseErrorKind::InvalidChunkSize,
                            offset: 0,
                        })?;
                        if size == 0 {
                            self.chunk_state = ChunkState::Trailer { line: Vec::new() };
                        } else {
                            self.chunk_state = ChunkState::Data { remaining: size };
                        }
                    }
                }
                ChunkState::Data { remaining } => {
                    if *remaining > 0 {
                        *remaining -= 1;
                        data_bytes += 1;
                        if *remaining == 0 {
                            if data_bytes > 0 {
                                events.push(RequestStreamEvent::BodyBytes { len: data_bytes });
                                data_bytes = 0;
                            }
                            self.chunk_state = ChunkState::DataCrlf { remaining: 2 };
                        }
                    }
                }
                ChunkState::DataCrlf { remaining } => {
                    if *remaining == 2 && byte != b'\r' {
                        return Err(ParseError {
                            kind: ParseErrorKind::InvalidChunkTerminator,
                            offset: 0,
                        });
                    }
                    if *remaining == 1 && byte != b'\n' {
                        return Err(ParseError {
                            kind: ParseErrorKind::InvalidChunkTerminator,
                            offset: 0,
                        });
                    }
                    *remaining -= 1;
                    if *remaining == 0 {
                        self.chunk_state = ChunkState::Size { line: Vec::new() };
                    }
                }
                ChunkState::Trailer { line } => {
                    line.push(byte);
                    if line.len() >= 2 && line[line.len() - 2..] == *CRLF {
                        let trimmed = &line[..line.len() - 2];
                        if trimmed.is_empty() {
                            self.chunk_state = ChunkState::Done;
                            events.push(RequestStreamEvent::EndOfMessage);
                            self.state = MessageState::Done;
                            return Ok(events);
                        }
                        line.clear();
                    }
                }
                ChunkState::Done => break,
                ChunkState::None => break,
            }
        }

        if data_bytes > 0 {
            events.push(RequestStreamEvent::BodyBytes { len: data_bytes });
        }

        Ok(events)
    }
}

fn parse_response_headers(bytes: &[u8]) -> Result<(ResponseFrameInfo, BodyMode), ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError {
        kind: ParseErrorKind::InvalidStatusLine,
        offset: 0,
    })?;
    let mut parts = text.split("\r\n");
    let status_line = parts.next().unwrap_or("");
    let (version, status_code) = parse_status_line(status_line)?;
    let headers = parse_headers(parts.collect::<Vec<_>>())?;
    let content_length = parse_content_length(&headers);
    let chunked = header_has_token(&headers, "transfer-encoding", "chunked");
    let close_delimited = !chunked && content_length.is_none() && !status_has_no_body(status_code);
    let connection_close = response_should_close(&version, &headers);

    let frame = ResponseFrameInfo {
        version,
        status_code,
        headers,
        content_length,
        chunked,
        close_delimited,
        connection_close,
    };

    let body_mode = if status_has_no_body(status_code) || content_length == Some(0) {
        BodyMode::NoBody
    } else if chunked {
        BodyMode::Chunked
    } else if let Some(_length) = content_length {
        BodyMode::ContentLength
    } else if close_delimited {
        BodyMode::CloseDelimited
    } else {
        BodyMode::NoBody
    };

    Ok((frame, body_mode))
}

fn parse_request_headers(
    bytes: &[u8],
) -> Result<(RequestFrameInfo, BodyMode, bool), ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError {
        kind: ParseErrorKind::InvalidStartLine,
        offset: 0,
    })?;
    let mut parts = text.split("\r\n");
    let request_line = parts.next().unwrap_or("");
    let (method, target, version) = parse_request_line(request_line)?;
    let headers = parse_headers(parts.collect::<Vec<_>>())?;
    let content_length = parse_content_length(&headers);
    let chunked = header_has_token(&headers, "transfer-encoding", "chunked");
    let connection_close = request_should_close(&version, &headers);
    let expect_continue = header_has_token(&headers, "expect", "100-continue");

    let frame = RequestFrameInfo {
        method,
        target,
        version,
        headers,
        content_length,
        chunked,
        close_delimited: false,
        connection_close,
    };

    let body_mode = if content_length == Some(0) {
        BodyMode::NoBody
    } else if chunked {
        BodyMode::Chunked
    } else if let Some(_length) = content_length {
        BodyMode::ContentLength
    } else {
        BodyMode::NoBody
    };

    Ok((frame, body_mode, expect_continue))
}

fn parse_request_line(line: &str) -> Result<(String, String, HttpVersion), ParseError> {
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or(ParseError {
        kind: ParseErrorKind::InvalidStartLine,
        offset: 0,
    })?;
    let target = parts.next().ok_or(ParseError {
        kind: ParseErrorKind::InvalidStartLine,
        offset: 0,
    })?;
    let version_raw = parts.next().unwrap_or("HTTP/1.1");
    if parts.next().is_some() {
        return Err(ParseError {
            kind: ParseErrorKind::InvalidStartLine,
            offset: 0,
        });
    }
    let version = parse_http_version(version_raw);
    Ok((method.to_string(), target.to_string(), version))
}

fn parse_status_line(line: &str) -> Result<(HttpVersion, u16), ParseError> {
    let mut parts = line.splitn(3, ' ');
    let version_raw = parts.next().unwrap_or("HTTP/1.1");
    let status_raw = parts.next().ok_or(ParseError {
        kind: ParseErrorKind::InvalidStatusLine,
        offset: 0,
    })?;
    let status_code = status_raw.parse::<u16>().map_err(|_| ParseError {
        kind: ParseErrorKind::InvalidStatusLine,
        offset: 0,
    })?;
    let version = parse_http_version(version_raw);
    Ok((version, status_code))
}

fn parse_http_version(version_raw: &str) -> HttpVersion {
    match version_raw {
        "HTTP/1.0" => HttpVersion::Http10,
        "HTTP/1.1" => HttpVersion::Http11,
        other => HttpVersion::Other(other.to_string()),
    }
}

fn parse_headers(lines: Vec<&str>) -> Result<Vec<Header>, ParseError> {
    let mut headers = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_raw_name: Option<String> = None;
    let mut current_value = String::new();

    for line in lines {
        if line.is_empty() {
            continue;
        }

        if let Some(first) = line.as_bytes().first() {
            if (*first == b' ' || *first == b'\t') && current_name.is_some() {
                current_value.push(' ');
                current_value.push_str(line.trim());
                continue;
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

        current_name = Some(raw_name.trim().to_string());
        current_raw_name = Some(raw_name.to_string());
        current_value.push_str(value.trim_start());
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

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(HEADER_TERMINATOR.len())
        .position(|window| window == HEADER_TERMINATOR)
}

fn parse_content_length(headers: &[Header]) -> Option<usize> {
    headers.iter().find_map(|header| {
        if header.name.eq_ignore_ascii_case("content-length") {
            header.value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })
}

fn header_has_token(headers: &[Header], name: &str, token: &str) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(name)
            && header
                .value
                .split(',')
                .any(|value| value.trim().eq_ignore_ascii_case(token))
    })
}

fn status_has_no_body(status_code: u16) -> bool {
    status_code / 100 == 1 || status_code == 204 || status_code == 304
}

fn request_should_close(version: &HttpVersion, headers: &[Header]) -> bool {
    match version {
        HttpVersion::Http10 => !header_has_token(headers, "connection", "keep-alive"),
        _ => header_has_token(headers, "connection", "close"),
    }
}

fn response_should_close(version: &HttpVersion, headers: &[Header]) -> bool {
    match version {
        HttpVersion::Http10 => !header_has_token(headers, "connection", "keep-alive"),
        HttpVersion::Http11 => header_has_token(headers, "connection", "close"),
        HttpVersion::Other(_) => header_has_token(headers, "connection", "close"),
    }
}
