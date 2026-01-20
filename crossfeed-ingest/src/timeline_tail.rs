use std::collections::HashMap;
use std::path::PathBuf;

use crossfeed_storage::{
    ResponseSummary, SqliteStore, TimelineQuery, TimelineRequestSummary, TimelineSort,
};

#[derive(Debug, Clone)]
pub struct TailCursor {
    pub started_at: Option<String>,
    pub request_id: Option<i64>,
}

impl Default for TailCursor {
    fn default() -> Self {
        Self {
            started_at: None,
            request_id: None,
        }
    }
}

impl TailCursor {
    pub fn from_items(items: &[TimelineItem]) -> Self {
        if let Some(last) = items.first() {
            Self {
                started_at: Some(last.started_at.clone()),
                request_id: Some(last.id),
            }
        } else {
            Self::default()
        }
    }

    pub fn merge(self, previous: TailCursor) -> TailCursor {
        let started_at = self.started_at.or(previous.started_at);
        let request_id = self.request_id.or(previous.request_id);
        TailCursor {
            started_at,
            request_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimelineItem {
    pub id: i64,
    pub source: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub url: String,
    pub started_at: String,
    pub duration_ms: Option<i64>,
    pub request_body_size: usize,
    pub request_body_truncated: bool,
    pub completed_at: Option<String>,
    pub http_version: String,
    pub scope_status_at_capture: String,
    pub scope_status_current: Option<String>,
}

impl From<TimelineRequestSummary> for TimelineItem {
    fn from(value: TimelineRequestSummary) -> Self {
        Self {
            id: value.id,
            source: value.source,
            method: value.method,
            host: value.host,
            path: value.path,
            url: value.url,
            started_at: value.started_at,
            duration_ms: value.duration_ms,
            request_body_size: value.request_body_size,
            request_body_truncated: value.request_body_truncated,
            completed_at: value.completed_at,
            http_version: value.http_version,
            scope_status_at_capture: value.scope_status_at_capture,
            scope_status_current: value.scope_status_current,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TailUpdate {
    pub new_items: Vec<TimelineItem>,
    pub tags: HashMap<i64, Vec<String>>,
    pub responses: HashMap<i64, ResponseSummary>,
    pub cursor: TailCursor,
}

pub async fn tail_query(
    store_path: PathBuf,
    cursor: TailCursor,
    existing_ids: Vec<i64>,
    limit: usize,
) -> Result<TailUpdate, String> {
    let store = SqliteStore::open(&store_path)?;
    let mut query = TimelineQuery::default();
    query.limit = limit;
    query.after_started_at = cursor.started_at.clone();
    query.after_request_id = cursor.request_id;

    let mut requests = store.query_request_summaries(&query, TimelineSort::StartedAtDesc)?;
    requests.retain(|item| !existing_ids.contains(&item.id));

    let new_items: Vec<TimelineItem> = requests.into_iter().map(TimelineItem::from).collect();
    let ids: Vec<i64> = new_items.iter().map(|item| item.id).collect();
    let tags = store.get_request_tags(&ids)?;
    let responses = store.get_response_summaries(&ids)?;
    let cursor = TailCursor::from_items(&new_items).merge(cursor);

    Ok(TailUpdate {
        new_items,
        tags,
        responses,
        cursor,
    })
}

#[cfg(feature = "sync-runtime")]
pub fn tail_query_sync(
    store_path: PathBuf,
    cursor: TailCursor,
    existing_ids: Vec<i64>,
    limit: usize,
) -> Result<TailUpdate, String> {
    futures::executor::block_on(tail_query(store_path, cursor, existing_ids, limit))
}
