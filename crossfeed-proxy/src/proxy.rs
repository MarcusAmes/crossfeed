use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};

use uuid::Uuid;

use crossfeed_net::{
    CertCache, Http2ParseStatus, Http2Parser, RequestParser, ResponseParser, SocksAddress,
    SocksAuth, SocksResponseParser, SocksVersion, TlsConfig, build_acceptor, generate_ca,
    generate_leaf_cert, write_ca_to_dir,
};
use crossfeed_storage::{TimelineRequest, TimelineResponse};

use crate::config::{
    ProxyConfig, SocksAuthConfig, SocksConfig, SocksVersion as ProxySocksVersion, UpstreamMode,
};
use crate::error::ProxyError;
use crate::events::{ProxyCommand, ProxyControl, ProxyEvents, control_channel, event_channel};
use crate::intercept::{InterceptDecision, InterceptManager, InterceptResult};
use crate::scope::is_in_scope;
use crate::timeline_event::{ProxyEvent, ProxyEventKind, ProxyRequest, ProxyResponse};

const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

pub struct Proxy {
    state: Arc<ProxyState>,
}

struct ProxyState {
    config: ProxyConfig,
    ca: crossfeed_net::CaCertificate,
    cache: Mutex<CertCache>,
    sender: mpsc::Sender<ProxyEvent>,
    control_rx: Mutex<mpsc::Receiver<ProxyCommand>>,
    intercepts: Mutex<InterceptManager<ProxyRequest, ProxyResponse>>,
    _ca_paths: crossfeed_net::CaMaterialPaths,
}

impl Proxy {
    pub fn new(config: ProxyConfig) -> Result<(Self, ProxyEvents, ProxyControl), ProxyError> {
        let ca = generate_ca(&config.tls.ca_common_name)
            .map_err(|err| ProxyError::Config(err.message))?;
        let cache = Mutex::new(CertCache::with_disk_path(1024, &config.tls.leaf_cert_dir));
        let (sender, events) = event_channel();
        let (control, control_rx) = control_channel();
        let ca_paths = write_ca_to_dir(&config.tls.ca_cert_dir, &ca.material)
            .map_err(|err| ProxyError::Runtime(err.message))?;
        Ok((
            Self {
                state: Arc::new(ProxyState {
                    config,
                    ca,
                    cache,
                    sender,
                    control_rx: Mutex::new(control_rx),
                    intercepts: Mutex::new(InterceptManager::default()),
                    _ca_paths: ca_paths,
                }),
            },
            events,
            control,
        ))
    }

    pub async fn run(&self) -> Result<(), ProxyError> {
        let addr = format!(
            "{}:{}",
            self.state.config.listen.host, self.state.config.listen.port
        );
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|err| ProxyError::Runtime(err.to_string()))?;

        let control_state = Arc::clone(&self.state);
        tokio::spawn(async move {
            control_loop(control_state).await;
        });

        loop {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            let state = Arc::clone(&self.state);
            tokio::spawn(async move {
                if let Err(err) = handle_connection(state, stream).await {
                    let _ = err;
                }
            });
        }
    }
}

async fn handle_connection(
    state: Arc<ProxyState>,
    mut stream: TcpStream,
) -> Result<(), ProxyError> {
    let mut buffer = Vec::new();

    let mut temp = vec![0u8; 8192];

    let n = stream
        .read(&mut temp)
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    if n == 0 {
        return Ok(());
    }
    buffer.extend_from_slice(&temp[..n]);

    if buffer.starts_with(HTTP2_PREFACE) {
        return handle_http2(state, stream, buffer).await;
    }

    handle_http1(state, stream, buffer).await
}

async fn handle_http2(
    state: Arc<ProxyState>,
    client: TcpStream,
    buffer: Vec<u8>,
) -> Result<(), ProxyError> {
    handle_http2_stream(state, client, buffer).await
}

