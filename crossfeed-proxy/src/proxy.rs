use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crossfeed_net::{
    build_acceptor, generate_ca, generate_leaf_cert, CertCache, Http2ParseStatus, Http2Parser,
    RequestParser, ResponseParser, SocksAddress, SocksAuth, SocksResponseParser, SocksVersion,
    TlsConfig,
};

use crate::config::{ProxyConfig, SocksAuthConfig, SocksConfig, SocksVersion as ProxySocksVersion, UpstreamMode};
use crate::error::ProxyError;
use crate::scope::is_in_scope;

const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

pub struct Proxy {
    state: Arc<ProxyState>,
}

struct ProxyState {
    config: ProxyConfig,
    ca: crossfeed_net::CaCertificate,
    cache: Mutex<CertCache>,
}

impl Proxy {
    pub fn new(config: ProxyConfig) -> Result<Self, ProxyError> {
        let ca = generate_ca(&config.tls.ca_common_name)
            .map_err(|err| ProxyError::Config(err.message))?;
        let cache = Mutex::new(CertCache::new(1024));
        Ok(Self {
            state: Arc::new(ProxyState { config, ca, cache }),
        })
    }

    pub async fn run(&self) -> Result<(), ProxyError> {
        let addr = format!("{}:{}", self.state.config.listen.host, self.state.config.listen.port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|err| ProxyError::Runtime(err.to_string()))?;

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

async fn handle_connection(state: Arc<ProxyState>, mut stream: TcpStream) -> Result<(), ProxyError> {
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

async fn handle_http1(
    state: Arc<ProxyState>,
    mut client: TcpStream,
    mut buffer: Vec<u8>,
) -> Result<(), ProxyError> {
    let mut parser = RequestParser::new();
    let mut request_bytes = Vec::new();

    loop {
        if buffer.is_empty() {
            let mut temp = vec![0u8; 8192];
            let n = client
                .read(&mut temp)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            if n == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&temp[..n]);
        }

        request_bytes.extend_from_slice(&buffer);
        let status = parser.push(&buffer);
        buffer.clear();

        match status {
            crossfeed_net::ParseStatus::NeedMore { .. } => continue,
            crossfeed_net::ParseStatus::Error { error, .. } => {
                return Err(ProxyError::Runtime(format!("parse error {error:?}")))
            }
            crossfeed_net::ParseStatus::Complete { message, .. } => {
                let method = message.line.method.to_ascii_uppercase();
                if method == "CONNECT" {
                    let target = message.line.target.clone();
                    return handle_connect(state, client, target).await;
                }

                let (host, port, path) = resolve_target(&message.line.target, &message.headers)
                    .ok_or_else(|| ProxyError::Runtime("missing host".to_string()))?;

                let _in_scope = is_in_scope(&state.config.scope.rules, &host, &path);

                let upstream = connect_upstream(&state.config, host.clone(), port).await?;
                let mut upstream = upstream;

                let outbound = serialize_request(&message, &path, &host);
                upstream
                    .write_all(&outbound)
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;

                let response_bytes = read_response(&mut upstream).await?;
                client
                    .write_all(&response_bytes)
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                return Ok(());
            }
        }
    }
}

async fn handle_http2(
    state: Arc<ProxyState>,
    mut client: TcpStream,
    mut buffer: Vec<u8>,
) -> Result<(), ProxyError> {
    let mut parser = Http2Parser::new();
    let mut accumulated = buffer.clone();
    loop {
        let status = parser.push(&buffer);
        buffer.clear();
        match status {
            Http2ParseStatus::NeedMore { .. } => {
                let mut temp = vec![0u8; 8192];
                let n = client
                    .read(&mut temp)
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                if n == 0 {
                    return Ok(());
                }
                accumulated.extend_from_slice(&temp[..n]);
                buffer.extend_from_slice(&temp[..n]);
            }
            Http2ParseStatus::Error { error, .. } => {
                return Err(ProxyError::Runtime(format!("http2 parse error {error:?}")))
            }
            Http2ParseStatus::Complete { frame, .. } => {
                if let crossfeed_net::FramePayload::Headers(headers) = frame.payload {
                    let mut authority = None;
                    for header in headers.headers {
                        if header.name == b":authority".to_vec() {
                            authority = Some(String::from_utf8_lossy(&header.value).to_string());
                            break;
                        }
                    }
                    let Some(authority) = authority else {
                        return Err(ProxyError::Runtime("missing :authority".to_string()));
                    };
                    let (host, port) = split_host_port(&authority);
                    let upstream = connect_upstream(&state.config, host, port).await?;
                    let mut upstream = upstream;
                    upstream
                        .write_all(&accumulated)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

                    tokio::io::copy_bidirectional(&mut client, &mut upstream)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    return Ok(());
                }
            }
        }
    }
}

async fn handle_connect(
    state: Arc<ProxyState>,
    mut client: TcpStream,
    target: String,
) -> Result<(), ProxyError> {
    let (host, port) = split_host_port(&target);
    let mut upstream = connect_upstream(&state.config, host.clone(), port).await?;

    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    if !state.config.tls.enabled {
        tokio::io::copy_bidirectional(&mut client, &mut upstream)
            .await
            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
        return Ok(());
    }

    let leaf = {
        let mut cache = state.cache.lock().await;
        if let Some(cert) = cache.get(&host) {
            cert
        } else {
            let cert = generate_leaf_cert(&host, &state.ca)
                .map_err(|err| ProxyError::Runtime(err.message))?;
            cache.persist(&host, &cert).map_err(|err| ProxyError::Runtime(err.message))?;
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

    tokio::io::copy_bidirectional(&mut tls_client, &mut tls_upstream)
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

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
                return Err(ProxyError::Runtime("socks auth not implemented".to_string()));
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
    let mut parser = ResponseParser::new();
    let mut buffer = vec![0u8; 8192];
    let mut response = Vec::new();

    loop {
        let n = stream
            .read(&mut buffer)
            .await
            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..n]);
        match parser.push(&buffer[..n]) {
            crossfeed_net::ParseStatus::NeedMore { .. } => continue,
            crossfeed_net::ParseStatus::Complete { .. } => break,
            crossfeed_net::ParseStatus::Error { error, .. } => {
                return Err(ProxyError::Runtime(format!("response parse error {error:?}")));
            }
        }
    }

    Ok(response)
}

fn resolve_target(target: &str, headers: &[crossfeed_net::Header]) -> Option<(String, u16, String)> {
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

    let host_header = headers.iter().find(|header| header.name.eq_ignore_ascii_case("host"));
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

fn serialize_request(
    request: &crossfeed_net::Request,
    path: &str,
    host: &str,
) -> Vec<u8> {
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
