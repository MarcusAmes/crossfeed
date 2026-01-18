use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineQuery {
    pub host: Option<String>,
    pub method: Option<String>,
    pub status: Option<u16>,
    pub scope_status: Option<String>,
    pub source: Option<String>,
    pub search: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: usize,
    pub offset: usize,
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
            since: None,
            until: None,
            limit: 100,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TimelineSort {
    StartedAtDesc,
    StartedAtAsc,
}
