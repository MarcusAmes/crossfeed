use crossfeed_replay::{ReplayEdit, ReplayService};
use crossfeed_storage::{ReplayRequest, ReplayVersion, SqliteStore, TimelineRequest};

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

fn sample_active_request() -> ReplayRequest {
    ReplayRequest {
        id: 42,
        collection_id: None,
        source_timeline_request_id: None,
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
        active_version_id: Some(1),
        created_at: "now".to_string(),
        updated_at: "now".to_string(),
    }
}

fn sample_version() -> ReplayVersion {
    ReplayVersion {
        id: 1,
        replay_request_id: 42,
        parent_id: None,
        label: "Initial import".to_string(),
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
fn import_creates_request_and_version() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();
    let service = ReplayService::new(store);

    let request = sample_timeline_request();
    let (replay_request, version) = service
        .import_from_timeline(&request, "GET /".to_string(), None)
        .unwrap();

    assert_eq!(replay_request.name, "GET /");
    assert_eq!(version.label, "Initial import");
    assert!(replay_request.active_version_id.is_some());
}

#[test]
fn apply_edit_creates_new_version() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();
    let service = ReplayService::new(store);

    let (imported_request, _version) = service
        .import_from_timeline(&sample_timeline_request(), "GET /".to_string(), None)
        .unwrap();
    let active_request = ReplayRequest {
        id: imported_request.id,
        active_version_id: imported_request.active_version_id,
        ..sample_active_request()
    };

    let edit = ReplayEdit {
        path: Some("/edit".to_string()),
        label: Some("Edit 1".to_string()),
        ..Default::default()
    };

    let version = service.apply_edit(&active_request, edit).unwrap();

    assert_eq!(version.path, "/edit");
    assert_eq!(version.label, "Edit 1");
}

#[test]
fn diff_versions_includes_raw_output() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();
    let service = ReplayService::new(store);

    let left = sample_version();
    let mut right = sample_version();
    right.path = "/other".to_string();
    let diff = service.diff_versions(&left, &right);

    assert!(diff.raw.contains("-GET /"));
    assert!(diff.raw.contains("+GET /other"));
}
