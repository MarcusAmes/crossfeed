use tempfile::NamedTempFile;

use crate::{
    ReplayExecution, ReplayRequest, ReplayVersion, SqliteStore, TimelineRequest, TimelineStore,
};

fn sample_timeline_request() -> TimelineRequest {
    TimelineRequest {
        source: "proxy".to_string(),
        method: "GET".to_string(),
        scheme: "http".to_string(),
        host: "example.com".to_string(),
        port: 80,
        path: "/".to_string(),
        query: None,
        url: "http://example.com/".to_string(),
        http_version: "HTTP/1.1".to_string(),
        request_headers: b"Host: example.com\r\n".to_vec(),
        request_body: Vec::new(),
        request_body_size: 0,
        request_body_truncated: false,
        started_at: "now".to_string(),
        completed_at: None,
        duration_ms: None,
        scope_status_at_capture: "in_scope".to_string(),
        scope_status_current: None,
        scope_rules_version: 1,
        capture_filtered: false,
        timeline_filtered: false,
    }
}

fn sample_replay_request(source_timeline_request_id: i64) -> ReplayRequest {
    ReplayRequest {
        id: 0,
        collection_id: None,
        source_timeline_request_id: Some(source_timeline_request_id),
        name: "GET /".to_string(),
        sort_index: 0,
        method: "GET".to_string(),
        scheme: "http".to_string(),
        host: "example.com".to_string(),
        port: 80,
        path: "/".to_string(),
        query: None,
        url: "http://example.com/".to_string(),
        http_version: "HTTP/1.1".to_string(),
        request_headers: b"Host: example.com\r\n".to_vec(),
        request_body: Vec::new(),
        request_body_size: 0,
        active_version_id: None,
        created_at: "now".to_string(),
        updated_at: "now".to_string(),
    }
}

fn sample_replay_version(
    replay_request_id: i64,
    parent_id: Option<i64>,
    label: &str,
) -> ReplayVersion {
    ReplayVersion {
        id: 0,
        replay_request_id,
        parent_id,
        label: label.to_string(),
        created_at: "now".to_string(),
        method: "GET".to_string(),
        scheme: "http".to_string(),
        host: "example.com".to_string(),
        port: 80,
        path: "/".to_string(),
        query: None,
        url: "http://example.com/".to_string(),
        http_version: "HTTP/1.1".to_string(),
        request_headers: b"Host: example.com\r\n".to_vec(),
        request_body: Vec::new(),
        request_body_size: 0,
    }
}

#[test]
fn replay_storage_inserts_versions_and_updates_active() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    let timeline_request_id = store
        .insert_request(sample_timeline_request())
        .unwrap()
        .request_id;
    let request_id = store
        .create_replay_request(&sample_replay_request(timeline_request_id))
        .unwrap();
    let first_version_id = store
        .insert_replay_version(&sample_replay_version(request_id, None, "Initial import"))
        .unwrap();
    store
        .update_replay_active_version(request_id, first_version_id, "now")
        .unwrap();

    let mut updated_version = sample_replay_version(request_id, Some(first_version_id), "Edit 1");
    updated_version.path = "/edit".to_string();
    let second_version_id = store.insert_replay_version(&updated_version).unwrap();
    store
        .update_replay_snapshot(request_id, &updated_version, "later")
        .unwrap();
    store
        .update_replay_active_version(request_id, second_version_id, "later")
        .unwrap();
}

#[test]
fn replay_storage_inserts_execution() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    let timeline_request_id = store
        .insert_request(sample_timeline_request())
        .unwrap()
        .request_id;
    let request_id = store
        .create_replay_request(&sample_replay_request(timeline_request_id))
        .unwrap();
    let execution = ReplayExecution {
        id: 0,
        replay_request_id: request_id,
        timeline_request_id,
        executed_at: "now".to_string(),
    };
    let execution_id = store.insert_replay_execution(&execution).unwrap();
    assert!(execution_id > 0);
}
