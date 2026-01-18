use tempfile::NamedTempFile;

use crate::{SqliteStore, TimelineRequest, TimelineResponse, TimelineStore};

fn sample_request() -> TimelineRequest {
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

fn sample_response(request_id: i64) -> TimelineResponse {
    TimelineResponse {
        timeline_request_id: request_id,
        status_code: 200,
        reason: Some("OK".to_string()),
        response_headers: b"Content-Length: 0\r\n".to_vec(),
        response_body: Vec::new(),
        response_body_size: 0,
        response_body_truncated: false,
        http_version: "HTTP/1.1".to_string(),
        received_at: "now".to_string(),
    }
}

#[test]
fn sqlite_inserts_request_and_response() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    let request_id = store.insert_request(sample_request()).unwrap().request_id;
    store.insert_response(sample_response(request_id)).unwrap();
}
