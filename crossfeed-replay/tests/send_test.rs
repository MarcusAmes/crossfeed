use crossfeed_replay::{ReplayEdit, ReplayService};
use crossfeed_storage::{SqliteStore, TimelineRequest, TimelineStore};

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

#[test]
fn replay_send_records_execution() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();
    let service = ReplayService::new(store);

    let (request, _version) = service
        .import_from_timeline(&sample_timeline_request(), "GET /".to_string())
        .unwrap();

    let edit = ReplayEdit {
        path: Some("/edit".to_string()),
        ..Default::default()
    };

    let _version = service.apply_edit(&request, edit).unwrap();

    let timeline_request_id = service
        .store()
        .insert_request(sample_timeline_request())
        .unwrap()
        .request_id;
    let execution = service
        .record_execution(request.id, timeline_request_id)
        .unwrap();

    assert_eq!(execution.replay_request_id, request.id);
    assert_eq!(execution.timeline_request_id, timeline_request_id);
}
