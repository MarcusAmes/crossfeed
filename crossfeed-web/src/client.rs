use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, HeaderValue};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_native_tls::TlsConnector;
use tokio_util::sync::CancellationToken;

use crossfeed_net::{
    DEFAULT_MAX_FRAME_SIZE, FramePayload, FrameType, HeaderField, HpackEncoder, Http2ParseStatus,
    Http2Parser, ParseStatus, ResponseParser, SettingsFrame, encode_data_frames,
    encode_headers_from_fields, encode_raw_frame,
};

use crate::rate_limit::RateLimiter;
use crate::request::Request;
use crate::response::Response;
use crate::retry::RetryPolicy;

const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub concurrency: usize,
    pub timeout: Duration,
    pub retry: RetryPolicy,
    pub rate_limit: Option<RateLimiter>,
    pub proxy: Option<ProxyConfig>,
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub kind: ProxyKind,
}

#[derive(Debug, Clone)]
pub enum ProxyKind {
    Http,
    Socks,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            concurrency: 20,
            timeout: Duration::from_secs(30),
            retry: RetryPolicy::default(),
            rate_limit: None,
            proxy: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    config: Arc<ClientConfig>,
}

#[derive(Debug, Clone)]
pub struct CancelToken {
    inner: CancellationToken,
}