async fn handle_http2_stream<C>(
    state: Arc<ProxyState>,
    mut client: C,
    mut buffer: Vec<u8>,
) -> Result<(), ProxyError>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    let mut parser = Http2Parser::new();
    let mut accumulated = buffer.clone();
    loop {
        let status = parser.push(&buffer);
        buffer.clear();
        match status {
            Http2ParseStatus::NeedMore { .. } => {
                let mut temp = vec![0u8; 8192];
                let n = client.read(&mut temp).await?;
                if n == 0 {
                    return Ok(());
                }
                accumulated.extend_from_slice(&temp[..n]);
                buffer.extend_from_slice(&temp[..n]);
                continue;
            }
            Http2ParseStatus::Error { error, .. } => {
                return Err(ProxyError::Runtime(format!("http2 parse error {error:?}")));
            }
            Http2ParseStatus::Complete { frame, .. } => {
                if let crossfeed_net::FramePayload::Headers(headers) = frame.payload {
                    let mut authority = None;
                    let mut path = None;
                    for header in headers.headers.iter() {
                        if header.name == b":authority".to_vec() {
                            authority = Some(String::from_utf8_lossy(&header.value).to_string());
                        }
                        if header.name == b":path".to_vec() {
                            path = Some(String::from_utf8_lossy(&header.value).to_string());
                        }
                    }
                    let Some(authority) = authority else {
                        return Err(ProxyError::Runtime("missing :authority".to_string()));
                    };
                    let (host, port) = split_host_port(&authority);
                    let path = path.unwrap_or_else(|| "/".to_string());
                    let in_scope = is_in_scope(&state.config.scope.rules, &host, &path);
                    let started_at = chrono::Utc::now().to_rfc3339();
                    let scope_status = if in_scope { "in_scope" } else { "out_of_scope" };
                    let request_id = Uuid::new_v4();
                    let timeline_request = TimelineRequest {
                        source: "proxy".to_string(),
                        method: "HTTP2".to_string(),
                        scheme: "https".to_string(),
                        host: host.clone(),
                        port,
                        path,
                        query: None,
                        url: format!("https://{host}"),
                        http_version: "HTTP/2".to_string(),
                        request_headers: accumulated.clone(),
                        request_body: Vec::new(),
                        request_body_size: 0,
                        request_body_truncated: false,
                        started_at: started_at.clone(),
                        completed_at: None,
                        duration_ms: None,
                        scope_status_at_capture: scope_status.to_string(),
                        scope_status_current: None,
                        scope_rules_version: 1,
                        capture_filtered: false,
                        timeline_filtered: false,
                    };
                    let proxy_request = ProxyRequest {
                        id: request_id,
                        timeline: timeline_request,
                        raw_request: accumulated.clone(),
                    };
                    let _ = state
                        .sender
                        .send(ProxyEvent {
                            event_id: Uuid::new_v4(),
                            request_id,
                            kind: ProxyEventKind::RequestForwarded,
                            request: Some(proxy_request),
                            response: None,
                        })
                        .await;

                    continue;
                }
            }
        }
    }
}

async fn handle_http1(
    state: Arc<ProxyState>,
    client: TcpStream,
    buffer: Vec<u8>,
) -> Result<(), ProxyError> {
    handle_http1_tcp(state, client, buffer).await
}

async fn handle_http1_tcp(
    state: Arc<ProxyState>,
    mut client: TcpStream,
    mut buffer: Vec<u8>,
) -> Result<(), ProxyError> {
    let mut parser = RequestParser::new();

    loop {
        if buffer.is_empty() {
            let mut temp = vec![0u8; 8192];
            let n = client.read(&mut temp).await?;
            if n == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&temp[..n]);
        }

        let status = parser.push(&buffer);
        buffer.clear();

        match status {
            crossfeed_net::ParseStatus::NeedMore { .. } => continue,
            crossfeed_net::ParseStatus::Error { error, .. } => {
                return Err(ProxyError::Runtime(format!("parse error {error:?}")));
            }
            crossfeed_net::ParseStatus::Complete { message, .. } => {
                let method = message.line.method.to_ascii_uppercase();
                if method == "CONNECT" {
                    handle_connect(Arc::clone(&state), &mut client, message.line.target.clone())
                        .await?;
                    return Ok(());
                }

                handle_http1_request(
                    Arc::clone(&state),
                    &mut client,
                    None::<&mut TcpStream>,
                    message,
                )
                .await?;
            }
        }
    }
}

