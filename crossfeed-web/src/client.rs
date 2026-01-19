use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, HeaderValue};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::rate_limit::RateLimiter;
use crate::request::Request;
use crate::response::Response;
use crate::retry::RetryPolicy;

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

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    pub async fn request(&self, request: Request) -> Result<Response, String> {
        let mut attempt = 0;
        loop {
            if let Some(limiter) = &self.config.rate_limit {
                limiter.acquire().await;
            }
            let result = self.execute(request.clone()).await;
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

    async fn execute(&self, request: Request) -> Result<Response, String> {
        let uri = request.uri.clone();
        let host = uri.host().ok_or("missing host")?.to_string();
        let port = uri.port_u16().unwrap_or_else(|| {
            if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            }
        });

        let mut stream = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|err| err.to_string())?;
        let request_bytes = serialize_request(
            &request,
            &host,
            uri.path_and_query().map(|v| v.as_str()).unwrap_or("/"),
        );
        stream
            .write_all(&request_bytes)
            .await
            .map_err(|err| err.to_string())?;

        let mut buffer = vec![0u8; 8192];
        let mut response_bytes = Vec::new();
        loop {
            let n = stream
                .read(&mut buffer)
                .await
                .map_err(|err| err.to_string())?;
            if n == 0 {
                break;
            }
            response_bytes.extend_from_slice(&buffer[..n]);
        }

        parse_response(&response_bytes)
    }
}

fn serialize_request(request: &Request, host: &str, path: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let method = request.method.as_str();
    bytes.extend_from_slice(format!("{} {} HTTP/1.1\r\n", method, path).as_bytes());
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

fn parse_response(bytes: &[u8]) -> Result<Response, String> {
    let response = String::from_utf8_lossy(bytes);
    let mut parts = response.split("\r\n\r\n");
    let head = parts.next().ok_or("missing response")?;
    let body = parts.next().unwrap_or("").as_bytes().to_vec();
    let mut lines = head.lines();
    let status_line = lines.next().ok_or("missing status")?;
    let mut status_parts = status_line.split_whitespace();
    status_parts.next();
    let status = status_parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);

    let mut headers = HeaderMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = http::header::HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| "invalid header")?;
            let value = HeaderValue::from_str(value.trim()).map_err(|_| "invalid header")?;
            headers.append(name, value);
        }
    }

    Ok(Response {
        status,
        headers,
        body,
    })
}
