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
        source_timeline_request_id: Option<i64>,
    ) -> Result<(ReplayRequest, ReplayVersion), ReplayError> {
        let now = Utc::now().to_rfc3339();
        let request = ReplayRequest {
            id: 0,
            collection_id: None,
            source_timeline_request_id,
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

    pub fn apply_raw_edit(
        &self,
        request_id: i64,
        raw_request: &str,
    ) -> Result<ReplayVersion, ReplayError> {
        let request = self
            .store
            .get_replay_request(request_id)
            .map_err(ReplayError::Storage)?
            .ok_or_else(|| ReplayError::InvalidRequest("Replay request not found".to_string()))?;
        let edit = parse_raw_request(raw_request, &request)?;
        self.apply_edit(&request, edit)
    }

    pub fn set_active_version(
        &self,
        request_id: i64,
        version_id: i64,
    ) -> Result<ReplayVersion, ReplayError> {
        let version = self
            .store
            .get_replay_version(version_id)
            .map_err(ReplayError::Storage)?
            .ok_or_else(|| ReplayError::InvalidRequest("Replay version not found".to_string()))?;
        let now = Utc::now().to_rfc3339();
        self.store
            .update_replay_snapshot(request_id, &version, &now)
            .map_err(ReplayError::Storage)?;
        self.store
            .update_replay_active_version(request_id, version_id, &now)
            .map_err(ReplayError::Storage)?;
        Ok(version)
    }

    pub fn list_child_versions(
        &self,
        parent_id: i64,
    ) -> Result<Vec<ReplayVersion>, ReplayError> {
        self.store
            .list_replay_version_children(parent_id)
            .map_err(ReplayError::Storage)
    }

    pub fn get_version(&self, version_id: i64) -> Result<Option<ReplayVersion>, ReplayError> {
        self.store
            .get_replay_version(version_id)
            .map_err(ReplayError::Storage)
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

fn parse_raw_request(raw: &str, fallback: &ReplayRequest) -> Result<ReplayEdit, ReplayError> {
    let normalized = raw.replace("\r\n", "\n");
    let trimmed = normalized.trim_end_matches('\n');
    let (head, body) = trimmed
        .split_once("\n\n")
        .unwrap_or((trimmed, ""));
    let mut lines = head.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| ReplayError::InvalidRequest("Missing request line".to_string()))?;
    let (method, target, http_version) = parse_request_line(request_line)?;
    let header_lines: Vec<&str> = lines.collect();

    let (mut scheme, mut host, mut port, path, query) = parse_target(&target, fallback);
    if let Some(host_header) = parse_host_header(&header_lines) {
        host = host_header.host;
        if let Some(header_port) = host_header.port {
            port = header_port;
        }
    }
    if scheme.is_empty() {
        scheme = fallback.scheme.clone();
    }
    if host.is_empty() {
        host = fallback.host.clone();
    }
    if port == 0 {
        port = fallback.port;
    }
    let query_ref = if query.is_empty() { None } else { Some(query.as_str()) };
    let url = build_url(&scheme, &host, port, &path, query_ref);

    let mut header_block = String::new();
    if !header_lines.is_empty() {
        header_block.push_str(&header_lines.join("\r\n"));
        header_block.push_str("\r\n");
    }
    let body_bytes = body.as_bytes().to_vec();

    Ok(ReplayEdit {
        method: Some(method),
        scheme: Some(scheme),
        host: Some(host),
        port: Some(port),
        path: Some(path),
        query: Some(query).filter(|value| !value.is_empty()),
        url: Some(url),
        http_version: Some(http_version),
        request_headers: Some(header_block.into_bytes()),
        request_body: Some(body_bytes.clone()),
        request_body_size: Some(body_bytes.len()),
        label: None,
    })
}

fn parse_request_line(line: &str) -> Result<(String, String, String), ReplayError> {
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| ReplayError::InvalidRequest("Missing method".to_string()))?;
    let target = parts
        .next()
        .ok_or_else(|| ReplayError::InvalidRequest("Missing target".to_string()))?;
    let http_version = parts
        .next()
        .ok_or_else(|| ReplayError::InvalidRequest("Missing HTTP version".to_string()))?;
    Ok((method.to_string(), target.to_string(), http_version.to_string()))
}

fn parse_target(target: &str, fallback: &ReplayRequest) -> (String, String, u16, String, String) {
    if let Some(rest) = target.strip_prefix("http://") {
        return parse_absolute_target("http", rest, 80, fallback);
    }
    if let Some(rest) = target.strip_prefix("https://") {
        return parse_absolute_target("https", rest, 443, fallback);
    }
    let (path, query) = split_path_query(target);
    (
        fallback.scheme.clone(),
        fallback.host.clone(),
        fallback.port,
        path,
        query,
    )
}

fn parse_absolute_target(
    scheme: &str,
    rest: &str,
    default_port: u16,
    fallback: &ReplayRequest,
) -> (String, String, u16, String, String) {
    let (host_part, path_part) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = parse_host_port(host_part, default_port);
    let path_raw = if path_part.is_empty() {
        "/".to_string()
    } else {
        format!("/{path_part}")
    };
    let (path, query) = split_path_query(&path_raw);
    (
        scheme.to_string(),
        if host.is_empty() { fallback.host.clone() } else { host },
        if port == 0 { fallback.port } else { port },
        path,
        query,
    )
}

fn split_path_query(target: &str) -> (String, String) {
    if let Some((path, query)) = target.split_once('?') {
        (path.to_string(), query.to_string())
    } else {
        (target.to_string(), String::new())
    }
}

fn parse_host_header(lines: &[&str]) -> Option<HostHeader> {
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("host") {
            let value = value.trim();
            let (host, port) = parse_host_port(value, 0);
            return Some(HostHeader {
                host,
                port: if port == 0 { None } else { Some(port) },
            });
        }
    }
    None
}

fn parse_host_port(value: &str, default_port: u16) -> (String, u16) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return (String::new(), default_port);
    }
    if let Some((host, port_str)) = trimmed.rsplit_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            return (host.to_string(), port);
        }
    }
    (trimmed.to_string(), default_port)
}

fn build_url(scheme: &str, host: &str, port: u16, path: &str, query: Option<&str>) -> String {
    let mut url = format!("{scheme}://{host}:{port}{path}");
    if let Some(query) = query {
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
    }
    url
}

struct HostHeader {
    host: String,
    port: Option<u16>,
}