async fn handle_http1_tls<C, U>(
    state: Arc<ProxyState>,
    mut client: C,
    mut buffer: Vec<u8>,
    mut upstream: U,
) -> Result<(), ProxyError>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let mut parser = RequestParser::new();

    loop {
        if buffer.is_empty() {
            let mut temp = vec![0u8; 8192];
            let n = client.read(&mut temp).await?;
            if n == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&temp[..n]);
        }

        let status = parser.push(&buffer);
        buffer.clear();

        match status {
            crossfeed_net::ParseStatus::NeedMore { .. } => continue,
            crossfeed_net::ParseStatus::Error { error, .. } => {
                return Err(ProxyError::Runtime(format!("parse error {error:?}")));
            }
            crossfeed_net::ParseStatus::Complete { message, .. } => {
                handle_http1_request(
                    Arc::clone(&state),
                    &mut client,
                    Some(&mut upstream),
                    message,
                )
                .await?;
            }
        }
    }
}

async fn handle_http1_request<C, U>(
    state: Arc<ProxyState>,
    client: &mut C,
    mut upstream: Option<&mut U>,
    message: crossfeed_net::Request,
) -> Result<(), ProxyError>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let method = message.line.method.to_ascii_uppercase();
    if method == "CONNECT" {
        return Err(ProxyError::Runtime("CONNECT not allowed".to_string()));
    }

    let (host, port, path) = resolve_target(&message.line.target, &message.headers)
        .ok_or_else(|| ProxyError::Runtime("missing host".to_string()))?;

    let in_scope = is_in_scope(&state.config.scope.rules, &host, &path);

    let request_id = Uuid::new_v4();
    let started_at = chrono::Utc::now().to_rfc3339();
    let scope_status = if in_scope { "in_scope" } else { "out_of_scope" };
    let (timeline_request, request_bytes) = build_request_record(
        &message,
        &path,
        &host,
        port,
        scope_status,
        started_at.clone(),
    );
    let proxy_request = ProxyRequest {
        id: request_id,
        timeline: timeline_request.clone(),
        raw_request: request_bytes,
    };

    let mut intercepts = state.intercepts.lock().await;
    let request_intercept = intercepts.intercept_request(request_id, proxy_request.clone());
    drop(intercepts);

    let (forwarded_request, proxy_response) = match request_intercept {
        InterceptResult::Forward(proxy_request) => {
            let _ = state
                .sender
                .send(ProxyEvent {
                    event_id: Uuid::new_v4(),
                    request_id,
                    kind: ProxyEventKind::RequestForwarded,
                    request: Some(proxy_request.clone()),
                    response: None,
                })
                .await;

            let response_bytes = match upstream.as_mut() {
                Some(upstream) => {
                    upstream
                        .write_all(&proxy_request.raw_request)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    upstream
                        .flush()
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    read_response_stream(upstream).await?
                }
                None => {
                    let mut upstream = connect_upstream(&state.config, host.clone(), port).await?;
                    upstream
                        .write_all(&proxy_request.raw_request)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    upstream
                        .flush()
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    read_response_stream(&mut upstream).await?
                }
            };

            (
                Some(proxy_request),
                parse_response(&response_bytes, &started_at).map(|timeline_response| {
                    ProxyResponse {
                        id: Uuid::new_v4(),
                        timeline: timeline_response,
                        raw_response: response_bytes,
                    }
                }),
            )
        }
        InterceptResult::Intercepted { receiver, .. } => {
            let _ = state
                .sender
                .send(ProxyEvent {
                    event_id: Uuid::new_v4(),
                    request_id,
                    kind: ProxyEventKind::RequestIntercepted,
                    request: Some(proxy_request.clone()),
                    response: None,
                })
                .await;

            let decision = receiver
                .await
                .map_err(|_| ProxyError::Runtime("request intercept closed".to_string()))?;
            let proxy_request = match decision {
                InterceptDecision::Allow(proxy_request) => proxy_request,
                InterceptDecision::Drop => return Ok(()),
            };

            let _ = state
                .sender
                .send(ProxyEvent {
                    event_id: Uuid::new_v4(),
                    request_id,
                    kind: ProxyEventKind::RequestForwarded,
                    request: Some(proxy_request.clone()),
                    response: None,
                })
                .await;

            let response_bytes = match upstream.as_mut() {
                Some(upstream) => {
                    upstream
                        .write_all(&proxy_request.raw_request)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    upstream
                        .flush()
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    read_response_stream(upstream).await?
                }
                None => {
                    let mut upstream = connect_upstream(&state.config, host.clone(), port).await?;
                    upstream
                        .write_all(&proxy_request.raw_request)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    upstream
                        .flush()
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    read_response_stream(&mut upstream).await?
                }
            };

            (
                Some(proxy_request),
                parse_response(&response_bytes, &started_at).map(|timeline_response| {
                    ProxyResponse {
                        id: Uuid::new_v4(),
                        timeline: timeline_response,
                        raw_response: response_bytes,
                    }
                }),
            )
        }
    };

    if let (Some(forwarded_request), Some(proxy_response)) = (forwarded_request, proxy_response) {
        let response_id = proxy_response.id;
        let mut intercepts = state.intercepts.lock().await;
        let response_intercept =
            intercepts.intercept_response(request_id, response_id, proxy_response.clone());
        drop(intercepts);

        match response_intercept {
            InterceptResult::Forward(proxy_response) => {
                client
                    .write_all(&proxy_response.raw_response)
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                let _ = state
                    .sender
                    .send(ProxyEvent {
                        event_id: Uuid::new_v4(),
                        request_id,
                        kind: ProxyEventKind::ResponseForwarded,
                        request: Some(forwarded_request.clone()),
                        response: Some(proxy_response),
                    })
                    .await;
            }
            InterceptResult::Intercepted { receiver, .. } => {
                let _ = state
                    .sender
                    .send(ProxyEvent {
                        event_id: Uuid::new_v4(),
                        request_id,
                        kind: ProxyEventKind::ResponseIntercepted,
                        request: Some(forwarded_request.clone()),
                        response: Some(proxy_response.clone()),
                    })
                    .await;
                let decision = receiver
                    .await
                    .map_err(|_| ProxyError::Runtime("response intercept closed".to_string()))?;
                match decision {
                    InterceptDecision::Allow(proxy_response) => {
                        client
                            .write_all(&proxy_response.raw_response)
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        let _ = state
                            .sender
                            .send(ProxyEvent {
                                event_id: Uuid::new_v4(),
                                request_id,
                                kind: ProxyEventKind::ResponseForwarded,
                                request: Some(forwarded_request.clone()),
                                response: Some(proxy_response),
                            })
                            .await;
                    }
                    InterceptDecision::Drop => {}
                }
            }
        }
    }

    Ok(())
}

