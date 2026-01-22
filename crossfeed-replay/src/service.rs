use chrono::Utc;
use similar::{ChangeTag, TextDiff};

use crossfeed_storage::{
    ReplayExecution, ReplayRequest, ReplayVersion, SqliteStore, TimelineRequest,
};

use crate::{ReplayDiff, ReplayEdit, ReplayError};

pub struct ReplayService {
    store: SqliteStore,
}

impl ReplayService {
    pub fn store(&self) -> &SqliteStore {
        &self.store
    }
}

impl ReplayService {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub fn import_from_timeline(
        &self,
        timeline: &TimelineRequest,
        name: String,
    ) -> Result<(ReplayRequest, ReplayVersion), ReplayError> {
        let now = Utc::now().to_rfc3339();
        let request = ReplayRequest {
            id: 0,
            collection_id: None,
            source_timeline_request_id: None,
            name,
            sort_index: 0,
            method: timeline.method.clone(),
            scheme: timeline.scheme.clone(),
            host: timeline.host.clone(),
            port: timeline.port,
            path: timeline.path.clone(),
            query: timeline.query.clone(),
            url: timeline.url.clone(),
            http_version: timeline.http_version.clone(),
            request_headers: timeline.request_headers.clone(),
            request_body: timeline.request_body.clone(),
            request_body_size: timeline.request_body_size,
            active_version_id: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        let request_id = self
            .store
            .create_replay_request(&request)
            .map_err(ReplayError::Storage)?;
        let version = ReplayVersion {
            id: 0,
            replay_request_id: request_id,
            parent_id: None,
            label: "Initial import".to_string(),
            created_at: now,
            method: request.method.clone(),
            scheme: request.scheme.clone(),
            host: request.host.clone(),
            port: request.port,
            path: request.path.clone(),
            query: request.query.clone(),
            url: request.url.clone(),
            http_version: request.http_version.clone(),
            request_headers: request.request_headers.clone(),
            request_body: request.request_body.clone(),
            request_body_size: request.request_body_size,
        };
        let version_id = self
            .store
            .insert_replay_version(&version)
            .map_err(ReplayError::Storage)?;
        self.store
            .update_replay_active_version(request_id, version_id, &request.updated_at)
            .map_err(ReplayError::Storage)?;

        let mut request = request;
        request.id = request_id;
        request.active_version_id = Some(version_id);
        let mut version = version;
        version.id = version_id;
        Ok((request, version))
    }

    pub fn apply_edit(
        &self,
        active_request: &ReplayRequest,
        edit: ReplayEdit,
    ) -> Result<ReplayVersion, ReplayError> {
        let now = Utc::now().to_rfc3339();
        let version = ReplayVersion {
            id: 0,
            replay_request_id: active_request.id,
            parent_id: active_request.active_version_id,
            label: edit.label.unwrap_or_else(|| format!("Edit {}", now)),
            created_at: now.clone(),
            method: edit.method.unwrap_or_else(|| active_request.method.clone()),
            scheme: edit.scheme.unwrap_or_else(|| active_request.scheme.clone()),
            host: edit.host.unwrap_or_else(|| active_request.host.clone()),
            port: edit.port.unwrap_or(active_request.port),
            path: edit.path.unwrap_or_else(|| active_request.path.clone()),
            query: edit.query.or_else(|| active_request.query.clone()),
            url: edit.url.unwrap_or_else(|| active_request.url.clone()),
            http_version: edit
                .http_version
                .unwrap_or_else(|| active_request.http_version.clone()),
            request_headers: edit
                .request_headers
                .unwrap_or_else(|| active_request.request_headers.clone()),
            request_body: edit
                .request_body
                .unwrap_or_else(|| active_request.request_body.clone()),
            request_body_size: edit
                .request_body_size
                .unwrap_or(active_request.request_body_size),
        };

        let version_id = self
            .store
            .insert_replay_version(&version)
            .map_err(ReplayError::Storage)?;
        self.store
            .update_replay_snapshot(active_request.id, &version, &now)
            .map_err(ReplayError::Storage)?;
        self.store
            .update_replay_active_version(active_request.id, version_id, &now)
            .map_err(ReplayError::Storage)?;

        let mut version = version;
        version.id = version_id;
        Ok(version)
    }

    pub fn record_execution(
        &self,
        replay_request_id: i64,
        timeline_request_id: i64,
    ) -> Result<ReplayExecution, ReplayError> {
        let execution = ReplayExecution {
            id: 0,
            replay_request_id,
            timeline_request_id,
            executed_at: Utc::now().to_rfc3339(),
        };
        let id = self
            .store
            .insert_replay_execution(&execution)
            .map_err(ReplayError::Storage)?;
        let mut execution = execution;
        execution.id = id;
        Ok(execution)
    }

    pub fn diff_versions(&self, left: &ReplayVersion, right: &ReplayVersion) -> ReplayDiff {
        let json = serde_json::json!({
            "method": diff_value(&left.method, &right.method),
            "scheme": diff_value(&left.scheme, &right.scheme),
            "host": diff_value(&left.host, &right.host),
            "port": diff_value(&left.port, &right.port),
            "path": diff_value(&left.path, &right.path),
            "query": diff_value(&left.query, &right.query),
            "url": diff_value(&left.url, &right.url),
            "http_version": diff_value(&left.http_version, &right.http_version),
            "headers": diff_bytes(&left.request_headers, &right.request_headers),
            "body": diff_bytes(&left.request_body, &right.request_body),
        });
        let raw_left = format_request_bytes(left);
        let raw_right = format_request_bytes(right);
        let raw = build_raw_diff(&raw_left, &raw_right);
        ReplayDiff { json, raw }
    }
}

fn diff_value<T: PartialEq + serde::Serialize>(left: &T, right: &T) -> serde_json::Value {
    if left == right {
        serde_json::json!({ "status": "unchanged", "value": left })
    } else {
        serde_json::json!({ "status": "changed", "from": left, "to": right })
    }
}

fn diff_bytes(left: &[u8], right: &[u8]) -> serde_json::Value {
    if left == right {
        serde_json::json!({ "status": "unchanged", "size": left.len() })
    } else {
        let left_text = String::from_utf8_lossy(left);
        let right_text = String::from_utf8_lossy(right);
        serde_json::json!({
            "status": "changed",
            "from_len": left.len(),
            "to_len": right.len(),
            "from_text": left_text,
            "to_text": right_text,
        })
    }
}

fn format_request_bytes(version: &ReplayVersion) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "{} {} {}",
        version.method, version.path, version.http_version
    ));
    let headers = String::from_utf8_lossy(&version.request_headers);
    lines.push(headers.trim_end().to_string());
    if !version.request_body.is_empty() {
        let body = String::from_utf8_lossy(&version.request_body);
        lines.push(String::new());
        lines.push(body.to_string());
    }
    lines.join("\n")
}

fn build_raw_diff(left: &str, right: &str) -> String {
    let diff = TextDiff::from_lines(left, right);
    let mut output = String::new();
    for change in diff.iter_all_changes() {
        let prefix = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        output.push_str(prefix);
        output.push_str(change.to_string().as_str());
    }
    output
}
