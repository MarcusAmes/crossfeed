use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};

use uuid::Uuid;

use crossfeed_net::{
    CertCache, HpackEncoder, Http2ParseStatus, Http2Parser, RequestParser,
    RequestStreamEvent, RequestStreamParser, ResponseParser, ResponseStreamEvent,
    ResponseStreamParser, SocksAddress, SocksAuth, SocksResponseParser, SocksVersion,
    TlsConfig, build_acceptor, encode_data_frames, encode_headers_from_fields,
    encode_raw_frame, generate_leaf_cert, load_or_generate_ca,
};
use crossfeed_storage::{TimelineRequest, TimelineResponse};

use crate::config::{
    ProxyConfig, ProxyProtocolMode, SocksAuthConfig, SocksConfig,
    SocksVersion as ProxySocksVersion, UpstreamMode,
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
    alpn_cache: Mutex<HashMap<String, NegotiatedProtocol>>,
}

impl Proxy {
    pub fn new(config: ProxyConfig) -> Result<(Self, ProxyEvents, ProxyControl), ProxyError> {
        let (ca, ca_paths) =
            load_or_generate_ca(&config.tls.ca_cert_dir, &config.tls.ca_common_name)
                .map_err(|err| ProxyError::Config(err.message))?;
        let cache = Mutex::new(CertCache::with_disk_path(1024, &config.tls.leaf_cert_dir));
        let (sender, events) = event_channel();
        let (control, control_rx) = control_channel();
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
                    alpn_cache: Mutex::new(HashMap::new()),
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
    let _ = state;
    let _ = client;
    let _ = buffer;
    Err(ProxyError::Runtime(
        "http2 cleartext not supported".to_string(),
    ))
}

#[derive(Debug)]
struct Http2StreamState {
    request_headers: Vec<crossfeed_net::HeaderField>,
    request_body: Vec<u8>,
    request_complete: bool,
    request_sent: bool,
    pending_request_data: Vec<u8>,
    pending_request_end_stream: bool,
    request_end_stream_sent: bool,
    response_headers: Vec<crossfeed_net::HeaderField>,
    response_body: Vec<u8>,
    response_complete: bool,
    response_sent: bool,
    pending_response_data: Vec<u8>,
    pending_response_end_stream: bool,
    response_end_stream_sent: bool,
    request_id: Option<Uuid>,
    request_started_at: Option<String>,
    scope_status: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    path: Option<String>,
    scheme: Option<String>,
    proxy_request: Option<ProxyRequest>,
    proxy_response: Option<ProxyResponse>,
    request_intercept: bool,
    response_intercept: bool,
}

impl Http2StreamState {
    fn new() -> Self {
        Self {
            request_headers: Vec::new(),
            request_body: Vec::new(),
            request_complete: false,
            request_sent: false,
            pending_request_data: Vec::new(),
            pending_request_end_stream: false,
            request_end_stream_sent: false,
            response_headers: Vec::new(),
            response_body: Vec::new(),
            response_complete: false,
            response_sent: false,
            pending_response_data: Vec::new(),
            pending_response_end_stream: false,
            response_end_stream_sent: false,
            request_id: None,
            request_started_at: None,
            scope_status: None,
            host: None,
            port: None,
            path: None,
            scheme: None,
            proxy_request: None,
            proxy_response: None,
            request_intercept: false,
            response_intercept: false,
        }
    }
}

#[derive(Debug)]
enum Http2InterceptDecision {
    Request {
        stream_id: u32,
        decision: InterceptDecision<ProxyRequest>,
    },
    Response {
        stream_id: u32,
        decision: InterceptDecision<ProxyResponse>,
    },
}

async fn handle_http2_stream<C, U>(
    state: Arc<ProxyState>,
    client: C,
    upstream: U,
    mut buffer: Vec<u8>,
    default_host: String,
    default_port: u16,
) -> Result<(), ProxyError>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let mut client_parser = Http2Parser::new();
    let mut upstream_parser = Http2Parser::new_without_preface();
    let mut client_session = Http2Session::new();
    let mut upstream_session = Http2Session::new();
    let (mut client_read, mut client_write) = tokio::io::split(client);
    let (mut upstream_read, mut upstream_write) = tokio::io::split(upstream);
    let (decision_tx, mut decision_rx) = mpsc::channel(128);
    let mut streams: HashMap<u32, Http2StreamState> = HashMap::new();

    send_settings_frame(&mut client_write, &client_session.local_settings, false).await?;
    send_preface_and_settings(&mut upstream_write, &upstream_session.local_settings).await?;

    if !buffer.is_empty() {
        handle_http2_bytes(
            &state,
            Direction::ClientToUpstream,
            &mut client_parser,
            &mut client_session,
            &mut upstream_session,
            &mut client_write,
            &mut upstream_write,
            &mut streams,
            &decision_tx,
            &buffer,
            &default_host,
            default_port,
        )
        .await?;
        buffer.clear();
    }

    let mut temp = vec![0u8; 8192];
    let mut upstream_temp = vec![0u8; 8192];

