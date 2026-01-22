use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReplayEdit {
    pub method: Option<String>,
    pub scheme: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub url: Option<String>,
    pub http_version: Option<String>,
    pub request_headers: Option<Vec<u8>>,
    pub request_body: Option<Vec<u8>>,
    pub request_body_size: Option<usize>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayDiff {
    pub json: serde_json::Value,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplaySendScope {
    pub scope_status_at_capture: String,
    pub scope_rules_version: i64,
    pub capture_filtered: bool,
    pub timeline_filtered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplaySendResult {
    pub timeline_request_id: i64,
}