async fn handle_connect<S>(
    state: Arc<ProxyState>,
    client: &mut S,
    target: String,
) -> Result<(), ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (host, port) = split_host_port(&target);
    let mut upstream = connect_upstream(&state.config, host.clone(), port).await?;

    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    if !state.config.tls.enabled {
        let (mut client_read, mut client_write) = tokio::io::split(client);
        let (mut upstream_read, mut upstream_write) = tokio::io::split(&mut upstream);
        tokio::try_join!(
            tokio::io::copy(&mut client_read, &mut upstream_write),
            tokio::io::copy(&mut upstream_read, &mut client_write)
        )?;
        return Ok(());
    }

    let leaf = {
        let mut cache = state.cache.lock().await;
        if let Some(cert) = cache.get(&host) {
            cert
        } else {
            let cert = generate_leaf_cert(&host, &state.ca)
                .map_err(|err| ProxyError::Runtime(err.message))?;
            cache
                .persist(&host, &cert)
                .map_err(|err| ProxyError::Runtime(err.message))?;
            cache.insert(host.clone(), cert.clone());
            cert
        }
    };

    let acceptor = build_acceptor(
        &TlsConfig {
            allow_legacy: state.config.tls.allow_legacy,
        },
        &leaf,
    )
    .map_err(|err| ProxyError::Runtime(err.message))?;

    let ssl = openssl::ssl::Ssl::new(acceptor.context())
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    let mut tls_client = tokio_openssl::SslStream::new(ssl, client)
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    tokio::io::AsyncWriteExt::flush(&mut tls_client)
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    tokio_openssl::SslStream::accept(std::pin::pin!(&mut tls_client))
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    let connector = openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls())
        .map_err(|err| ProxyError::Runtime(err.to_string()))?
        .build();
    let ssl = connector
        .configure()
        .map_err(|err| ProxyError::Runtime(err.to_string()))?
        .into_ssl(&host)
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    let mut tls_upstream = tokio_openssl::SslStream::new(ssl, upstream)
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    tokio_openssl::SslStream::connect(std::pin::pin!(&mut tls_upstream))
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    let mut buffer = vec![0u8; 8192];
    let n = tls_client.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }
    buffer.truncate(n);
    if buffer.starts_with(HTTP2_PREFACE) {
        handle_http2_stream(state, tls_client, buffer).await?;
    } else {
        handle_http1_tls(state, tls_client, buffer, tls_upstream).await?;
    }

    Ok(())
}
async fn connect_upstream(
    config: &ProxyConfig,
    host: String,
    port: u16,
) -> Result<TcpStream, ProxyError> {
    match config.upstream.mode {
        UpstreamMode::Direct => TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|err| ProxyError::Runtime(err.to_string())),
        UpstreamMode::Socks => connect_via_socks(config.upstream.socks.as_ref(), host, port).await,
    }
}

