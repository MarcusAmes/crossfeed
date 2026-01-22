use chrono::Utc;
use std::path::PathBuf;
use crossfeed_storage::{
    ReplayCollection, ReplayExecution, ReplayRequest, ReplayVersion, SqliteStore, TimelineResponse,
};

pub async fn list_replay_collections(store_path: PathBuf) -> Result<Vec<ReplayCollection>, String> {
    let store = SqliteStore::open(store_path)?;
    store.list_replay_collections()
}

pub async fn list_replay_requests_unassigned(
    store_path: PathBuf,
) -> Result<Vec<ReplayRequest>, String> {
    let store = SqliteStore::open(store_path)?;
    store.list_replay_requests_unassigned()
}

pub async fn list_replay_requests_in_collection(
    store_path: PathBuf,
    collection_id: i64,
) -> Result<Vec<ReplayRequest>, String> {
    let store = SqliteStore::open(store_path)?;
    store.list_replay_requests_in_collection(collection_id)
}

pub async fn update_replay_request_sort(
    store_path: PathBuf,
    request_id: i64,
    collection_id: Option<i64>,
    sort_index: i64,
) -> Result<(), String> {
    let store = SqliteStore::open(store_path)?;
    let now = Utc::now().to_rfc3339();
    store.update_replay_request_sort(request_id, collection_id, sort_index, &now)
}

pub async fn move_replay_request_to_collection(
    store_path: PathBuf,
    request_id: i64,
    collection_id: Option<i64>,
) -> Result<(), String> {
    let store = SqliteStore::open(store_path)?;
    let now = Utc::now().to_rfc3339();
    let sort_index = store.next_replay_request_sort_index(collection_id)?;
    store.update_replay_request_sort(request_id, collection_id, sort_index, &now)
}

pub async fn update_replay_collection_sort(
    store_path: PathBuf,
    collection_id: i64,
    sort_index: i64,
) -> Result<(), String> {
    let store = SqliteStore::open(store_path)?;
    store.update_replay_collection_sort(collection_id, sort_index)
}

pub async fn update_replay_request_name(
    store_path: PathBuf,
    request_id: i64,
    name: String,
) -> Result<(), String> {
    let store = SqliteStore::open(store_path)?;
    let now = Utc::now().to_rfc3339();
    store.update_replay_request_name(request_id, &name, &now)
}

pub async fn create_replay_collection(
    store_path: PathBuf,
    name: String,
) -> Result<i64, String> {
    let store = SqliteStore::open(store_path)?;
    let now = Utc::now().to_rfc3339();
    let sort_index = store.next_replay_collection_sort_index()?;
    store.create_replay_collection(&name, sort_index, &now)
}

pub async fn create_collection_and_add_request(
    store_path: PathBuf,
    name: String,
    request_id: i64,
) -> Result<i64, String> {
    let store = SqliteStore::open(store_path)?;
    let now = Utc::now().to_rfc3339();
    let sort_index = store.next_replay_collection_sort_index()?;
    let collection_id = store.create_replay_collection(&name, sort_index, &now)?;
    let request_sort = store.next_replay_request_sort_index(Some(collection_id))?;
    store.update_replay_request_sort(request_id, Some(collection_id), request_sort, &now)?;
    Ok(collection_id)
}

pub async fn get_replay_request(
    store_path: PathBuf,
    request_id: i64,
) -> Result<Option<ReplayRequest>, String> {
    let store = SqliteStore::open(store_path)?;
    store.get_replay_request(request_id)
}

pub async fn get_replay_active_version(
    store_path: PathBuf,
    request_id: i64,
) -> Result<Option<ReplayVersion>, String> {
    let store = SqliteStore::open(store_path)?;
    store.get_replay_active_version(request_id)
}

pub async fn get_latest_replay_execution(
    store_path: PathBuf,
    request_id: i64,
) -> Result<Option<ReplayExecution>, String> {
    let store = SqliteStore::open(store_path)?;
    store.get_latest_replay_execution(request_id)
}

pub async fn get_latest_replay_response(
    store_path: PathBuf,
    request_id: i64,
) -> Result<Option<TimelineResponse>, String> {
    let store = SqliteStore::open(store_path)?;
    let execution = store.get_latest_replay_execution(request_id)?;
    let Some(execution) = execution else {
        return Ok(None);
    };
    store.get_response_by_request_id(execution.timeline_request_id)
}

