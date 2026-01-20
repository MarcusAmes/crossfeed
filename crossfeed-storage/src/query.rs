use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineQuery {
    pub host: Option<String>,
    pub method: Option<String>,
    pub status: Option<u16>,
    pub scope_status: Option<String>,
    pub source: Option<String>,
    pub search: Option<String>,
    pub path_exact: Option<String>,
    pub path_prefix: Option<String>,
    pub path_contains: Option<String>,
    pub path_case_sensitive: bool,
    pub tags_any: Vec<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub after_started_at: Option<String>,
    pub after_request_id: Option<i64>,
}

impl Default for TimelineQuery {
    fn default() -> Self {
        Self {
            host: None,
            method: None,
            status: None,
            scope_status: None,
            source: None,
            search: None,
            path_exact: None,
            path_prefix: None,
            path_contains: None,
            path_case_sensitive: false,
            tags_any: Vec::new(),
            since: None,
            until: None,
            limit: 100,
            offset: 0,
            after_started_at: None,
            after_request_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TimelineSort {
    StartedAtDesc,
    StartedAtAsc,
}