    loop {
        if let Ok(decision) = decision_rx.try_recv() {
            handle_http2_decision(
                &state,
                decision,
                &mut client_session,
                &mut upstream_session,
                &mut client_write,
                &mut upstream_write,
                &mut streams,
                &default_host,
                default_port,
            )
            .await?;
        }

        tokio::select! {
            client_read_result = client_read.read(&mut temp) => {
                let n = client_read_result.map_err(|err| ProxyError::Runtime(err.to_string()))?;
                if n == 0 {
                    return Ok(());
                }
                handle_http2_bytes(
                    &state,
                    Direction::ClientToUpstream,
                    &mut client_parser,
                    &mut client_session,
                    &mut upstream_session,
                    &mut client_write,
                    &mut upstream_write,
                    &mut streams,
                    &decision_tx,
                    &temp[..n],
                    &default_host,
                    default_port,
                )
                .await?;
            }
            upstream_read_result = upstream_read.read(&mut upstream_temp) => {
                let n = upstream_read_result.map_err(|err| ProxyError::Runtime(err.to_string()))?;
                if n == 0 {
                    return Ok(());
                }
                handle_http2_bytes(
                    &state,
                    Direction::UpstreamToClient,
                    &mut upstream_parser,
                    &mut upstream_session,
                    &mut client_session,
                    &mut upstream_write,
                    &mut client_write,
                    &mut streams,
                    &decision_tx,
                    &upstream_temp[..n],
                    &default_host,
                    default_port,
                )
                .await?;
            }
            decision = decision_rx.recv() => {
                if let Some(decision) = decision {
                    handle_http2_decision(
                        &state,
                        decision,
                        &mut client_session,
                        &mut upstream_session,
                        &mut client_write,
                        &mut upstream_write,
                        &mut streams,
                        &default_host,
                        default_port,
                    )
                    .await?;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    ClientToUpstream,
    UpstreamToClient,
}

const FLOW_CONTROL_THRESHOLD: i32 = 32 * 1024;

#[derive(Debug, Clone)]
struct Http2Settings {
    header_table_size: u32,
    enable_push: bool,
    initial_window_size: u32,
    max_frame_size: u32,
    max_header_list_size: u32,
}

impl Default for Http2Settings {
    fn default() -> Self {
        Self {
            header_table_size: 4096,
            enable_push: false,
            initial_window_size: 65_535,
            max_frame_size: 16_384,
            max_header_list_size: 262_144,
        }
    }
}

struct Http2Session {
    hpack_encoder: HpackEncoder,
    local_settings: Http2Settings,
    peer_settings: Http2Settings,
    send_conn_window: i32,
    send_stream_windows: HashMap<u32, i32>,
    recv_conn_window: i32,
    recv_stream_windows: HashMap<u32, i32>,
    peer_settings_received: bool,
}

impl Http2Session {
    fn new() -> Self {
        let local_settings = Http2Settings::default();
        let peer_settings = Http2Settings::default();
        let send_conn_window = peer_settings.initial_window_size as i32;
        let recv_conn_window = local_settings.initial_window_size as i32;
        Self {
            hpack_encoder: HpackEncoder::new(),
            local_settings,
            peer_settings,
            send_conn_window,
            send_stream_windows: HashMap::new(),
            recv_conn_window,
            recv_stream_windows: HashMap::new(),
            peer_settings_received: false,
        }
    }

    fn send_stream_window(&mut self, stream_id: u32) -> &mut i32 {
        self.send_stream_windows
            .entry(stream_id)
            .or_insert(self.peer_settings.initial_window_size as i32)
    }

    fn recv_stream_window(&mut self, stream_id: u32) -> &mut i32 {
        self.recv_stream_windows
            .entry(stream_id)
            .or_insert(self.local_settings.initial_window_size as i32)
    }

    fn max_frame_size(&self) -> usize {
        self.peer_settings.max_frame_size as usize
    }

    fn apply_peer_settings(&mut self, settings: &crossfeed_net::SettingsFrame) {
        for (id, value) in &settings.settings {
            match *id {
                0x1 => {
                    self.peer_settings.header_table_size = *value;
                }
                0x2 => {
                    self.peer_settings.enable_push = *value != 0;
                }
                0x4 => {
                    let new_size = *value as i32;
                    let delta = new_size - self.peer_settings.initial_window_size as i32;
                    for window in self.send_stream_windows.values_mut() {
                        *window += delta;
                    }
                    self.peer_settings.initial_window_size = *value;
                }
                0x5 => {
                    self.peer_settings.max_frame_size = *value;
                }
                0x6 => {
                    self.peer_settings.max_header_list_size = *value;
                }
                _ => {}
            }
        }
    }

    fn apply_send_window_update(&mut self, stream_id: u32, increment: u32) {
        if stream_id == 0 {
            self.send_conn_window += increment as i32;
        } else {
            let window = self.send_stream_window(stream_id);
            *window += increment as i32;
        }
    }

    fn consume_recv_data(&mut self, stream_id: u32, size: usize) -> Vec<WindowUpdate> {
        let mut updates = Vec::new();
        let size = size as i32;
        self.recv_conn_window -= size;
        let stream_window_value = {
            let stream_window = self.recv_stream_window(stream_id);
            *stream_window -= size;
            *stream_window
        };
        let target = self.local_settings.initial_window_size as i32;

        if self.recv_conn_window < FLOW_CONTROL_THRESHOLD {
            let increment = (target - self.recv_conn_window).max(0) as u32;
            if increment > 0 {
                self.recv_conn_window += increment as i32;
                updates.push(WindowUpdate { stream_id: 0, increment });
            }
        }

        if stream_window_value < FLOW_CONTROL_THRESHOLD {
            let increment = (target - stream_window_value).max(0) as u32;
            if increment > 0 {
                let stream_window = self.recv_stream_window(stream_id);
                *stream_window += increment as i32;
                updates.push(WindowUpdate { stream_id, increment });
            }
        }

        updates
    }

}

#[derive(Debug, Clone, Copy)]
struct WindowUpdate {
    stream_id: u32,
    increment: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NegotiatedProtocol {
    Http1,
    Http2,
}

#[derive(Debug, Clone)]
struct Http2RequestMeta {
    method: String,
    scheme: String,
    authority: String,
    host: String,
    port: u16,
    path: String,
}

async fn handle_http2_bytes<W1, W2>(
    state: &Arc<ProxyState>,
    direction: Direction,
    parser: &mut Http2Parser,
    recv_session: &mut Http2Session,
    send_session: &mut Http2Session,
    sender_write: &mut W1,
    peer_write: &mut W2,
    streams: &mut HashMap<u32, Http2StreamState>,
    decision_tx: &mpsc::Sender<Http2InterceptDecision>,
    bytes: &[u8],
    default_host: &str,
    default_port: u16,
) -> Result<(), ProxyError>
where
    W1: AsyncWrite + Unpin,
    W2: AsyncWrite + Unpin,
{
    let mut status = parser.push(bytes);
    loop {
        match status {
            Http2ParseStatus::NeedMore { .. } => break,
            Http2ParseStatus::Error { error, .. } => {
                let direction_label = match direction {
                    Direction::ClientToUpstream => "client",
                    Direction::UpstreamToClient => "upstream",
                };
                println!("ERROR: H2 parse error dir={} {:?}", direction_label, error);
                return Err(ProxyError::Runtime(format!("http2 parse error {error:?}")));
            }
            Http2ParseStatus::Complete { frame, .. } => {
                if let crossfeed_net::FramePayload::Settings(ref settings) = frame.payload {
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
                handle_http2_frame(
                    state,
                    direction,
                    recv_session,
                    send_session,
                    sender_write,
                    peer_write,
                    streams,
                    decision_tx,
                    frame,
                    default_host,
                    default_port,
                )
                .await?;
                status = parser.push(&[]);
            }
        }
    }

    Ok(())
}

async fn handle_http2_frame<W1, W2>(
    state: &Arc<ProxyState>,
    direction: Direction,
    recv_session: &mut Http2Session,
    send_session: &mut Http2Session,
    sender_write: &mut W1,
    peer_write: &mut W2,
    streams: &mut HashMap<u32, Http2StreamState>,
    decision_tx: &mpsc::Sender<Http2InterceptDecision>,
    frame: crossfeed_net::Frame,
    default_host: &str,
    default_port: u16,
) -> Result<(), ProxyError>
where
    W1: AsyncWrite + Unpin,
    W2: AsyncWrite + Unpin,
{
    let stream_id = frame.header.stream_id;
    let frame_type = frame.header.frame_type.clone();
    let frame_flags = frame.header.flags;
    match frame.payload {
        crossfeed_net::FramePayload::Settings(settings) => {
            if !settings.ack {
                recv_session.apply_peer_settings(&settings);
                recv_session.peer_settings_received = true;
                for (id, value) in &settings.settings {
                    if *id == 0x1 && matches!(direction, Direction::ClientToUpstream) {
                        let _ = value;
                    }
                }
                send_settings_frame(sender_write, &recv_session.local_settings, true).await?;
                flush_pending_after_settings(
                    state,
                    direction,
                    send_session,
                    peer_write,
                    streams,
                    decision_tx,
                    default_host,
                    default_port,
                )
                .await?;
            }
        }
        crossfeed_net::FramePayload::WindowUpdate(update) => {
            recv_session.apply_send_window_update(update.stream_id, update.increment);
            flush_pending_data(direction, recv_session, sender_write, streams).await?;
        }
        crossfeed_net::FramePayload::Ping(ping) => {
            if !ping.ack {
                let ack = encode_raw_frame(
                    crossfeed_net::FrameType::Ping,
                    0x1,
                    0,
                    &ping.opaque_data,
                );
                sender_write
                    .write_all(&ack)
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                sender_write
                    .flush()
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            }
        }
        crossfeed_net::FramePayload::GoAway(goaway) => {
            let mut payload = Vec::with_capacity(8 + goaway.debug_data.len());
            payload.extend_from_slice(&(goaway.last_stream_id & 0x7FFF_FFFF).to_be_bytes());
            payload.extend_from_slice(&goaway.error_code.to_be_bytes());
            payload.extend_from_slice(&goaway.debug_data);
            let frame = encode_raw_frame(crossfeed_net::FrameType::GoAway, 0, 0, &payload);
            peer_write.write_all(&frame).await.map_err(|err| {
                println!("ERROR: H2 GOAWAY write failed: {}", err);
                ProxyError::Runtime(err.to_string())
            })?;
            peer_write.flush().await.map_err(|err| {
                println!("ERROR: H2 GOAWAY flush failed: {}", err);
                ProxyError::Runtime(err.to_string())
            })?;
        }
        crossfeed_net::FramePayload::RstStream(rst) => {
            let frame = encode_raw_frame(
                crossfeed_net::FrameType::RstStream,
                0,
                stream_id,
                &rst.error_code.to_be_bytes(),
            );
            peer_write.write_all(&frame).await.map_err(|err| {
                println!("ERROR: H2 RST_STREAM write failed: {}", err);
                ProxyError::Runtime(err.to_string())
            })?;
            peer_write.flush().await.map_err(|err| {
                println!("ERROR: H2 RST_STREAM flush failed: {}", err);
                ProxyError::Runtime(err.to_string())
            })?;
            streams.remove(&stream_id);
        }
        crossfeed_net::FramePayload::Headers(headers) => {
            match direction {
                Direction::ClientToUpstream => {
                    let stream = streams.entry(stream_id).or_insert_with(Http2StreamState::new);
                    stream.request_headers.extend(headers.headers.clone());
                    if stream.request_id.is_none() {
                        initialize_http2_request_state(
                            state,
                            stream,
                            &headers.headers,
                            default_host,
                            default_port,
                        )
                        .await?;
                    }

                    if !stream.request_intercept {
                        let max_frame_size = send_session.max_frame_size();
                        send_headers_logged(
                            peer_write,
                            &mut send_session.hpack_encoder,
                            max_frame_size,
                            stream_id,
                            headers.end_stream,
                            &headers.headers,
                            "upstream",
                        )
                        .await?;
                        stream.request_sent = true;
                        if headers.end_stream {
                            stream.request_end_stream_sent = true;
                        }
                    }

                    if headers.end_stream {
                        finalize_http2_request(
                            state,
                            stream_id,
                            stream,
                            decision_tx,
                            default_host,
                            default_port,
                            send_session,
                            peer_write,
                        )
                        .await?;
                    }
                }
                Direction::UpstreamToClient => {
                    let stream = streams.entry(stream_id).or_insert_with(Http2StreamState::new);
                    if stream.response_headers.is_empty() {
                        initialize_http2_response_state(state, stream).await?;
                    }
                    stream.response_headers.extend(headers.headers.clone());

                    if !stream.response_intercept {
                        let max_frame_size = send_session.max_frame_size();
                        send_headers_logged(
                            peer_write,
                            &mut send_session.hpack_encoder,
                            max_frame_size,
                            stream_id,
                            headers.end_stream,
                            &headers.headers,
                            "client",
                        )
                        .await?;
                        stream.response_sent = true;
                        if headers.end_stream {
                            stream.response_end_stream_sent = true;
                        }
                    }

                    if headers.end_stream {
                        let should_remove = finalize_http2_response(
                            state,
                            stream_id,
                            stream,
                            decision_tx,
                            send_session,
                            peer_write,
                        )
                        .await?;
                        if should_remove {
                            streams.remove(&stream_id);
                        }
                    }
                }
            }
        }
        crossfeed_net::FramePayload::Data(data) => {
            let updates = recv_session.consume_recv_data(stream_id, data.payload.len());
            let direction_label = match direction {
                Direction::ClientToUpstream => "client",
                Direction::UpstreamToClient => "upstream",
            };
            send_window_updates(sender_write, &updates, direction_label).await?;
            match direction {
                Direction::ClientToUpstream => {
                    let stream = streams.entry(stream_id).or_insert_with(Http2StreamState::new);
                    stream.request_body.extend_from_slice(&data.payload);

                    if !stream.request_intercept {
                        send_data_with_flow(
                            send_session,
                            peer_write,
                            stream,
                            stream_id,
                            &data.payload,
                            data.end_stream,
                            "upstream",
                            true,
                        )
                        .await?;
                    }

                    if data.end_stream {
                        finalize_http2_request(
                            state,
                            stream_id,
                            stream,
                            decision_tx,
                            default_host,
                            default_port,
                            send_session,
                            peer_write,
                        )
                        .await?;
                    }
                }
                Direction::UpstreamToClient => {
                    let stream = streams.entry(stream_id).or_insert_with(Http2StreamState::new);
                    stream.response_body.extend_from_slice(&data.payload);

                    if !stream.response_intercept {
                        send_data_with_flow(
                            send_session,
                            peer_write,
                            stream,
                            stream_id,
                            &data.payload,
                            data.end_stream,
                            "client",
                            false,
                        )
                        .await?;
                    }

                    if data.end_stream {
                        let should_remove = finalize_http2_response(
                            state,
                            stream_id,
                            stream,
                            decision_tx,
                            send_session,
                            peer_write,
                        )
                        .await?;
                        if should_remove {
                            streams.remove(&stream_id);
                        }
                    }
                }
            }
        }
        crossfeed_net::FramePayload::Priority(_priority) => {}
        crossfeed_net::FramePayload::Continuation(_payload) => {}
        crossfeed_net::FramePayload::Raw(payload) => {
            if frame_type == crossfeed_net::FrameType::PushPromise {
                if let Some(promised_id) = parse_promised_stream_id(&payload, frame_flags) {
                    println!("ERROR: H2 reject push stream={}", promised_id);
                    let rst = encode_raw_frame(
                        crossfeed_net::FrameType::RstStream,
                        0,
                        promised_id,
                        &0x7u32.to_be_bytes(),
                    );
                    sender_write.write_all(&rst).await.map_err(|err| {
                        println!("ERROR: H2 push RST write failed: {}", err);
                        ProxyError::Runtime(err.to_string())
                    })?;
                    sender_write.flush().await.map_err(|err| {
                        println!("ERROR: H2 push RST flush failed: {}", err);
                        ProxyError::Runtime(err.to_string())
                    })?;
                }
            }
        }
    }

    Ok(())
}

async fn flush_pending_after_settings<W: AsyncWrite + Unpin>(
    state: &Arc<ProxyState>,
    direction: Direction,
    send_session: &mut Http2Session,
    peer_write: &mut W,
    streams: &mut HashMap<u32, Http2StreamState>,
    decision_tx: &mpsc::Sender<Http2InterceptDecision>,
    default_host: &str,
    default_port: u16,
) -> Result<(), ProxyError> {
    if !send_session.peer_settings_received {
        return Ok(());
    }

    match direction {
        Direction::ClientToUpstream => {
            for (stream_id, stream) in streams.iter_mut() {
                if stream.response_complete && !stream.response_sent && !stream.response_intercept {
                    finalize_http2_response(
                        state,
                        *stream_id,
                        stream,
                        decision_tx,
                        send_session,
                        peer_write,
                    )
                    .await?;
                }
            }
        }
        Direction::UpstreamToClient => {
            for (stream_id, stream) in streams.iter_mut() {
                if stream.request_complete && !stream.request_sent && !stream.request_intercept {
                    finalize_http2_request(
                        state,
                        *stream_id,
                        stream,
                        decision_tx,
                        default_host,
                        default_port,
                        send_session,
                        peer_write,
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

async fn handle_http2_decision<CU: AsyncWrite + Unpin, UU: AsyncWrite + Unpin>(
    state: &Arc<ProxyState>,
    decision: Http2InterceptDecision,
    client_session: &mut Http2Session,
    upstream_session: &mut Http2Session,
    client_write: &mut CU,
    upstream_write: &mut UU,
    streams: &mut HashMap<u32, Http2StreamState>,
    default_host: &str,
    default_port: u16,
) -> Result<(), ProxyError> {
    match decision {
        Http2InterceptDecision::Request {
            stream_id,
            decision,
        } => {
            let Some(stream) = streams.get_mut(&stream_id) else {
                return Ok(());
            };
            match decision {
                InterceptDecision::Allow(proxy_request) => {
                    let request = parse_http1_request(&proxy_request.raw_request)?;
                    let scheme = stream
                        .scheme
                        .clone()
                        .unwrap_or_else(|| default_scheme_for_port(default_port).to_string());
                    let authority = match (stream.host.clone(), stream.port) {
                        (Some(host), Some(port)) => format_authority(&host, port, &scheme),
                        (Some(host), None) => host,
                        (None, _) => default_host.to_string(),
                    };
                    let (_meta, headers) = http1_request_to_h2(&request, &scheme, &authority)?;
                    let max_frame_size = upstream_session.max_frame_size();
                    send_headers_logged(
                        upstream_write,
                        &mut upstream_session.hpack_encoder,
                        max_frame_size,
                        stream_id,
                        request.body.is_empty(),
                        &headers,
                        "upstream",
                    )
                    .await?;
                    send_data_with_flow(
                        upstream_session,
                        upstream_write,
                        stream,
                        stream_id,
                        &request.body,
                        true,
                        "upstream",
                        true,
                    )
                    .await?;
                    stream.proxy_request = Some(proxy_request.clone());
                    if let Some(request_id) = stream.request_id {
                        send_proxy_event(
                            state,
                            request_id,
                            ProxyEventKind::RequestForwarded,
                            Some(proxy_request),
                            None,
                        )
                        .await;
                    }
                }
                InterceptDecision::Drop => {
                    let frame = encode_raw_frame(
                        crossfeed_net::FrameType::RstStream,
                        0,
                        stream_id,
                        &0x8u32.to_be_bytes(),
                    );
                    client_write
                        .write_all(&frame)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    streams.remove(&stream_id);
                }
            }
        }
        Http2InterceptDecision::Response {
            stream_id,
            decision,
        } => {
            let Some(stream) = streams.get_mut(&stream_id) else {
                return Ok(());
            };
            match decision {
                InterceptDecision::Allow(proxy_response) => {
                    let response = parse_http1_response(&proxy_response.raw_response)?;
                    let headers = http1_response_to_h2(&response);
                    let max_frame_size = client_session.max_frame_size();
                    send_headers_logged(
                        client_write,
                        &mut client_session.hpack_encoder,
                        max_frame_size,
                        stream_id,
                        response.body.is_empty(),
                        &headers,
                        "client",
                    )
                    .await?;
                    send_data_with_flow(
                        client_session,
                        client_write,
                        stream,
                        stream_id,
                        &response.body,
                        true,
                        "client",
                        false,
                    )
                    .await?;
                    stream.proxy_response = Some(proxy_response.clone());
                    if let (Some(request_id), Some(request)) =
                        (stream.request_id, stream.proxy_request.clone())
                    {
                        send_proxy_event(
                            state,
                            request_id,
                            ProxyEventKind::ResponseForwarded,
                            Some(request),
                            Some(proxy_response),
                        )
                        .await;
                    }
                    streams.remove(&stream_id);
                }
                InterceptDecision::Drop => {
                    let frame = encode_raw_frame(
                        crossfeed_net::FrameType::RstStream,
                        0,
                        stream_id,
                        &0x8u32.to_be_bytes(),
                    );
                    client_write
                        .write_all(&frame)
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    streams.remove(&stream_id);
                }
            }
        }
    }

    Ok(())
}

async fn initialize_http2_request_state(
    state: &Arc<ProxyState>,
    stream: &mut Http2StreamState,
    headers: &[crossfeed_net::HeaderField],
    default_host: &str,
    default_port: u16,
) -> Result<(), ProxyError> {
    let meta = parse_http2_request_meta(headers, default_host, default_port)?;
    stream.request_id = Some(Uuid::new_v4());
    stream.host = Some(meta.host.clone());
    stream.port = Some(meta.port);
    stream.path = Some(meta.path.clone());
    stream.scheme = Some(meta.scheme.clone());
    stream.request_started_at = Some(chrono::Utc::now().to_rfc3339());
    let in_scope = is_in_scope(&state.config.scope.rules, &meta.host, &meta.path);
    stream.scope_status = Some(if in_scope { "in_scope" } else { "out_of_scope" }.to_string());
    let intercepts = state.intercepts.lock().await;
    stream.request_intercept = intercepts.is_request_intercept_enabled();
    Ok(())
}

async fn initialize_http2_response_state(
    state: &Arc<ProxyState>,
    stream: &mut Http2StreamState,
) -> Result<(), ProxyError> {
    let request_id = match stream.request_id {
        Some(id) => id,
        None => {
            return Ok(());
        }
    };
    let intercepts = state.intercepts.lock().await;
    stream.response_intercept = intercepts.is_response_intercept_enabled()
        || intercepts.should_intercept_response_for_request(request_id);
    Ok(())
}

async fn finalize_http2_request<W: AsyncWrite + Unpin>(
    state: &Arc<ProxyState>,
    stream_id: u32,
    stream: &mut Http2StreamState,
    decision_tx: &mpsc::Sender<Http2InterceptDecision>,
    default_host: &str,
    default_port: u16,
    send_session: &mut Http2Session,
    peer_write: &mut W,
) -> Result<(), ProxyError> {
    if stream.request_complete {
        return Ok(());
    }
    stream.request_complete = true;
    let meta = parse_http2_request_meta(&stream.request_headers, default_host, default_port)?;
    let request_id = stream.request_id.unwrap_or_else(Uuid::new_v4);
    stream.request_id = Some(request_id);
    stream.host = Some(meta.host.clone());
    stream.port = Some(meta.port);
    stream.path = Some(meta.path.clone());
    stream.scheme = Some(meta.scheme.clone());

    let started_at = stream
        .request_started_at
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let scope_status = stream.scope_status.clone().unwrap_or_else(|| {
        let in_scope = is_in_scope(&state.config.scope.rules, &meta.host, &meta.path);
        if in_scope {
            "in_scope".to_string()
        } else {
            "out_of_scope".to_string()
        }
    });
    let request_bytes = synthesize_http2_request_bytes(&meta, &stream.request_headers, &stream.request_body);
    let timeline_request = build_http2_timeline_request(
        &meta,
        request_bytes.clone(),
        stream.request_body.clone(),
        started_at,
        &scope_status,
    );
    let proxy_request = ProxyRequest {
        id: request_id,
        timeline: timeline_request,
        raw_request: request_bytes,
    };
    stream.proxy_request = Some(proxy_request.clone());

    if stream.request_intercept {
        let mut intercepts = state.intercepts.lock().await;
        let request_intercept = intercepts.intercept_request(request_id, proxy_request.clone());
        drop(intercepts);
        match request_intercept {
            InterceptResult::Forward(proxy_request) => {
                let _ = decision_tx
                    .send(Http2InterceptDecision::Request {
                        stream_id,
                        decision: InterceptDecision::Allow(proxy_request),
                    })
                    .await;
            }
            InterceptResult::Intercepted { receiver, .. } => {
                send_proxy_event(
                    state,
                    request_id,
                    ProxyEventKind::RequestIntercepted,
                    Some(proxy_request),
                    None,
                )
                .await;
                let decision_tx = decision_tx.clone();
                tokio::spawn(async move {
                    if let Ok(decision) = receiver.await {
                        let _ = decision_tx
                            .send(Http2InterceptDecision::Request { stream_id, decision })
                            .await;
                    }
                });
            }
        }
    } else if stream.request_sent {
        send_proxy_event(
            state,
            request_id,
            ProxyEventKind::RequestForwarded,
            Some(proxy_request),
            None,
        )
        .await;
    } else {
        let end_stream = stream.request_body.is_empty();
        let max_frame_size = send_session.max_frame_size();
        send_headers_logged(
            peer_write,
            &mut send_session.hpack_encoder,
            max_frame_size,
            stream_id,
            end_stream,
            &stream.request_headers,
            "upstream",
        )
        .await?;
        let body = stream.request_body.clone();
        send_data_with_flow(
            send_session,
            peer_write,
            stream,
            stream_id,
            &body,
            true,
            "upstream",
            true,
        )
        .await?;
        stream.request_sent = true;
        send_proxy_event(
            state,
            request_id,
            ProxyEventKind::RequestForwarded,
            Some(proxy_request),
            None,
        )
        .await;
    }

    Ok(())
}

async fn finalize_http2_response<W: AsyncWrite + Unpin>(
    state: &Arc<ProxyState>,
    stream_id: u32,
    stream: &mut Http2StreamState,
    decision_tx: &mpsc::Sender<Http2InterceptDecision>,
    send_session: &mut Http2Session,
    peer_write: &mut W,
) -> Result<bool, ProxyError> {
    if stream.response_complete {
        return Ok(false);
    }
    stream.response_complete = true;

    let request_id = match stream.request_id {
        Some(id) => id,
        None => return Ok(true),
    };
    let status_code = parse_http2_status(&stream.response_headers)?;
    let response_bytes =
        synthesize_http2_response_bytes(status_code, &stream.response_headers, &stream.response_body);
    let timeline_response = build_http2_timeline_response(
        status_code,
        response_bytes.clone(),
        stream.response_body.clone(),
        chrono::Utc::now().to_rfc3339(),
    );
    let proxy_response = ProxyResponse {
        id: Uuid::new_v4(),
        timeline: timeline_response,
        raw_response: response_bytes,
    };
    stream.proxy_response = Some(proxy_response.clone());

    if stream.response_intercept {
        let mut intercepts = state.intercepts.lock().await;
        let response_intercept =
            intercepts.intercept_response(request_id, proxy_response.id, proxy_response.clone());
        drop(intercepts);
        match response_intercept {
            InterceptResult::Forward(proxy_response) => {
                let _ = decision_tx
                    .send(Http2InterceptDecision::Response {
                        stream_id,
                        decision: InterceptDecision::Allow(proxy_response),
                    })
                    .await;
                return Ok(false);
            }
            InterceptResult::Intercepted { receiver, .. } => {
                send_proxy_event(
                    state,
                    request_id,
                    ProxyEventKind::ResponseIntercepted,
                    stream.proxy_request.clone(),
                    Some(proxy_response),
                )
                .await;
                let decision_tx = decision_tx.clone();
                tokio::spawn(async move {
                    if let Ok(decision) = receiver.await {
                        let _ = decision_tx
                            .send(Http2InterceptDecision::Response { stream_id, decision })
                            .await;
                    }
                });
                return Ok(false);
            }
        }
    } else if stream.response_sent {
        if let Some(request) = stream.proxy_request.clone() {
            send_proxy_event(
                state,
                request_id,
                ProxyEventKind::ResponseForwarded,
                Some(request),
                Some(proxy_response),
            )
            .await;
        }
        return Ok(true);
    } else if let Some(request) = stream.proxy_request.clone() {
        let end_stream = stream.response_body.is_empty();
        let max_frame_size = send_session.max_frame_size();
        send_headers_logged(
            peer_write,
            &mut send_session.hpack_encoder,
            max_frame_size,
            stream_id,
            end_stream,
            &stream.response_headers,
            "client",
        )
        .await?;
        let body = stream.response_body.clone();
        send_data_with_flow(
            send_session,
            peer_write,
            stream,
            stream_id,
            &body,
            true,
            "client",
            false,
        )
        .await?;
        stream.response_sent = true;
        send_proxy_event(
            state,
            request_id,
            ProxyEventKind::ResponseForwarded,
            Some(request),
            Some(proxy_response),
        )
        .await;
        return Ok(true);
    }

    Ok(true)
}

async fn send_preface_and_settings<W: AsyncWrite + Unpin>(
    writer: &mut W,
    settings: &Http2Settings,
) -> Result<(), ProxyError> {
    writer
        .write_all(HTTP2_PREFACE)
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    send_settings_frame(writer, settings, false).await
}

async fn send_settings_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    settings: &Http2Settings,
    ack: bool,
) -> Result<(), ProxyError> {
    let payload = if ack { Vec::new() } else { build_settings_payload(settings) };
    let flags = if ack { 0x1 } else { 0x0 };
    let frame = encode_raw_frame(crossfeed_net::FrameType::Settings, flags, 0, &payload);
    writer
        .write_all(&frame)
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    Ok(())
}

fn build_settings_payload(settings: &Http2Settings) -> Vec<u8> {
    let mut payload = Vec::with_capacity(30);
    payload.extend_from_slice(&0x1u16.to_be_bytes());
    payload.extend_from_slice(&settings.header_table_size.to_be_bytes());
    payload.extend_from_slice(&0x2u16.to_be_bytes());
    payload.extend_from_slice(&(settings.enable_push as u32).to_be_bytes());
    payload.extend_from_slice(&0x4u16.to_be_bytes());
    payload.extend_from_slice(&settings.initial_window_size.to_be_bytes());
    payload.extend_from_slice(&0x5u16.to_be_bytes());
    payload.extend_from_slice(&settings.max_frame_size.to_be_bytes());
    payload.extend_from_slice(&0x6u16.to_be_bytes());
    payload.extend_from_slice(&settings.max_header_list_size.to_be_bytes());
    payload
}

async fn send_headers_logged<W: AsyncWrite + Unpin>(
    writer: &mut W,
    encoder: &mut HpackEncoder,
    max_frame_size: usize,
    stream_id: u32,
    end_stream: bool,
    headers: &[crossfeed_net::HeaderField],
    _direction_label: &str,
) -> Result<(), ProxyError> {
    let frames = encode_headers_from_fields(
        stream_id,
        end_stream,
        headers,
        encoder,
        max_frame_size,
    );
        write_frames(writer, &frames).await
}

async fn send_data<W: AsyncWrite + Unpin>(
    writer: &mut W,
    max_frame_size: usize,
    stream_id: u32,
    end_stream: bool,
    payload: &[u8],
) -> Result<(), ProxyError> {
    let frames = encode_data_frames(stream_id, end_stream, payload, max_frame_size);
    write_frames(writer, &frames).await
}

async fn send_window_updates<W: AsyncWrite + Unpin>(
    writer: &mut W,
    updates: &[WindowUpdate],
    _direction_label: &str,
) -> Result<(), ProxyError> {
    for update in updates {
        let payload = (update.increment & 0x7FFF_FFFF).to_be_bytes();
        let frame = encode_raw_frame(
            crossfeed_net::FrameType::WindowUpdate,
            0,
            update.stream_id,
            &payload,
        );
        writer
            .write_all(&frame)
            .await
            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    }
    writer
        .flush()
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    Ok(())
}

async fn send_data_with_flow<W: AsyncWrite + Unpin>(
    session: &mut Http2Session,
    writer: &mut W,
    stream: &mut Http2StreamState,
    stream_id: u32,
    data: &[u8],
    end_stream: bool,
    direction_label: &str,
    is_request: bool,
) -> Result<(), ProxyError> {
    if is_request && !stream.request_sent {
        println!(
            "ERROR: H2 invalid state data before headers stream={} dir={}",
            stream_id, direction_label
        );
        send_rst_stream(writer, stream_id).await?;
        return Ok(());
    }
    if !is_request && !stream.response_sent {
        println!(
            "ERROR: H2 invalid state data before headers stream={} dir={}",
            stream_id, direction_label
        );
        send_rst_stream(writer, stream_id).await?;
        return Ok(());
    }
    if is_request {
        stream.pending_request_data.extend_from_slice(data);
        if end_stream {
            stream.pending_request_end_stream = true;
        }
        let (pending_data, pending_end_stream, end_stream_sent) = (
            &mut stream.pending_request_data,
            &mut stream.pending_request_end_stream,
            &mut stream.request_end_stream_sent,
        );
        flush_pending_data_inner(
            session,
            writer,
            stream_id,
            pending_data,
            pending_end_stream,
            direction_label,
            end_stream_sent,
        )
        .await?;
    } else {
        stream.pending_response_data.extend_from_slice(data);
        if end_stream {
            stream.pending_response_end_stream = true;
        }
        let (pending_data, pending_end_stream, end_stream_sent) = (
            &mut stream.pending_response_data,
            &mut stream.pending_response_end_stream,
            &mut stream.response_end_stream_sent,
        );
        flush_pending_data_inner(
            session,
            writer,
            stream_id,
            pending_data,
            pending_end_stream,
            direction_label,
            end_stream_sent,
        )
        .await?;
    }
    Ok(())
}

async fn send_rst_stream<W: AsyncWrite + Unpin>(
    writer: &mut W,
    stream_id: u32,
) -> Result<(), ProxyError> {
    let frame = encode_raw_frame(
        crossfeed_net::FrameType::RstStream,
        0,
        stream_id,
        &0x1u32.to_be_bytes(),
    );
    writer
        .write_all(&frame)
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    Ok(())
}

async fn flush_pending_data_inner<W: AsyncWrite + Unpin>(
    session: &mut Http2Session,
    writer: &mut W,
    stream_id: u32,
    buffer: &mut Vec<u8>,
    pending_end_stream: &mut bool,
    _direction_label: &str,
    end_stream_sent: &mut bool,
) -> Result<(), ProxyError> {
    if buffer.is_empty() && *pending_end_stream {
        if *end_stream_sent {
            *pending_end_stream = false;
            return Ok(());
        }
        send_data(writer, session.max_frame_size(), stream_id, true, &[]).await?;
        let _stream_window_value = *session.send_stream_window(stream_id);
        *pending_end_stream = false;
        *end_stream_sent = true;
        return Ok(());
    }

    loop {
        if buffer.is_empty() {
            break;
        }
        let available = session
            .send_conn_window
            .min(*session.send_stream_window(stream_id));
        if available <= 0 {
            break;
        }
        let max_frame = session.max_frame_size();
        let chunk_len = buffer
            .len()
            .min(available as usize)
            .min(max_frame);
        let chunk = buffer.drain(..chunk_len).collect::<Vec<u8>>();
        let end_stream = buffer.is_empty() && *pending_end_stream;
        send_data(writer, max_frame, stream_id, end_stream, &chunk).await?;
        session.send_conn_window -= chunk_len as i32;
        let stream_window = session.send_stream_window(stream_id);
        *stream_window -= chunk_len as i32;
        let _stream_window_value = *stream_window;
        if end_stream {
            *pending_end_stream = false;
            *end_stream_sent = true;
            break;
        }
    }
    Ok(())
}

async fn flush_pending_data<W: AsyncWrite + Unpin>(
    direction: Direction,
    session: &mut Http2Session,
    writer: &mut W,
    streams: &mut HashMap<u32, Http2StreamState>,
) -> Result<(), ProxyError> {
    let direction_label = match direction {
        Direction::ClientToUpstream => "client",
        Direction::UpstreamToClient => "upstream",
    };
    for (stream_id, stream) in streams.iter_mut() {
        match direction {
            Direction::ClientToUpstream => {
                if !stream.pending_response_data.is_empty() || stream.pending_response_end_stream {
                    let (pending_data, pending_end_stream, end_stream_sent) = (
                        &mut stream.pending_response_data,
                        &mut stream.pending_response_end_stream,
                        &mut stream.response_end_stream_sent,
                    );
                    flush_pending_data_inner(
                        session,
                        writer,
                        *stream_id,
                        pending_data,
                        pending_end_stream,
                        direction_label,
                        end_stream_sent,
                    )
                    .await?;
                }
            }
            Direction::UpstreamToClient => {
                if !stream.pending_request_data.is_empty() || stream.pending_request_end_stream {
                    let (pending_data, pending_end_stream, end_stream_sent) = (
                        &mut stream.pending_request_data,
                        &mut stream.pending_request_end_stream,
                        &mut stream.request_end_stream_sent,
                    );
                    flush_pending_data_inner(
                        session,
                        writer,
                        *stream_id,
                        pending_data,
                        pending_end_stream,
                        direction_label,
                        end_stream_sent,
                    )
                    .await?;
                }
            }
        }
    }
    Ok(())
}


fn parse_promised_stream_id(payload: &[u8], flags: u8) -> Option<u32> {
    let mut offset = 0;
    if flags & 0x8 != 0 {
        offset = 1;
    }
    if payload.len() < offset + 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&payload[offset..offset + 4]);
    Some(u32::from_be_bytes(bytes) & 0x7FFF_FFFF)
}


async fn write_frames<W: AsyncWrite + Unpin>(
    writer: &mut W,
    frames: &[Vec<u8>],
) -> Result<(), ProxyError> {
    for frame in frames {
        writer
            .write_all(frame)
            .await
            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    }
    Ok(())
}

fn parse_http2_request_meta(
    headers: &[crossfeed_net::HeaderField],
    default_host: &str,
    default_port: u16,
) -> Result<Http2RequestMeta, ProxyError> {
    let mut method = None;
    let mut scheme = None;
    let mut authority = None;
    let mut path = None;

    for header in headers {
        match header.name.as_slice() {
            b":method" => method = Some(String::from_utf8_lossy(&header.value).to_string()),
            b":scheme" => scheme = Some(String::from_utf8_lossy(&header.value).to_string()),
            b":authority" => authority = Some(String::from_utf8_lossy(&header.value).to_string()),
            b":path" => path = Some(String::from_utf8_lossy(&header.value).to_string()),
            _ => {}
        }
    }

    let method = method.ok_or_else(|| ProxyError::Runtime("missing :method".to_string()))?;
    let scheme = scheme.unwrap_or_else(|| default_scheme_for_port(default_port).to_string());
    let authority = authority.unwrap_or_else(|| format_authority(default_host, default_port, &scheme));
    let path = path.unwrap_or_else(|| "/".to_string());
    let (host, port) = split_host_port_with_scheme(&authority, &scheme, default_port);

    Ok(Http2RequestMeta {
        method,
        scheme,
        authority,
        host,
        port,
        path,
    })
}

fn parse_http2_status(headers: &[crossfeed_net::HeaderField]) -> Result<u16, ProxyError> {
    for header in headers {
        if header.name.as_slice() == b":status" {
            let status = String::from_utf8_lossy(&header.value)
                .parse::<u16>()
                .map_err(|_| ProxyError::Runtime("invalid :status".to_string()))?;
            return Ok(status);
        }
    }
    Err(ProxyError::Runtime("missing :status".to_string()))
}

fn synthesize_http2_request_bytes(
    meta: &Http2RequestMeta,
    headers: &[crossfeed_net::HeaderField],
    body: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(format!("{} {} HTTP/2\r\n", meta.method, meta.path).as_bytes());
    let mut has_host = false;
    for header in headers {
        if header.name.starts_with(b":") {
            continue;
        }
        let name = String::from_utf8_lossy(&header.name).to_string();
        if name.eq_ignore_ascii_case("host") {
            has_host = true;
        }
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(b": ");
        bytes.extend_from_slice(&header.value);
        bytes.extend_from_slice(b"\r\n");
    }
    if !has_host {
        bytes.extend_from_slice(format!("Host: {}\r\n", meta.authority).as_bytes());
    }
    bytes.extend_from_slice(b"\r\n");
    bytes.extend_from_slice(body);
    bytes
}

fn synthesize_http2_response_bytes(
    status_code: u16,
    headers: &[crossfeed_net::HeaderField],
    body: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(format!("HTTP/2 {}\r\n", status_code).as_bytes());
    for header in headers {
        if header.name.starts_with(b":") {
            continue;
        }
        let name = String::from_utf8_lossy(&header.name).to_string();
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(b": ");
        bytes.extend_from_slice(&header.value);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"\r\n");
    bytes.extend_from_slice(body);
    bytes
}

fn build_http2_timeline_request(
    meta: &Http2RequestMeta,
    headers: Vec<u8>,
    body: Vec<u8>,
    started_at: String,
    scope_status: &str,
) -> TimelineRequest {
    TimelineRequest {
        source: "proxy".to_string(),
        method: meta.method.clone(),
        scheme: meta.scheme.clone(),
        host: meta.host.clone(),
        port: meta.port,
        path: meta.path.clone(),
        query: None,
        url: format!("{}://{}{}", meta.scheme, meta.host, meta.path),
        http_version: "HTTP/2".to_string(),
        request_headers: headers,
        request_body: body.clone(),
        request_body_size: body.len(),
        request_body_truncated: false,
        started_at,
        completed_at: None,
        duration_ms: None,
        scope_status_at_capture: scope_status.to_string(),
        scope_status_current: None,
        scope_rules_version: 1,
        capture_filtered: false,
        timeline_filtered: false,
    }
}

fn build_http2_timeline_response(
    status_code: u16,
    headers: Vec<u8>,
    body: Vec<u8>,
    received_at: String,
) -> TimelineResponse {
    TimelineResponse {
        timeline_request_id: 0,
        status_code,
        reason: None,
        response_headers: headers,
        response_body: body.clone(),
        response_body_size: body.len(),
        response_body_truncated: false,
        http_version: "HTTP/2".to_string(),
        received_at,
    }
}

fn parse_http1_request(raw: &[u8]) -> Result<crossfeed_net::Request, ProxyError> {
    let mut parser = RequestParser::new();
    match parser.push(raw) {
        crossfeed_net::ParseStatus::Complete { message, .. } => Ok(message),
        crossfeed_net::ParseStatus::Error { error, .. } => {
            Err(ProxyError::Runtime(format!("parse error {error:?}")))
        }
        crossfeed_net::ParseStatus::NeedMore { .. } => {
            Err(ProxyError::Runtime("incomplete request".to_string()))
        }
    }
}

fn parse_http1_response(raw: &[u8]) -> Result<crossfeed_net::Response, ProxyError> {
    let mut parser = ResponseParser::new();
    match parser.push(raw) {
        crossfeed_net::ParseStatus::Complete { message, .. } => Ok(message),
        crossfeed_net::ParseStatus::Error { error, .. } => {
            Err(ProxyError::Runtime(format!("parse error {error:?}")))
        }
        crossfeed_net::ParseStatus::NeedMore { .. } => {
            Err(ProxyError::Runtime("incomplete response".to_string()))
        }
    }
}

fn parse_http1_response_with_limits(
    raw: &[u8],
    limits: crossfeed_net::Limits,
) -> Result<crossfeed_net::Response, ProxyError> {
    let mut parser = ResponseParser::with_limits(limits);
    match parser.push(raw) {
        crossfeed_net::ParseStatus::Complete { message, .. } => Ok(message),
        crossfeed_net::ParseStatus::Error { error, .. } => {
            Err(ProxyError::Runtime(format!("parse error {error:?}")))
        }
        crossfeed_net::ParseStatus::NeedMore { .. } => {
            Err(ProxyError::Runtime("incomplete response".to_string()))
        }
    }
}

fn http1_request_to_h2(
    request: &crossfeed_net::Request,
    default_scheme: &str,
    default_authority: &str,
) -> Result<(Http2RequestMeta, Vec<crossfeed_net::HeaderField>), ProxyError> {
    let mut scheme = default_scheme.to_string();
    let mut authority = default_authority.to_string();
    let mut path = request.line.target.clone();

    if request.line.target.starts_with("http://") || request.line.target.starts_with("https://") {
        if let Ok(url) = url::Url::parse(&request.line.target) {
            scheme = url.scheme().to_string();
            authority = url
                .host_str()
                .unwrap_or(default_authority)
                .to_string();
            if let Some(port) = url.port() {
                authority = format!("{}:{}", authority, port);
            }
            path = url.path().to_string();
            if let Some(query) = url.query() {
                path.push('?');
                path.push_str(query);
            }
        }
    } else if let Some(host) = request
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("host"))
    {
        authority = host.value.clone();
    }

    let (host, port) = split_host_port_with_scheme(&authority, &scheme, 443);
    let meta = Http2RequestMeta {
        method: request.line.method.clone(),
        scheme: scheme.clone(),
        authority: authority.clone(),
        host,
        port,
        path: path.clone(),
    };
    let mut headers = Vec::new();
    headers.push(crossfeed_net::HeaderField {
        name: b":method".to_vec(),
        value: request.line.method.as_bytes().to_vec(),
    });
    headers.push(crossfeed_net::HeaderField {
        name: b":scheme".to_vec(),
        value: scheme.as_bytes().to_vec(),
    });
    headers.push(crossfeed_net::HeaderField {
        name: b":authority".to_vec(),
        value: authority.as_bytes().to_vec(),
    });
    headers.push(crossfeed_net::HeaderField {
        name: b":path".to_vec(),
        value: path.as_bytes().to_vec(),
    });

    for header in &request.headers {
        let name = header.name.to_ascii_lowercase();
        if name == "host"
            || name == "connection"
            || name == "proxy-connection"
            || name == "transfer-encoding"
            || name == "upgrade"
        {
            continue;
        }
        headers.push(crossfeed_net::HeaderField {
            name: name.as_bytes().to_vec(),
            value: header.value.as_bytes().to_vec(),
        });
    }

    Ok((meta, headers))
}

fn http1_response_to_h2(response: &crossfeed_net::Response) -> Vec<crossfeed_net::HeaderField> {
    let mut headers = Vec::new();
    headers.push(crossfeed_net::HeaderField {
        name: b":status".to_vec(),
        value: response.line.status_code.to_string().as_bytes().to_vec(),
    });
    for header in &response.headers {
        let name = header.name.to_ascii_lowercase();
        if name == "connection"
            || name == "proxy-connection"
            || name == "transfer-encoding"
            || name == "upgrade"
        {
            continue;
        }
        headers.push(crossfeed_net::HeaderField {
            name: name.as_bytes().to_vec(),
            value: header.value.as_bytes().to_vec(),
        });
    }
    headers
}

fn default_scheme_for_port(port: u16) -> &'static str {
    if port == 443 {
        "https"
    } else {
        "http"
    }
}

fn protocol_from_alpn(selected: Option<&[u8]>) -> NegotiatedProtocol {
    match selected {
        Some(b"h2") => NegotiatedProtocol::Http2,
        _ => NegotiatedProtocol::Http1,
    }
}

fn protocol_name(protocol: NegotiatedProtocol) -> &'static str {
    match protocol {
        NegotiatedProtocol::Http1 => "http/1.1",
        NegotiatedProtocol::Http2 => "h2",
    }
}

fn alpn_list(preferred: NegotiatedProtocol, include_fallback: bool) -> Vec<String> {
    let mut list = vec![protocol_name(preferred).to_string()];
    if include_fallback {
        let fallback = if preferred == NegotiatedProtocol::Http2 {
            "http/1.1"
        } else {
            "h2"
        };
        if fallback != list[0] {
            list.push(fallback.to_string());
        }
    }
    list
}

fn format_authority(host: &str, port: u16, scheme: &str) -> String {
    let default_port = if scheme == "http" { 80 } else { 443 };
    if port == default_port {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

fn split_host_port_with_scheme(host: &str, scheme: &str, default_port: u16) -> (String, u16) {
    if let Some((host, port)) = host.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            return (host.to_string(), port);
        }
    }
    let port = if scheme == "http" { 80 } else { default_port };
    (host.to_string(), port)
}

async fn send_proxy_event(
    state: &Arc<ProxyState>,
    request_id: Uuid,
    kind: ProxyEventKind,
    request: Option<ProxyRequest>,
    response: Option<ProxyResponse>,
) {
    let _ = state
        .sender
        .send(ProxyEvent {
            event_id: Uuid::new_v4(),
            request_id,
            kind,
            request,
            response,
        })
        .await;
}

async fn handle_http1(
    state: Arc<ProxyState>,
    client: TcpStream,
    buffer: Vec<u8>,
) -> Result<(), ProxyError> {
    handle_http1_tcp(state, client, buffer).await
}

enum UpstreamWriteMode {
    FullRequest,
    HeadersOnly,
    BodyOnly,
}

async fn handle_http1_tcp(
    state: Arc<ProxyState>,
    mut client: TcpStream,
    mut buffer: Vec<u8>,
) -> Result<(), ProxyError> {
    let request_limits = http1_request_limits(&state.config);
    let mut parser = RequestStreamParser::with_limits(request_limits);
    let mut request_bytes = Vec::new();
    let mut expect_continue = false;
    let mut expect_info: Option<crossfeed_net::RequestFrameInfo> = None;
    let mut expect_header_end: Option<usize> = None;
    let mut expect_upstream: Option<TcpStream> = None;

    loop {
        if buffer.is_empty() {
            let mut temp = vec![0u8; 8192];
            let n = client.read(&mut temp).await?;
            if n == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&temp[..n]);
        }

        request_bytes.extend_from_slice(&buffer);
        let events = parser
            .push(&buffer)
            .map_err(|error| ProxyError::Runtime(format!("parse error {error:?}")))?;
        buffer.clear();

        for event in events {
            match event {
                RequestStreamEvent::Headers(info) => {
                    let method = info.method.to_ascii_uppercase();
                    let _host = info
                        .headers
                        .iter()
                        .find(|header| header.name.eq_ignore_ascii_case("host"))
                        .map(|header| header.value.as_str())
                        .unwrap_or("");
                    if method == "CONNECT" {
                        handle_connect(Arc::clone(&state), &mut client, info.target.clone())
                            .await?;
                        return Ok(());
                    }
                    expect_info = Some(info);
                    expect_header_end = find_header_end(&request_bytes).map(|end| end + 4);
                }
                RequestStreamEvent::ExpectContinue => {
                    let request_intercept_enabled = {
                        let intercepts = state.intercepts.lock().await;
                        intercepts.is_request_intercept_enabled()
                    };
                    expect_continue = true;
                    if !request_intercept_enabled {
                        if expect_upstream.is_none() {
                            let Some(info) = expect_info.clone() else {
                                continue;
                            };
                            let Some(header_end) = expect_header_end else {
                                continue;
                            };
                            let header_bytes = request_bytes[..header_end].to_vec();
                            let request_stub = crossfeed_net::Request {
                                line: crossfeed_net::RequestLine {
                                    method: info.method.clone(),
                                    target: info.target.clone(),
                                    version: info.version.clone(),
                                },
                                headers: info.headers.clone(),
                                body: Vec::new(),
                            };
                            let (host, port, _path) = resolve_target(
                                &request_stub.line.target,
                                &request_stub.headers,
                            )
                            .ok_or_else(|| ProxyError::Runtime("missing host".to_string()))?;
                            let mut upstream =
                                connect_upstream(&state.config, host.clone(), port).await?;
                            upstream
                                .write_all(&header_bytes)
                                .await
                                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                            upstream
                                .flush()
                                .await
                                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                            if let Some(peek_bytes) = peek_upstream_response(
                                &mut upstream,
                                http1_response_limits(&state.config),
                            )
                            .await?
                            {
                                let response = read_response_streaming(
                                    &mut upstream,
                                    &mut client,
                                    http1_response_limits(&state.config),
                                    &request_stub,
                                    Some(peek_bytes),
                                )
                                .await?;
                                if response.should_close {
                                    close_http1_connection(&mut client, Some(&mut upstream)).await;
                                }
                                return Ok(());
                            }
                            client
                                .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                                .await
                                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                            client
                                .flush()
                                .await
                                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                            expect_upstream = Some(upstream);
                        }
                    }
                }
                RequestStreamEvent::EndOfMessage => {
                    let message = parse_http1_request(&request_bytes)?;
                    if expect_continue {
                        if let Some(upstream) = expect_upstream.as_mut() {
                            handle_http1_request(
                                Arc::clone(&state),
                                &mut client,
                                Some(upstream),
                                message,
                                UpstreamWriteMode::BodyOnly,
                                false,
                            )
                            .await?;
                        } else {
                            handle_http1_request(
                                Arc::clone(&state),
                                &mut client,
                                None::<&mut TcpStream>,
                                message,
                                UpstreamWriteMode::FullRequest,
                                true,
                            )
                            .await?;
                        }
                    } else {
                        handle_http1_request(
                            Arc::clone(&state),
                            &mut client,
                            None::<&mut TcpStream>,
                            message,
                            UpstreamWriteMode::FullRequest,
                            false,
                        )
                        .await?;
                    }
                    request_bytes.clear();
                    parser = RequestStreamParser::with_limits(request_limits);
                    expect_continue = false;
                    expect_info = None;
                    expect_header_end = None;
                    expect_upstream = None;
                }
                RequestStreamEvent::BodyBytes { .. } => {}
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
    let request_limits = http1_request_limits(&state.config);
    let mut parser = RequestStreamParser::with_limits(request_limits);
    let mut request_bytes = Vec::new();
    let mut expect_continue = false;
    let mut expect_info: Option<crossfeed_net::RequestFrameInfo> = None;
    let mut expect_header_end: Option<usize> = None;

    loop {
        if buffer.is_empty() {
            let mut temp = vec![0u8; 8192];
            let n = client.read(&mut temp).await?;
            if n == 0 {
                return Ok(());
            }
            buffer.extend_from_slice(&temp[..n]);
        }

        request_bytes.extend_from_slice(&buffer);
        let events = parser
            .push(&buffer)
            .map_err(|error| ProxyError::Runtime(format!("parse error {error:?}")))?;
        buffer.clear();

        for event in events {
            match event {
                RequestStreamEvent::Headers(info) => {
                    let _method = info.method.to_ascii_uppercase();
                    let _host = info
                        .headers
                        .iter()
                        .find(|header| header.name.eq_ignore_ascii_case("host"))
                        .map(|header| header.value.as_str())
                        .unwrap_or("");
                    expect_info = Some(info);
                    expect_header_end = find_header_end(&request_bytes).map(|end| end + 4);
                }
                RequestStreamEvent::ExpectContinue => {
                    let request_intercept_enabled = {
                        let intercepts = state.intercepts.lock().await;
                        intercepts.is_request_intercept_enabled()
                    };
                    expect_continue = true;
                    if !request_intercept_enabled {
                        let Some(info) = expect_info.clone() else {
                            continue;
                        };
                        let Some(header_end) = expect_header_end else {
                            continue;
                        };
                        let header_bytes = request_bytes[..header_end].to_vec();
                        let request_stub = crossfeed_net::Request {
                            line: crossfeed_net::RequestLine {
                                method: info.method.clone(),
                                target: info.target.clone(),
                                version: info.version.clone(),
                            },
                            headers: info.headers.clone(),
                            body: Vec::new(),
                        };
                        upstream
                            .write_all(&header_bytes)
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        if let Some(peek_bytes) = peek_upstream_response(
                            &mut upstream,
                            http1_response_limits(&state.config),
                        )
                        .await?
                        {
                            let response = read_response_streaming(
                                &mut upstream,
                                &mut client,
                                http1_response_limits(&state.config),
                                &request_stub,
                                Some(peek_bytes),
                            )
                            .await?;
                            if response.should_close {
                                close_http1_connection(&mut client, Some(&mut upstream)).await;
                            }
                            return Ok(());
                        }
                        client
                            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        client
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                    }
                }
                RequestStreamEvent::EndOfMessage => {
                    let message = parse_http1_request(&request_bytes)?;
                    handle_http1_request(
                        Arc::clone(&state),
                        &mut client,
                        Some(&mut upstream),
                        message,
                        if expect_continue {
                            UpstreamWriteMode::BodyOnly
                        } else {
                            UpstreamWriteMode::FullRequest
                        },
                        expect_continue,
                    )
                    .await?;
                    request_bytes.clear();
                    parser = RequestStreamParser::with_limits(request_limits);
                    expect_continue = false;
                    expect_info = None;
                    expect_header_end = None;
                }
                RequestStreamEvent::BodyBytes { .. } => {}
            }
        }
    }
}

async fn handle_http1_request<C, U>(
    state: Arc<ProxyState>,
    client: &mut C,
    mut upstream: Option<&mut U>,
    message: crossfeed_net::Request,
    write_mode: UpstreamWriteMode,
    expect_continue: bool,
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

    let response_intercept_enabled = {
        let intercepts = state.intercepts.lock().await;
        intercepts.is_response_intercept_enabled()
            || intercepts.should_intercept_response_for_request(request_id)
    };

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

            if !response_intercept_enabled {
                let limits = http1_response_limits(&state.config);
                let streamed = match upstream.as_mut() {
                    Some(upstream) => {
                        log_http1_upstream_send(request_id, &host, port, true);
                        if expect_continue && matches!(write_mode, UpstreamWriteMode::FullRequest) {
                            let header_write = write_http1_request(
                                upstream,
                                &proxy_request.raw_request,
                                UpstreamWriteMode::HeadersOnly,
                            )
                            .await;
                            if header_write.is_ok() {
                                upstream
                                    .flush()
                                    .await
                                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                                if let Some(peek_bytes) = peek_upstream_response(
                                    upstream,
                                    http1_response_limits(&state.config),
                                )
                                .await?
                                {
                                    let response = read_response_streaming(
                                        upstream,
                                        client,
                                        limits,
                                        &message,
                                        Some(peek_bytes),
                                    )
                                    .await?;
                                    if response.should_close {
                                        close_http1_connection(client, Some(upstream)).await;
                                    }
                                    return Ok(());
                                }
                                client
                                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                                    .await
                                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                            }
                        }
                        write_http1_request(
                            upstream,
                            &proxy_request.raw_request,
                            if expect_continue {
                                UpstreamWriteMode::BodyOnly
                            } else {
                                write_mode
                            },
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_streaming(upstream, client, limits, &message, None)
                        .await?
                    }
                    None => {
                        let mut upstream =
                            connect_upstream(&state.config, host.clone(), port).await?;
                        log_http1_upstream_send(request_id, &host, port, false);
                        write_http1_request(
                            &mut upstream,
                            &proxy_request.raw_request,
                            write_mode,
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_streaming(&mut upstream, client, limits, &message, None)
                        .await?
                    }
                };

                let proxy_response =
                    parse_response(&streamed.bytes, &started_at, limits).map(|timeline_response| {
                        ProxyResponse {
                            id: Uuid::new_v4(),
                            timeline: timeline_response,
                            raw_response: streamed.bytes,
                        }
                    });

                if let Some(proxy_response) = proxy_response {
                    let _ = state
                        .sender
                        .send(ProxyEvent {
                            event_id: Uuid::new_v4(),
                            request_id,
                            kind: ProxyEventKind::ResponseForwarded,
                            request: Some(proxy_request.clone()),
                            response: Some(proxy_response),
                        })
                        .await;
                }

                if streamed.should_close {
                    close_http1_connection(client, upstream.as_deref_mut()).await;
                    return Ok(());
                }

                return Ok(());
            }

            let response_bytes = match upstream.as_mut() {
                Some(upstream) => {
                    log_http1_upstream_send(request_id, &host, port, true);
                    let (early_response, effective_write_mode) = if expect_continue
                        && matches!(write_mode, UpstreamWriteMode::FullRequest)
                    {
                        handle_expect_continue(
                            upstream,
                            client,
                            &proxy_request.raw_request,
                            http1_response_limits(&state.config),
                            true,
                        )
                        .await?
                    } else {
                        (None, write_mode)
                    };
                    if let Some(early_response) = early_response {
                        early_response
                    } else {
                        write_http1_request(
                            upstream,
                            &proxy_request.raw_request,
                            effective_write_mode,
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_stream(upstream, http1_response_limits(&state.config))
                            .await?
                    }
                }
                None => {
                    let mut upstream = connect_upstream(&state.config, host.clone(), port).await?;
                    log_http1_upstream_send(request_id, &host, port, false);
                    let (early_response, effective_write_mode) = if expect_continue
                        && matches!(write_mode, UpstreamWriteMode::FullRequest)
                    {
                        handle_expect_continue(
                            &mut upstream,
                            client,
                            &proxy_request.raw_request,
                            http1_response_limits(&state.config),
                            true,
                        )
                        .await?
                    } else {
                        (None, write_mode)
                    };
                    if let Some(early_response) = early_response {
                        early_response
                    } else {
                        write_http1_request(
                            &mut upstream,
                            &proxy_request.raw_request,
                            effective_write_mode,
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_stream(&mut upstream, http1_response_limits(&state.config))
                            .await?
                    }
                }
            };

            (
                Some(proxy_request),
                parse_response(&response_bytes, &started_at, http1_response_limits(&state.config))
                    .map(|timeline_response| ProxyResponse {
                        id: Uuid::new_v4(),
                        timeline: timeline_response,
                        raw_response: response_bytes,
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

            if !response_intercept_enabled {
                let limits = http1_response_limits(&state.config);
                let streamed = match upstream.as_mut() {
                    Some(upstream) => {
                        log_http1_upstream_send(request_id, &host, port, true);
                        if expect_continue && matches!(write_mode, UpstreamWriteMode::FullRequest) {
                            let header_write = write_http1_request(
                                upstream,
                                &proxy_request.raw_request,
                                UpstreamWriteMode::HeadersOnly,
                            )
                            .await;
                            if header_write.is_ok() {
                                upstream
                                    .flush()
                                    .await
                                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                                if let Some(peek_bytes) = peek_upstream_response(
                                    upstream,
                                    http1_response_limits(&state.config),
                                )
                                .await?
                                {
                                    let response = read_response_streaming(
                                        upstream,
                                        client,
                                        limits,
                                        &message,
                                        Some(peek_bytes),
                                    )
                                    .await?;
                                    if response.should_close {
                                        close_http1_connection(client, Some(upstream)).await;
                                    }
                                    return Ok(());
                                }
                                client
                                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                                    .await
                                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                            }
                        }
                        write_http1_request(
                            upstream,
                            &proxy_request.raw_request,
                            if expect_continue {
                                UpstreamWriteMode::BodyOnly
                            } else {
                                write_mode
                            },
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_streaming(upstream, client, limits, &message, None)
                        .await?
                    }
                    None => {
                        let mut upstream =
                            connect_upstream(&state.config, host.clone(), port).await?;
                        log_http1_upstream_send(request_id, &host, port, false);
                        write_http1_request(
                            &mut upstream,
                            &proxy_request.raw_request,
                            write_mode,
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_streaming(&mut upstream, client, limits, &message, None)
                        .await?
                    }
                };

                let proxy_response =
                    parse_response(&streamed.bytes, &started_at, limits).map(|timeline_response| {
                        ProxyResponse {
                            id: Uuid::new_v4(),
                            timeline: timeline_response,
                            raw_response: streamed.bytes,
                        }
                    });

                if let Some(proxy_response) = proxy_response {
                    let _ = state
                        .sender
                        .send(ProxyEvent {
                            event_id: Uuid::new_v4(),
                            request_id,
                            kind: ProxyEventKind::ResponseForwarded,
                            request: Some(proxy_request.clone()),
                            response: Some(proxy_response),
                        })
                        .await;
                }

                if streamed.should_close {
                    close_http1_connection(client, upstream.as_deref_mut()).await;
                    return Ok(());
                }

                return Ok(());
            }

            let response_bytes = match upstream.as_mut() {
                Some(upstream) => {
                    log_http1_upstream_send(request_id, &host, port, true);
                    let (early_response, effective_write_mode) = if expect_continue
                        && matches!(write_mode, UpstreamWriteMode::FullRequest)
                    {
                        handle_expect_continue(
                            upstream,
                            client,
                            &proxy_request.raw_request,
                            http1_response_limits(&state.config),
                            true,
                        )
                        .await?
                    } else {
                        (None, write_mode)
                    };
                    if let Some(early_response) = early_response {
                        early_response
                    } else {
                        write_http1_request(
                            upstream,
                            &proxy_request.raw_request,
                            effective_write_mode,
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_stream(upstream, http1_response_limits(&state.config))
                            .await?
                    }
                }
                None => {
                    let mut upstream = connect_upstream(&state.config, host.clone(), port).await?;
                    log_http1_upstream_send(request_id, &host, port, false);
                    let (early_response, effective_write_mode) = if expect_continue
                        && matches!(write_mode, UpstreamWriteMode::FullRequest)
                    {
                        handle_expect_continue(
                            &mut upstream,
                            client,
                            &proxy_request.raw_request,
                            http1_response_limits(&state.config),
                            true,
                        )
                        .await?
                    } else {
                        (None, write_mode)
                    };
                    if let Some(early_response) = early_response {
                        early_response
                    } else {
                        write_http1_request(
                            &mut upstream,
                            &proxy_request.raw_request,
                            effective_write_mode,
                        )
                        .await?;
                        upstream
                            .flush()
                            .await
                            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                        read_response_stream(&mut upstream, http1_response_limits(&state.config))
                            .await?
                    }
                }
            };

            (
                Some(proxy_request),
                parse_response(&response_bytes, &started_at, http1_response_limits(&state.config))
                    .map(|timeline_response| ProxyResponse {
                        id: Uuid::new_v4(),
                        timeline: timeline_response,
                        raw_response: response_bytes,
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
                    .map_err(|err| {
                        println!("ERROR: H1 write failed: {}", err);
                        ProxyError::Runtime(err.to_string())
                    })?;
                let should_close = parse_http1_response_with_limits(
                    &proxy_response.raw_response,
                    http1_response_limits(&state.config),
                )
                .map(|response| should_close_from_response(&message, &response))
                .unwrap_or(true);
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
                if should_close {
                    close_http1_connection(client, upstream.as_deref_mut()).await;
                    return Ok(());
                }
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
                            .map_err(|err| {
                                println!("ERROR: H1 write failed: {}", err);
                                ProxyError::Runtime(err.to_string())
                            })?;
                        let should_close = parse_http1_response_with_limits(
                            &proxy_response.raw_response,
                            http1_response_limits(&state.config),
                        )
                        .map(|response| should_close_from_response(&message, &response))
                        .unwrap_or(true);
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
                        if should_close {
                            close_http1_connection(client, upstream.as_deref_mut()).await;
                            return Ok(());
                        }
                    }
                    InterceptDecision::Drop => {}
                }
            }
        }
    }

    Ok(())
}

fn http1_request_limits(config: &ProxyConfig) -> crossfeed_net::Limits {
    crossfeed_net::Limits {
        max_header_bytes: config.http1_max_header_bytes,
        max_body_bytes: config.body_limits.request_max_bytes,
    }
}

async fn write_http1_request<S>(
    stream: &mut S,
    raw_request: &[u8],
    mode: UpstreamWriteMode,
) -> Result<(), ProxyError>
where
    S: AsyncWrite + Unpin,
{
    match mode {
        UpstreamWriteMode::FullRequest => {
            stream
                .write_all(raw_request)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
        }
        UpstreamWriteMode::HeadersOnly => {
            if let Some(header_end) = find_header_end(raw_request) {
                let header_end = header_end + 4;
                stream
                    .write_all(&raw_request[..header_end])
                    .await
                    .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            }
        }
        UpstreamWriteMode::BodyOnly => {
            if let Some(header_end) = find_header_end(raw_request) {
                let body_start = header_end + 4;
                if body_start < raw_request.len() {
                    stream
                        .write_all(&raw_request[body_start..])
                        .await
                        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
                }
            }
        }
    }
    Ok(())
}

async fn handle_expect_continue<S, C>(
    upstream: &mut S,
    client: &mut C,
    raw_request: &[u8],
    limits: crossfeed_net::Limits,
    expect_continue: bool,
) -> Result<(Option<Vec<u8>>, UpstreamWriteMode), ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: AsyncRead + AsyncWrite + Unpin,
{
    if !expect_continue {
        return Ok((None, UpstreamWriteMode::FullRequest));
    }

    write_http1_request(upstream, raw_request, UpstreamWriteMode::HeadersOnly).await?;
    upstream
        .flush()
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    if let Some(peek_bytes) = peek_upstream_response(upstream, limits).await? {
        let response_bytes = read_response_buffered_with_initial(upstream, limits, peek_bytes)
            .await?;
        return Ok((Some(response_bytes), UpstreamWriteMode::HeadersOnly));
    }

    client
        .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    client
        .flush()
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    Ok((None, UpstreamWriteMode::BodyOnly))
}

fn http1_response_limits(config: &ProxyConfig) -> crossfeed_net::Limits {
    crossfeed_net::Limits {
        max_header_bytes: config.http1_max_header_bytes,
        max_body_bytes: config.body_limits.response_max_bytes,
    }
}

fn should_close_from_frame(
    request: &crossfeed_net::Request,
    frame: &crossfeed_net::ResponseFrameInfo,
) -> bool {
    request_should_close(request) || frame.connection_close || frame.close_delimited
}

fn should_close_from_response(
    request: &crossfeed_net::Request,
    response: &crossfeed_net::Response,
) -> bool {
    let response_close = response_should_close(&response.line.version, &response.headers);
    let close_delimited = response_close_delimited_from_headers(response);
    request_should_close(request) || response_close || close_delimited
}

fn response_close_delimited_from_headers(response: &crossfeed_net::Response) -> bool {
    if status_has_no_body(response.line.status_code) {
        return false;
    }
    if has_content_length(&response.headers) || has_chunked_transfer_encoding(&response.headers) {
        return false;
    }
    true
}

fn log_http1_upstream_send(request_id: Uuid, host: &str, port: u16, reused: bool) {
    let _ = request_id;
    let _ = host;
    let _ = port;
    let _ = reused;
}

async fn close_http1_connection<C, U>(client: &mut C, upstream: Option<&mut U>)
where
    C: AsyncWrite + Unpin,
    U: AsyncWrite + Unpin,
{
    let _ = client.shutdown().await;
    if let Some(upstream) = upstream {
        let _ = upstream.shutdown().await;
    }
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

    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    if !state.config.tls.enabled {
        let mut upstream = connect_upstream(&state.config, host.clone(), port).await?;
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

    let cache_key = format!("{host}:{port}");
    let protocol_mode = state.config.protocol_mode.clone();
    let cached_protocol = {
        let cache = state.alpn_cache.lock().await;
        cache.get(&cache_key).copied()
    };
    let upstream_alpn_list = build_upstream_alpn_list(protocol_mode.clone(), cached_protocol);
    let (mut tls_upstream, mut upstream_protocol) = connect_tls_upstream(
        &state.config,
        host.clone(),
        port,
        &upstream_alpn_list,
    )
    .await?;

    match protocol_mode {
        ProxyProtocolMode::Http2 => {
            if upstream_protocol != NegotiatedProtocol::Http2 {
                return Err(ProxyError::Runtime(
                    "upstream did not negotiate h2".to_string(),
                ));
            }
        }
        ProxyProtocolMode::Http1 => {
            upstream_protocol = NegotiatedProtocol::Http1;
        }
        ProxyProtocolMode::Auto => {}
    }

    let client_preferred = match protocol_mode {
        ProxyProtocolMode::Http1 => NegotiatedProtocol::Http1,
        ProxyProtocolMode::Http2 => NegotiatedProtocol::Http2,
        ProxyProtocolMode::Auto => upstream_protocol,
    };
    let client_alpn_list = match protocol_mode {
        ProxyProtocolMode::Http2 => alpn_list(client_preferred, false),
        ProxyProtocolMode::Http1 => alpn_list(client_preferred, false),
        ProxyProtocolMode::Auto => {
            if upstream_protocol == NegotiatedProtocol::Http2 {
                alpn_list(client_preferred, true)
            } else {
                alpn_list(client_preferred, false)
            }
        }
    };

    let acceptor = build_acceptor(
        &TlsConfig {
            allow_legacy: state.config.tls.allow_legacy,
            alpn_protocols: client_alpn_list,
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

    let client_protocol = match tls_client.ssl().selected_alpn_protocol() {
        Some(b"h2") => NegotiatedProtocol::Http2,
        _ => NegotiatedProtocol::Http1,
    };
    if protocol_mode == ProxyProtocolMode::Http2
        && client_protocol != NegotiatedProtocol::Http2
    {
        return Err(ProxyError::Runtime(
            "client did not negotiate h2".to_string(),
        ));
    }

    let mut _fallback_to_http1 = false;
    if protocol_mode == ProxyProtocolMode::Auto
        && upstream_protocol == NegotiatedProtocol::Http2
        && client_protocol == NegotiatedProtocol::Http1
    {
        let (fallback_upstream, _) = connect_tls_upstream(
            &state.config,
            host.clone(),
            port,
            &alpn_list(NegotiatedProtocol::Http1, false),
        )
        .await?;
        tls_upstream = fallback_upstream;
        upstream_protocol = NegotiatedProtocol::Http1;
        _fallback_to_http1 = true;
    }

    {
        let mut cache = state.alpn_cache.lock().await;
        cache.insert(cache_key.clone(), upstream_protocol);
    }



    let mut buffer = vec![0u8; 8192];
    let n = tls_client.read(&mut buffer).await?;
    if n == 0 {
        return Ok(());
    }
    buffer.truncate(n);

    if client_protocol == NegotiatedProtocol::Http2 {
        if !buffer.starts_with(HTTP2_PREFACE) {
            return Err(ProxyError::Runtime("missing http2 preface".to_string()));
        }
        handle_http2_stream(state, tls_client, tls_upstream, buffer, host, port).await?;
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

fn build_upstream_alpn_list(
    mode: ProxyProtocolMode,
    cached: Option<NegotiatedProtocol>,
) -> Vec<String> {
    match mode {
        ProxyProtocolMode::Http1 => alpn_list(NegotiatedProtocol::Http1, false),
        ProxyProtocolMode::Http2 => alpn_list(NegotiatedProtocol::Http2, false),
        ProxyProtocolMode::Auto => match cached {
            Some(NegotiatedProtocol::Http1) => alpn_list(NegotiatedProtocol::Http1, false),
            Some(NegotiatedProtocol::Http2) => alpn_list(NegotiatedProtocol::Http2, true),
            None => alpn_list(NegotiatedProtocol::Http2, true),
        },
    }
}

async fn connect_tls_upstream(
    config: &ProxyConfig,
    host: String,
    port: u16,
    alpn_protocols: &[String],
) -> Result<(tokio_openssl::SslStream<TcpStream>, NegotiatedProtocol), ProxyError> {
    let upstream = connect_upstream(config, host.clone(), port).await?;
    let mut connector = openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls())
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    if !alpn_protocols.is_empty() {
        let encoded = encode_alpn_protocols(alpn_protocols)?;
        connector
            .set_alpn_protos(&encoded)
            .map_err(|err| ProxyError::Runtime(err.to_string()))?;
    }
    let connector = connector.build();
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
    let protocol = protocol_from_alpn(tls_upstream.ssl().selected_alpn_protocol());
    Ok((tls_upstream, protocol))
}

fn encode_alpn_protocols(protocols: &[String]) -> Result<Vec<u8>, ProxyError> {
    let mut encoded = Vec::new();
    for protocol in protocols {
        let bytes = protocol.as_bytes();
        if bytes.len() > u8::MAX as usize {
            return Err(ProxyError::Runtime("alpn protocol too long".to_string()));
        }
        encoded.push(bytes.len() as u8);
        encoded.extend_from_slice(bytes);
    }
    Ok(encoded)
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

struct StreamedHttp1Response {
    bytes: Vec<u8>,
    should_close: bool,
}


#[derive(Debug)]
enum StreamEndReason {
    ContentLength,
    ChunkedComplete,
    UpstreamEofComplete,
    UpstreamEofIncomplete,
    CloseDelimited,
    ParseError,
}


async fn read_response_streaming<S, C>(
    upstream: &mut S,
    client: &mut C,
    limits: crossfeed_net::Limits,
    request: &crossfeed_net::Request,
    initial: Option<Vec<u8>>,
) -> Result<StreamedHttp1Response, ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 8192];
    let mut response = Vec::new();
    let capture_limit = limits.max_header_bytes.saturating_add(limits.max_body_bytes);
    let mut parser = ResponseStreamParser::with_limits(limits);
    let mut frame_info: Option<crossfeed_net::ResponseFrameInfo> = None;
    let mut should_close = false;
    let mut end_reason = StreamEndReason::ParseError;
    let mut upstream_incomplete = false;

    if let Some(initial_bytes) = initial {
        if !initial_bytes.is_empty() {
            client
                .write_all(&initial_bytes)
                .await
                .map_err(|err| ProxyError::Runtime(err.to_string()))?;
            if response.len() < capture_limit {
                let remaining = capture_limit - response.len();
                let to_copy = remaining.min(initial_bytes.len());
                response.extend_from_slice(&initial_bytes[..to_copy]);
            }
            let events = parser
                .push(&initial_bytes)
                .map_err(|error| ProxyError::Runtime(format!("response parse error {error:?}")))?;
            for event in events {
                match event {
                    ResponseStreamEvent::Headers(info) => {
                        should_close = should_close_from_frame(request, &info);
                        frame_info = Some(info);
                    }
                ResponseStreamEvent::EndOfMessage => {
                    return Ok(StreamedHttp1Response {
                        bytes: response,
                        should_close,
                        });
                    }
                    ResponseStreamEvent::BodyBytes { .. } => {}
                }
            }
        }
    }

    loop {
        let n = upstream.read(&mut buffer).await?;
        if n == 0 {
            match parser.push_eof() {
                Ok(events) => {
                    if events
                        .iter()
                        .any(|event| matches!(event, ResponseStreamEvent::EndOfMessage))
                    {
                        end_reason = StreamEndReason::UpstreamEofComplete;
                    } else {
                        upstream_incomplete = true;
                        end_reason = StreamEndReason::UpstreamEofIncomplete;
                    }
                }
                Err(_) => {
                    upstream_incomplete = true;
                    end_reason = StreamEndReason::UpstreamEofIncomplete;
                }
            }
            break;
        }

        if let Err(err) = client.write_all(&buffer[..n]).await {
            return Err(ProxyError::Runtime(err.to_string()));
        }

        if response.len() < capture_limit {
            let remaining = capture_limit - response.len();
            let to_copy = remaining.min(n);
            response.extend_from_slice(&buffer[..to_copy]);
        }

        let events = parser
            .push(&buffer[..n])
            .map_err(|error| ProxyError::Runtime(format!("response parse error {error:?}")))?;
        for event in events {
            match event {
                ResponseStreamEvent::Headers(info) => {
                    should_close = should_close_from_frame(request, &info);
                    frame_info = Some(info);
                }
                ResponseStreamEvent::EndOfMessage => {
                    if let Some(info) = frame_info.as_ref() {
                        end_reason = if info.chunked {
                            StreamEndReason::ChunkedComplete
                        } else if info.content_length.is_some() {
                            StreamEndReason::ContentLength
                        } else {
                            StreamEndReason::CloseDelimited
                        };
                    }
                    break;
                }
                ResponseStreamEvent::BodyBytes { .. } => {}
            }
        }

        if matches!(end_reason, StreamEndReason::ChunkedComplete | StreamEndReason::ContentLength | StreamEndReason::CloseDelimited) {
            break;
        }
    }

    client
        .flush()
        .await
        .map_err(|err| ProxyError::Runtime(err.to_string()))?;

    if upstream_incomplete {
        should_close = true;
    }

    let _ = end_reason;
    Ok(StreamedHttp1Response {
        bytes: response,
        should_close,
    })
}

async fn read_response_stream<S>(
    stream: &mut S,
    limits: crossfeed_net::Limits,
) -> Result<Vec<u8>, ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut parser = ResponseParser::with_limits(limits);
    let mut buffer = vec![0u8; 8192];
    let mut response = Vec::new();
    let mut read_until_eof = false;

    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        response.extend_from_slice(&buffer[..n]);
        if read_until_eof {
            continue;
        }
        match parser.push(&buffer[..n]) {
            crossfeed_net::ParseStatus::NeedMore { .. } => continue,
            crossfeed_net::ParseStatus::Complete { message, .. } => {
                let has_length = response_has_length(&message.headers);
                if has_length || status_has_no_body(message.line.status_code) {
                    break;
                }
                read_until_eof = true;
            }
            crossfeed_net::ParseStatus::Error { error, .. } => {
                if matches!(error.kind, crossfeed_net::ParseErrorKind::UnexpectedEof) {
                    continue;
                }
                if matches!(error.kind, crossfeed_net::ParseErrorKind::HeaderTooLarge) {
                    read_until_eof = true;
                    continue;
                }
                return Err(ProxyError::Runtime(format!(
                    "response parse error {error:?}"
                )));
            }
        }
    }

    if response.is_empty() {
        return Ok(response);
    }

    Ok(response)
}

async fn read_response_buffered_with_initial<S>(
    stream: &mut S,
    limits: crossfeed_net::Limits,
    initial: Vec<u8>,
) -> Result<Vec<u8>, ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut parser = ResponseStreamParser::with_limits(limits);
    let mut response = initial;
    let mut buffer = vec![0u8; 8192];

    let events = parser
        .push(&response)
        .map_err(|error| ProxyError::Runtime(format!("response parse error {error:?}")))?;
    if events
        .iter()
        .any(|event| matches!(event, ResponseStreamEvent::EndOfMessage))
    {
        return Ok(response);
    }

    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            let events = parser
                .push_eof()
                .map_err(|error| ProxyError::Runtime(format!("response parse error {error:?}")))?;
            if events
                .iter()
                .any(|event| matches!(event, ResponseStreamEvent::EndOfMessage))
            {
                return Ok(response);
            }
            return Err(ProxyError::Runtime("response incomplete".to_string()));
        }
        response.extend_from_slice(&buffer[..n]);
        let events = parser
            .push(&buffer[..n])
            .map_err(|error| ProxyError::Runtime(format!("response parse error {error:?}")))?;
        if events
            .iter()
            .any(|event| matches!(event, ResponseStreamEvent::EndOfMessage))
        {
            return Ok(response);
        }
    }
}

async fn peek_upstream_response<S>(
    stream: &mut S,
    limits: crossfeed_net::Limits,
) -> Result<Option<Vec<u8>>, ProxyError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 8192];
    let read_result = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        stream.read(&mut buffer),
    )
    .await;

    let Ok(Ok(n)) = read_result else {
        return Ok(None);
    };
    if n == 0 {
        return Ok(Some(Vec::new()));
    }
    buffer.truncate(n);

    let mut parser = ResponseStreamParser::with_limits(limits);
    let events = parser
        .push(&buffer)
        .map_err(|error| ProxyError::Runtime(format!("response parse error {error:?}")))?;
    for event in events {
        if let ResponseStreamEvent::Headers(info) = event {
            if info.status_code != 100 {
                return Ok(Some(buffer));
            }
            return Ok(None);
        }
    }

    Ok(None)
}

fn response_has_length(headers: &[crossfeed_net::Header]) -> bool {
    has_content_length(headers) || has_chunked_transfer_encoding(headers)
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(b"\r\n\r\n".len())
        .position(|window| window == b"\r\n\r\n")
}

fn request_should_close(request: &crossfeed_net::Request) -> bool {
    match request.line.version {
        crossfeed_net::HttpVersion::Http10 => {
            !header_has_token(&request.headers, "connection", "keep-alive")
        }
        _ => header_has_token(&request.headers, "connection", "close"),
    }
}

fn response_should_close(
    version: &crossfeed_net::HttpVersion,
    headers: &[crossfeed_net::Header],
) -> bool {
    match version {
        crossfeed_net::HttpVersion::Http10 => {
            !header_has_token(headers, "connection", "keep-alive")
        }
        crossfeed_net::HttpVersion::Http11 => header_has_token(headers, "connection", "close"),
        crossfeed_net::HttpVersion::Other(_) => {
            header_has_token(headers, "connection", "close")
        }
    }
}

fn header_has_token(headers: &[crossfeed_net::Header], name: &str, token: &str) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(name)
            && header
                .value
                .split(',')
                .any(|value| value.trim().eq_ignore_ascii_case(token))
    })
}


fn has_content_length(headers: &[crossfeed_net::Header]) -> bool {
    headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("content-length"))
}

fn has_chunked_transfer_encoding(headers: &[crossfeed_net::Header]) -> bool {
    headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case("transfer-encoding")
            && header
                .value
                .split(',')
                .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
    })
}

fn status_has_no_body(status_code: u16) -> bool {
    status_code / 100 == 1 || status_code == 204 || status_code == 304
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

fn parse_response(
    response_bytes: &[u8],
    received_at: &str,
    limits: crossfeed_net::Limits,
) -> Option<TimelineResponse> {
    let mut parser = ResponseParser::with_limits(limits);
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