pub async fn create_replay_from_timeline(
    store_path: PathBuf,
    timeline_request_id: i64,
) -> Result<i64, String> {
    let store = SqliteStore::open(store_path)?;
    let summary = store
        .get_request_summary(timeline_request_id)?
        .ok_or_else(|| "Timeline request not found".to_string())?;
    let now = Utc::now().to_rfc3339();
    let name = build_replay_name(&summary.method, &summary.path);
    let sort_index = store.next_replay_request_sort_index(None)?;
    let request = ReplayRequest {
        id: 0,
        collection_id: None,
        source_timeline_request_id: Some(timeline_request_id),
        name,
        sort_index,
        method: summary.method.clone(),
        scheme: summary.scheme.clone(),
        host: summary.host.clone(),
        port: summary.port,
        path: summary.path.clone(),
        query: summary.query.clone(),
        url: summary.url.clone(),
        http_version: summary.http_version.clone(),
        request_headers: summary.request_headers.clone(),
        request_body: summary.request_body.clone(),
        request_body_size: summary.request_body_size,
        active_version_id: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let request_id = store.create_replay_request(&request)?;
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
    let version_id = store.insert_replay_version(&version)?;
    store.update_replay_active_version(request_id, version_id, &request.updated_at)?;
    Ok(request_id)
}

pub async fn duplicate_replay_request(
    store_path: PathBuf,
    request_id: i64,
) -> Result<i64, String> {
    let store = SqliteStore::open(store_path)?;
    let request = store
        .get_replay_request(request_id)?
        .ok_or_else(|| "Replay request not found".to_string())?;
    let version = store
        .get_replay_active_version(request_id)?
        .ok_or_else(|| "Replay version not found".to_string())?;
    let now = Utc::now().to_rfc3339();
    let name = format!("{} copy", request.name);
    let new_request = ReplayRequest {
        id: 0,
        collection_id: request.collection_id,
        source_timeline_request_id: request.source_timeline_request_id,
        name,
        sort_index: 0,
        method: version.method.clone(),
        scheme: version.scheme.clone(),
        host: version.host.clone(),
        port: version.port,
        path: version.path.clone(),
        query: version.query.clone(),
        url: version.url.clone(),
        http_version: version.http_version.clone(),
        request_headers: version.request_headers.clone(),
        request_body: version.request_body.clone(),
        request_body_size: version.request_body_size,
        active_version_id: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let new_request_id = store.create_replay_request(&new_request)?;
    let new_version = ReplayVersion {
        id: 0,
        replay_request_id: new_request_id,
        parent_id: Some(version.id),
        label: "Duplicate".to_string(),
        created_at: now,
        method: version.method,
        scheme: version.scheme,
        host: version.host,
        port: version.port,
        path: version.path,
        query: version.query,
        url: version.url,
        http_version: version.http_version,
        request_headers: version.request_headers,
        request_body: version.request_body,
        request_body_size: version.request_body_size,
    };
    let version_id = store.insert_replay_version(&new_version)?;
    store.update_replay_active_version(new_request_id, version_id, &new_request.updated_at)?;

    let mut ordered = if let Some(collection_id) = request.collection_id {
        store.list_replay_requests_in_collection(collection_id)?
    } else {
        store.list_replay_requests_unassigned()?
    };
    ordered.retain(|req| req.id != new_request_id);
    let insert_at = ordered
        .iter()
        .position(|req| req.id == request_id)
        .map(|idx| idx + 1)
        .unwrap_or(ordered.len());
    ordered.insert(insert_at, ReplayRequest { id: new_request_id, ..new_request });
    let now = Utc::now().to_rfc3339();
    for (index, item) in ordered.iter().enumerate() {
        let sort_index = (ordered.len() - index) as i64;
        store.update_replay_request_sort(item.id, request.collection_id, sort_index, &now)?;
    }
    Ok(new_request_id)
}

fn build_replay_name(method: &str, path: &str) -> String {
    let truncated = truncate_path(path, 48);
    format!("{method} {truncated}")
}

fn truncate_path(path: &str, max_len: usize) -> String {
    let count = path.chars().count();
    if count <= max_len {
        return path.to_string();
    }
    let keep = max_len.saturating_sub(3);
    let prefix: String = path.chars().take(keep).collect();
    format!("{prefix}...")
}
