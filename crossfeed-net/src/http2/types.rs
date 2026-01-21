#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub length: usize,
    pub frame_type: FrameType,
    pub flags: u8,
    pub stream_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameType {
    Data,
    Headers,
    Priority,
    RstStream,
    Settings,
    PushPromise,
    Ping,
    GoAway,
    WindowUpdate,
    Continuation,
    Unknown(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: FramePayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramePayload {
    Data(DataFrame),
    Headers(HeadersFrame),
    Priority(PriorityFrame),
    RstStream(RstStreamFrame),
    Settings(SettingsFrame),
    Ping(PingFrame),
    GoAway(GoAwayFrame),
    WindowUpdate(WindowUpdateFrame),
    Continuation(Vec<u8>),
    Raw(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFrame {
    pub end_stream: bool,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadersFrame {
    pub end_stream: bool,
    pub end_headers: bool,
    pub header_block: Vec<u8>,
    pub headers: Vec<HeaderField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderField {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityFrame {
    pub stream_dependency: u32,
    pub weight: u8,
    pub exclusive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RstStreamFrame {
    pub error_code: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFrame {
    pub settings: Vec<(u16, u32)>,
    pub ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingFrame {
    pub opaque_data: [u8; 8],
    pub ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoAwayFrame {
    pub last_stream_id: u32,
    pub error_code: u32,
    pub debug_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowUpdateFrame {
    pub stream_id: u32,
    pub increment: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Http2Warning {
    pub kind: Http2WarningKind,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Http2WarningKind {
    FrameTooLarge { declared: usize, max: usize },
    UnknownFrameType(u8),
    HeadersContinuationMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Http2Error {
    pub kind: Http2ErrorKind,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Http2ErrorKind {
    InvalidPreface,
    InvalidFrameHeader,
    IncompleteFrame,
    HpackDecode,
    PendingHeadersOverflow,
}