async fn connect_via_socks(
    socks: Option<&SocksConfig>,
    host: String,
    port: u16,
) -> Result<TcpStream, ProxyError> {
    let Some(socks) = socks else {
        return Err(ProxyError::Config("missing socks config".to_string()));
    };

    let mut stream = TcpStream::connect((socks.host.as_str(), socks.port))
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    match socks.version {
        ProxySocksVersion::V5 => {
            let auth = match &socks.auth {
                SocksAuthConfig::None => SocksAuth::NoAuth,
                SocksAuthConfig::UserPass { username, password } => SocksAuth::UserPass {
                    username: username.clone(),
                    password: password.clone(),
                },
            };
            let handshake = crossfeed_net::build_handshake_request(SocksVersion::V5, &auth);
            stream
                .write_all(&handshake)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;

            let mut response = [0u8; 2];
            stream
                .read_exact(&mut response)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            let method = crossfeed_net::parse_handshake_response(&response)
                .map_err(|err| ProxyError::Runtime(format!("socks handshake {err:?}")))?;
            if method == 0x02 {
                return Err(ProxyError::Runtime(
                    "socks auth not implemented".to_string(),
                ));
            }

            let address = SocksAddress::Domain(host);
            let connect = crossfeed_net::build_socks5_connect(address, port);
            stream
                .write_all(&connect)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;

            let mut parser = SocksResponseParser::new();
            let mut buffer = vec![0u8; 512];
            loop {
                let n = stream
                    .read(&mut buffer)
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                if n == 0 {
                    return Err(ProxyError::Runtime("socks connection closed".to_string()));
                }
                match parser.push(&buffer[..n]) {
                    crossfeed_net::SocksParseStatus::NeedMore => continue,
                    crossfeed_net::SocksParseStatus::Complete { response } => {
                        if response.reply != crossfeed_net::SocksReply::Succeeded {
                            return Err(ProxyError::Runtime("socks connect failed".to_string()));
                        }
                        break;
                    }
                    crossfeed_net::SocksParseStatus::Error { error } => {
                        return Err(ProxyError::Runtime(format!("socks error {error:?}")));
                    }
                }
            }
        }
        ProxySocksVersion::V4 | ProxySocksVersion::V4a => {
            let address = if matches!(socks.version, ProxySocksVersion::V4) {
                match host.parse::<std::net::Ipv4Addr>() {
                    Ok(ip) => SocksAddress::IpV4(ip.octets()),
                    Err(_) => SocksAddress::Domain(host.clone()),
                }
            } else {
                SocksAddress::Domain(host.clone())
            };
            let connect = crossfeed_net::build_socks4_connect(address, port, "");
            stream
                .write_all(&connect)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            let mut response = [0u8; 8];
            stream
                .read_exact(&mut response)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            let reply = crossfeed_net::parse_socks_response(&response)
                .map_err(|err| ProxyError::Runtime(format!("socks response {err:?}")))?;
            if reply.reply != crossfeed_net::SocksReply::Succeeded {
                return Err(ProxyError::Runtime("socks connect failed".to_string()));
            }
        }
    }

    Ok(stream)
}

