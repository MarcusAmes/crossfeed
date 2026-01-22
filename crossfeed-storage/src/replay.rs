use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayCollection {
    pub id: i64,
    pub name: String,
    pub sort_index: i64,
    pub color: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayRequest {
    pub id: i64,
    pub collection_id: Option<i64>,
    pub source_timeline_request_id: Option<i64>,
    pub name: String,
    pub sort_index: i64,
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
    pub active_version_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayVersion {
    pub id: i64,
    pub replay_request_id: i64,
    pub parent_id: Option<i64>,
    pub label: String,
    pub created_at: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayExecution {
    pub id: i64,
    pub replay_request_id: i64,
    pub timeline_request_id: i64,
    pub executed_at: String,
}
