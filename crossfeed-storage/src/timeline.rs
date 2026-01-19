use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineRequest {
    pub source: String,
    pub method: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub query: Option<String>,
    pub url: String,
    pub http_version: String,
    pub request_headers: Vec<u8>,
    pub request_body: Vec<u8>,
    pub request_body_size: usize,
    pub request_body_truncated: bool,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub scope_status_at_capture: String,
    pub scope_status_current: Option<String>,
    pub scope_rules_version: i64,
    pub capture_filtered: bool,
    pub timeline_filtered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineResponse {
    pub timeline_request_id: i64,
    pub status_code: u16,
    pub reason: Option<String>,
    pub response_headers: Vec<u8>,
    pub response_body: Vec<u8>,
    pub response_body_size: usize,
    pub response_body_truncated: bool,
    pub http_version: String,
    pub received_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct BodyLimits {
    pub request_max_bytes: usize,
    pub response_max_bytes: usize,
}

impl Default for BodyLimits {
    fn default() -> Self {
        Self {
            request_max_bytes: 5 * 1024 * 1024,
            response_max_bytes: 20 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineInsertResult {
    pub request_id: i64,
}

pub trait TimelineStore: Send {
    fn insert_request(&self, request: TimelineRequest) -> Result<TimelineInsertResult, String>;
    fn insert_response(&self, response: TimelineResponse) -> Result<(), String>;
}

pub struct TimelineRecorder {
    store: Box<dyn TimelineStore + Send>,
    limits: BodyLimits,
}

impl TimelineRecorder {
    pub fn new(store: Box<dyn TimelineStore + Send>, limits: BodyLimits) -> Self {
        Self { store, limits }
    }

    pub fn record_request(
        &self,
        mut request: TimelineRequest,
    ) -> Result<TimelineInsertResult, String> {
        let (body, truncated) = truncate_body(request.request_body, self.limits.request_max_bytes);
        request.request_body = body;
        request.request_body_truncated = truncated;
        self.store.insert_request(request)
    }

    pub fn record_response(&self, mut response: TimelineResponse) -> Result<(), String> {
        let (body, truncated) =
            truncate_body(response.response_body, self.limits.response_max_bytes);
        response.response_body = body;
        response.response_body_truncated = truncated;
        self.store.insert_response(response)
    }
}

fn truncate_body(body: Vec<u8>, limit: usize) -> (Vec<u8>, bool) {
    if body.len() > limit {
        (body[..limit].to_vec(), true)
    } else {
        (body, false)
    }
}