async fn read_response(stream: &mut TcpStream) -> Result<Vec<u8>, ProxyError> {
    read_response_stream(stream).await
}

async fn read_response_stream<S>(stream: &mut S) -> Result<Vec<u8>, ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut parser = ResponseParser::new();
    let mut buffer = vec![0u8; 8192];
    let mut response = Vec::new();

    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..n]);
        match parser.push(&buffer[..n]) {
            crossfeed_net::ParseStatus::NeedMore { .. } => {
                continue;
            }
            crossfeed_net::ParseStatus::Complete { .. } => {
                break;
            }
            crossfeed_net::ParseStatus::Error { error, .. } => {
                if matches!(error.kind, crossfeed_net::ParseErrorKind::UnexpectedEof) {
                    continue;
                }
                return Err(ProxyError::Runtime(format!(
                    "response parse error {error:?}"
                )));
            }
        }
    }

    if response.is_empty() {
        return Err(ProxyError::Runtime(
            "empty response from upstream".to_string(),
        ));
    }

    Ok(response)
}

fn resolve_target(
    target: &str,
    headers: &[crossfeed_net::Header],
) -> Option<(String, u16, String)> {
    if target.starts_with("http://") || target.starts_with("https://") {
        if let Ok(url) = url::Url::parse(target) {
            let host = url.host_str()?.to_string();
            let port = url.port_or_known_default().unwrap_or(80) as u16;
            let mut path = url.path().to_string();
            if let Some(query) = url.query() {
                path.push('?');
                path.push_str(query);
            }
            return Some((host, port, path));
        }
    }

    let host_header = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("host"));
    let host_header = host_header.map(|header| header.value.clone());
    let host = host_header?;
    let (host, port) = split_host_port(&host);
    Some((host, port, target.to_string()))
}

fn split_host_port(host: &str) -> (String, u16) {
    if let Some((host, port)) = host.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            return (host.to_string(), port);
        }
    }
    (host.to_string(), 443)
}