#[derive(Debug, Clone)]
pub enum RequestError {
    Cancelled,
    Transport(String),
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            inner: CancellationToken::new(),
        }
    }

    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    fn token(&self) -> CancellationToken {
        self.inner.clone()
    }
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub async fn request(&self, request: Request) -> Result<Response, String> {
        let cancel = CancelToken::new();
        self.request_with_cancel(request, cancel)
            .await
            .map_err(|err| match err {
                RequestError::Cancelled => "cancelled".to_string(),
                RequestError::Transport(message) => message,
            })
    }

    pub async fn request_with_cancel(
        &self,
        request: Request,
        cancel: CancelToken,
    ) -> Result<Response, RequestError> {
        let mut attempt = 0;
        loop {
            if cancel.is_cancelled() {
                return Err(RequestError::Cancelled);
            }
            if let Some(limiter) = &self.config.rate_limit {
                limiter.acquire().await;
            }
            let result = self.execute_with_cancel(request.clone(), cancel.token()).await;
            match result {
                Ok(response) => {
                    if self.config.retry.retry_on_5xx
                        && response.status >= 500
                        && attempt < self.config.retry.max_retries
                    {
                        let delay = self.config.retry.next_delay(attempt);
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Ok(response);
                }
                Err(RequestError::Cancelled) => {
                    return Err(RequestError::Cancelled);
                }
                Err(err) => {
                    if attempt < self.config.retry.max_retries {
                        let delay = self.config.retry.next_delay(attempt);
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    async fn execute_with_cancel(
        &self,
        request: Request,
        cancel: CancellationToken,
    ) -> Result<Response, RequestError> {
        let uri = request.uri.clone();
        let host = uri
            .host()
            .ok_or_else(|| RequestError::Transport("missing host".to_string()))?
            .to_string();
        let is_https = uri
            .scheme_str()
            .map(|scheme| scheme.eq_ignore_ascii_case("https"))
            .unwrap_or(false);
        let http_version = request.http_version.trim();
        let is_http2 = is_http2_version(http_version);
        let port = uri.port_u16().unwrap_or_else(|| if is_https { 443 } else { 80 });

        let mut stream = tokio::select! {
            _ = cancel.cancelled() => return Err(RequestError::Cancelled),
            result = TcpStream::connect((host.as_str(), port)) => {
                result.map_err(|err| RequestError::Transport(err.to_string()))?
            }
        };
        if is_https {
            let mut builder = native_tls::TlsConnector::builder();
            if is_http2 {
                builder.request_alpns(&["h2"]);
            }
            let connector = builder
                .build()
                .map_err(|err| RequestError::Transport(err.to_string()))?;
            let connector = TlsConnector::from(connector);
            let mut tls_stream = tokio::select! {
                _ = cancel.cancelled() => return Err(RequestError::Cancelled),
                result = connector.connect(&host, stream) => {
                    result.map_err(|err| RequestError::Transport(err.to_string()))?
                }
            };
            if is_http2 {
                return send_http2_request(&mut tls_stream, &request, &host, &cancel).await;
            }
            return send_http1_request(&mut tls_stream, &request, &host, &cancel).await;
        }

        if is_http2 {
            return send_http2_request(&mut stream, &request, &host, &cancel).await;
        }
        send_http1_request(&mut stream, &request, &host, &cancel).await
    }
}

async fn write_with_cancel<S>(
    stream: &mut S,
    bytes: &[u8],
    cancel: &CancellationToken,
) -> Result<(), RequestError>
where
    S: AsyncWriteExt + Unpin,
{
    tokio::select! {
        _ = cancel.cancelled() => Err(RequestError::Cancelled),
        result = stream.write_all(bytes) => result.map_err(|err| RequestError::Transport(err.to_string())),
    }
}

async fn send_http1_request<S>(
    stream: &mut S,
    request: &Request,
    host: &str,
    cancel: &CancellationToken,
) -> Result<Response, RequestError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let path = request.uri.path_and_query().map(|v| v.as_str()).unwrap_or("/");
    let request_bytes = serialize_request(request, host, path);
    write_with_cancel(stream, &request_bytes, cancel).await?;
    read_http1_response(stream, cancel).await
}

async fn send_http2_request<S>(
    stream: &mut S,
    request: &Request,
    host: &str,
    cancel: &CancellationToken,
) -> Result<Response, RequestError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_with_cancel(stream, HTTP2_PREFACE, cancel).await?;
    let settings_frame = encode_raw_frame(FrameType::Settings, 0, 0, &[]);
    write_with_cancel(stream, &settings_frame, cancel).await?;

    let headers = build_http2_headers(request, host);
    let mut encoder = HpackEncoder::new();
    let header_frames = encode_headers_from_fields(
        1,
        request.body.is_empty(),
        &headers,
        &mut encoder,
        DEFAULT_MAX_FRAME_SIZE,
    );
    for frame in header_frames {
        write_with_cancel(stream, &frame, cancel).await?;
    }
    if !request.body.is_empty() {
        let data_frames = encode_data_frames(1, true, &request.body, DEFAULT_MAX_FRAME_SIZE);
        for frame in data_frames {
            write_with_cancel(stream, &frame, cancel).await?;
        }
    }
    read_http2_response(stream, cancel).await
}

async fn read_http1_response<S>(
    stream: &mut S,
    cancel: &CancellationToken,
) -> Result<Response, RequestError>
where
    S: AsyncRead + Unpin,
{
    let mut parser = ResponseParser::new();
    let mut buffer = vec![0u8; 8192];
    loop {
        let n = tokio::select! {
            _ = cancel.cancelled() => return Err(RequestError::Cancelled),
            result = stream.read(&mut buffer) => {
                result.map_err(|err| RequestError::Transport(err.to_string()))?
            }
        };
        if n == 0 {
            let status = parser.push(&[]);
            return match status {
                ParseStatus::Complete { message, .. } => Ok(convert_http1_response(message)),
                _ => Err(RequestError::Transport("unexpected eof".to_string())),
            };
        }
        match parser.push(&buffer[..n]) {
            ParseStatus::Complete { message, .. } => {
                return Ok(convert_http1_response(message));
            }
            ParseStatus::NeedMore { .. } => {}
            ParseStatus::Error { error, .. } => {
                return Err(RequestError::Transport(format!(
                    "http1 parse error: {:?}",
                    error
                )));
            }
        }
    }
}

async fn read_http2_response<S>(
    stream: &mut S,
    cancel: &CancellationToken,
) -> Result<Response, RequestError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut parser = Http2Parser::new_without_preface();
    let mut buffer = vec![0u8; 8192];
    let mut headers = HeaderMap::new();
    let mut status: Option<u16> = None;
    let mut body = Vec::new();
    loop {
        let n = tokio::select! {
            _ = cancel.cancelled() => return Err(RequestError::Cancelled),
            result = stream.read(&mut buffer) => {
                result.map_err(|err| RequestError::Transport(err.to_string()))?
            }
        };
        if n == 0 {
            return Err(RequestError::Transport("unexpected eof".to_string()));
        }
        let mut status_frame = parser.push(&buffer[..n]);
        loop {
            match status_frame {
                Http2ParseStatus::NeedMore { .. } => break,
                Http2ParseStatus::Error { error, .. } => {
                    return Err(RequestError::Transport(format!(
                        "http2 parse error: {:?}",
                        error
                    )));
                }
                Http2ParseStatus::Complete { frame, .. } => {
                    if let FramePayload::Settings(settings) = &frame.payload {
                        if !settings.ack {
                            apply_http2_settings(&mut parser, settings);
                            let ack = encode_raw_frame(FrameType::Settings, 0x1, 0, &[]);
                            write_with_cancel(stream, &ack, cancel).await?;
                        }
                    }
                    match frame.payload {
                        FramePayload::Headers(headers_frame) => {
                            if frame.header.stream_id == 1 {
                                apply_http2_headers(&mut headers, &mut status, &headers_frame.headers)?;
                                if headers_frame.end_stream {
                                    return finalize_http2_response(status, headers, body);
                                }
                            }
                        }
                        FramePayload::Data(data_frame) => {
                            if frame.header.stream_id == 1 {
                                body.extend_from_slice(&data_frame.payload);
                                if data_frame.end_stream {
                                    return finalize_http2_response(status, headers, body);
                                }
                            }
                        }
                        FramePayload::GoAway(_) | FramePayload::RstStream(_) => {
                            return Err(RequestError::Transport("http2 stream closed".to_string()));
                        }
                        _ => {}
                    }
                    status_frame = parser.push(&[]);
                }
            }
        }
    }
}

fn serialize_request(request: &Request, host: &str, path: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let method = request.method.as_str();
    let version = if request.http_version.trim().is_empty() {
        "HTTP/1.1"
    } else {
        request.http_version.trim()
    };
    bytes.extend_from_slice(format!("{} {} {}\r\n", method, path, version).as_bytes());
    bytes.extend_from_slice(format!("Host: {}\r\n", host).as_bytes());
    for (name, value) in request.headers.iter() {
        bytes.extend_from_slice(name.as_str().as_bytes());
        bytes.extend_from_slice(b": ");
        bytes.extend_from_slice(value.as_bytes());
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"\r\n");
    bytes.extend_from_slice(&request.body);
    bytes
}

fn is_http2_version(version: &str) -> bool {
    let normalized = version.trim();
    normalized.eq_ignore_ascii_case("HTTP/2") || normalized.eq_ignore_ascii_case("HTTP/2.0")
}

fn convert_http1_response(response: crossfeed_net::Response) -> Response {
    let mut headers = HeaderMap::new();
    for header in response.headers {
        if let (Ok(name), Ok(value)) = (
            http::header::HeaderName::from_bytes(header.name.as_bytes()),
            HeaderValue::from_str(&header.value),
        ) {
            headers.append(name, value);
        }
    }
    Response {
        status: response.line.status_code,
        headers,
        body: response.body,
    }
}

fn build_http2_headers(request: &Request, host: &str) -> Vec<HeaderField> {
    let mut headers = Vec::new();
    let scheme = request.uri.scheme_str().unwrap_or("http");
    let path = request
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let authority = match request.uri.port_u16() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    headers.push(HeaderField {
        name: b":method".to_vec(),
        value: request.method.as_str().as_bytes().to_vec(),
    });
    headers.push(HeaderField {
        name: b":scheme".to_vec(),
        value: scheme.as_bytes().to_vec(),
    });
    headers.push(HeaderField {
        name: b":authority".to_vec(),
        value: authority.as_bytes().to_vec(),
    });
    headers.push(HeaderField {
        name: b":path".to_vec(),
        value: path.as_bytes().to_vec(),
    });
    for (name, value) in request.headers.iter() {
        let name = name.as_str().to_ascii_lowercase();
        if name == "host" || name == "connection" {
            continue;
        }
        headers.push(HeaderField {
            name: name.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
        });
    }
    headers
}

fn apply_http2_settings(parser: &mut Http2Parser, settings: &SettingsFrame) {
    for (id, value) in &settings.settings {
        if *id == 0x1 {
            parser.set_max_header_table_size(*value);
        }
        if *id == 0x5 {
            parser.set_max_frame_size(*value as usize);
        }
    }
    parser.set_settings_received(true);
}

fn apply_http2_headers(
    headers: &mut HeaderMap,
    status: &mut Option<u16>,
    fields: &[HeaderField],
) -> Result<(), RequestError> {
    for field in fields {
        let name = String::from_utf8_lossy(&field.name);
        if name.starts_with(':') {
            if name == ":status" {
                let value = String::from_utf8_lossy(&field.value);
                *status = value.parse::<u16>().ok();
            }
            continue;
        }
        let header_name = http::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| RequestError::Transport(err.to_string()))?;
        let header_value = HeaderValue::from_bytes(&field.value)
            .map_err(|err| RequestError::Transport(err.to_string()))?;
        headers.append(header_name, header_value);
    }
    Ok(())
}

fn finalize_http2_response(
    status: Option<u16>,
    headers: HeaderMap,
    body: Vec<u8>,
) -> Result<Response, RequestError> {
    let status = status.ok_or_else(|| RequestError::Transport("missing :status".to_string()))?;
    Ok(Response { status, headers, body })
}
