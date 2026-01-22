use chrono::Utc;
use std::path::PathBuf;

use crossfeed_replay::{
    ReplayEdit, ReplaySendScope, ReplayService, send_replay_request as replay_send_request,
};
use crossfeed_storage::{
    ReplayCollection, ReplayExecution, ReplayRequest, ReplayVersion, SqliteStore, TimelineResponse,
    TimelineRequest,
};
use crossfeed_web::CancelToken;

use crate::scope::evaluate_scope;

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

pub async fn update_replay_collection_name(
    store_path: PathBuf,
    collection_id: i64,
    name: String,
) -> Result<(), String> {
    let store = SqliteStore::open(store_path)?;
    store.update_replay_collection_name(collection_id, &name)
}

pub async fn update_replay_collection_color(
    store_path: PathBuf,
    collection_id: i64,
    color: Option<String>,
) -> Result<(), String> {
    let store = SqliteStore::open(store_path)?;
    store.update_replay_collection_color(collection_id, color.as_deref())
}

pub async fn create_replay_collection(
    store_path: PathBuf,
    name: String,
) -> Result<i64, String> {
    let store = SqliteStore::open(store_path)?;
    let now = Utc::now().to_rfc3339();
    let sort_index = store.next_replay_collection_sort_index()?;
    store.create_replay_collection(&name, sort_index, None, &now)
}

pub async fn create_collection_and_add_request(
    store_path: PathBuf,
    name: String,
    request_id: i64,
) -> Result<i64, String> {
    let store = SqliteStore::open(store_path)?;
    let now = Utc::now().to_rfc3339();
    let sort_index = store.next_replay_collection_sort_index()?;
    let collection_id = store.create_replay_collection(&name, sort_index, None, &now)?;
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
    let name = build_replay_name(&summary.method, &summary.path);
    let sort_index = store.next_replay_request_sort_index(None)?;
    let timeline_request: TimelineRequest = summary.into();
    let service = ReplayService::new(store);
    let (request, _version) = service
        .import_from_timeline(&timeline_request, name, Some(timeline_request_id))
        .map_err(|err| err.to_string())?;
    let now = Utc::now().to_rfc3339();
    service
        .store()
        .update_replay_request_sort(request.id, None, sort_index, &now)?;
    Ok(request.id)
}

pub async fn apply_replay_raw_edit(
    store_path: PathBuf,
    request_id: i64,
    raw_request: String,
) -> Result<ReplayVersion, String> {
    let store = SqliteStore::open(store_path)?;
    let service = ReplayService::new(store);
    service
        .apply_raw_edit(request_id, &raw_request)
        .map_err(|err| err.to_string())
}

pub async fn apply_replay_edit(
    store_path: PathBuf,
    request_id: i64,
    edit: ReplayEdit,
) -> Result<ReplayVersion, String> {
    let store = SqliteStore::open(store_path)?;
    let service = ReplayService::new(store);
    let request = service
        .store()
        .get_replay_request(request_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "Replay request not found".to_string())?;
    service
        .apply_edit(&request, edit)
        .map_err(|err| err.to_string())
}

pub async fn set_replay_active_version(
    store_path: PathBuf,
    request_id: i64,
    version_id: i64,
) -> Result<ReplayVersion, String> {
    let store = SqliteStore::open(store_path)?;
    let service = ReplayService::new(store);
    service
        .set_active_version(request_id, version_id)
        .map_err(|err| err.to_string())
}

pub async fn activate_latest_replay_child(
    store_path: PathBuf,
    request_id: i64,
    parent_id: i64,
) -> Result<Option<ReplayVersion>, String> {
    let store = SqliteStore::open(store_path)?;
    let service = ReplayService::new(store);
    let children = service
        .list_child_versions(parent_id)
        .map_err(|err| err.to_string())?;
    if let Some(version) = children.into_iter().next() {
        let version = service
            .set_active_version(request_id, version.id)
            .map_err(|err| err.to_string())?;
        Ok(Some(version))
    } else {
        Ok(None)
    }
}

pub async fn send_replay_request(
    store_path: PathBuf,
    request_id: i64,
    cancel: CancelToken,
) -> Result<Option<i64>, String> {
    let store = SqliteStore::open(store_path.clone())?;
    let version = store
        .get_replay_active_version(request_id)?
        .ok_or_else(|| "Missing active replay version".to_string())?;
    let scope = evaluate_scope(&store_path, &version.host, &version.path)?;
    let send_scope = ReplaySendScope {
        scope_status_at_capture: scope.scope_status_at_capture,
        scope_rules_version: scope.scope_rules_version,
        capture_filtered: scope.capture_filtered,
        timeline_filtered: scope.timeline_filtered,
    };
    match replay_send_request(&store_path, request_id, send_scope, cancel).await {
        Ok(result) => Ok(Some(result.timeline_request_id)),
        Err(crossfeed_replay::ReplayError::Cancelled) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
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