fn serialize_request(request: &crossfeed_net::Request, path: &str, host: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let version = match request.line.version {
        crossfeed_net::HttpVersion::Http10 => "HTTP/1.0",
        crossfeed_net::HttpVersion::Http11 => "HTTP/1.1",
        crossfeed_net::HttpVersion::Other(ref other) => other.as_str(),
    };
    bytes.extend_from_slice(format!("{} {} {}\r\n", request.line.method, path, version).as_bytes());
    let mut has_host = false;
    for header in &request.headers {
        if header.name.eq_ignore_ascii_case("host") {
            has_host = true;
        }
        if header.name.eq_ignore_ascii_case("proxy-connection") {
            continue;
        }
        bytes.extend_from_slice(header.raw_name.as_bytes());
        bytes.extend_from_slice(b": ");
        bytes.extend_from_slice(header.value.as_bytes());
        bytes.extend_from_slice(b"\r\n");
    }
    if !has_host {
        bytes.extend_from_slice(format!("Host: {}\r\n", host).as_bytes());
    }
    bytes.extend_from_slice(b"\r\n");
    bytes.extend_from_slice(&request.body);
    bytes
}

fn build_request_record(
    request: &crossfeed_net::Request,
    path: &str,
    host: &str,
    port: u16,
    scope_status: &str,
    started_at: String,
) -> (TimelineRequest, Vec<u8>) {
    let request_headers = serialize_request(request, path, host);
    let timeline_request = TimelineRequest {
        source: "proxy".to_string(),
        method: request.line.method.clone(),
        scheme: "http".to_string(),
        host: host.to_string(),
        port,
        path: path.to_string(),
        query: None,
        url: format!("http://{}{}", request.line.target, path),
        http_version: match request.line.version {
            crossfeed_net::HttpVersion::Http10 => "HTTP/1.0".to_string(),
            crossfeed_net::HttpVersion::Http11 => "HTTP/1.1".to_string(),
            crossfeed_net::HttpVersion::Other(ref other) => other.to_string(),
        },
        request_headers: request_headers.clone(),
        request_body: request.body.clone(),
        request_body_size: request.body.len(),
        request_body_truncated: false,
        started_at,
        completed_at: None,
        duration_ms: None,
        scope_status_at_capture: scope_status.to_string(),
        scope_status_current: None,
        scope_rules_version: 1,
        capture_filtered: false,
        timeline_filtered: false,
    };

    (timeline_request, request_headers)
}

fn parse_response(response_bytes: &[u8], received_at: &str) -> Option<TimelineResponse> {
    let mut parser = ResponseParser::new();
    let status = parser.push(response_bytes);
    let crossfeed_net::ParseStatus::Complete { message, .. } = status else {
        return None;
    };

    let body = message.body;
    let body_size = body.len();

    Some(TimelineResponse {
        timeline_request_id: 0,
        status_code: message.line.status_code,
        reason: Some(message.line.reason),
        response_headers: response_bytes.to_vec(),
        response_body: body,
        response_body_size: body_size,
        response_body_truncated: false,
        http_version: match message.line.version {
            crossfeed_net::HttpVersion::Http10 => "HTTP/1.0".to_string(),
            crossfeed_net::HttpVersion::Http11 => "HTTP/1.1".to_string(),
            crossfeed_net::HttpVersion::Other(ref other) => other.to_string(),
        },
        received_at: received_at.to_string(),
    })
}

async fn control_loop(state: Arc<ProxyState>) {
    loop {
        let command = {
            let mut receiver = state.control_rx.lock().await;
            receiver.recv().await
        };

        let Some(command) = command else {
            break;
        };

        let mut intercepts = state.intercepts.lock().await;
        match command {
            ProxyCommand::SetRequestIntercept(enabled) => intercepts.set_request_intercept(enabled),
            ProxyCommand::SetResponseIntercept(enabled) => {
                intercepts.set_response_intercept(enabled)
            }
            ProxyCommand::InterceptResponseForRequest(id) => {
                intercepts.intercept_response_for_request(id)
            }
            ProxyCommand::DecideRequest { id, decision } => {
                intercepts.resolve_request(id, decision);
            }
            ProxyCommand::DecideResponse { id, decision } => {
                intercepts.resolve_response(id, decision);
            }
        }
    }
}
