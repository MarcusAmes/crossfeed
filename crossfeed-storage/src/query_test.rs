use tempfile::NamedTempFile;

use crate::{SqliteConfig, SqliteStore, TimelineQuery, TimelineRequest, TimelineResponse, TimelineSort, TimelineStore};

fn sample_request(url: &str) -> TimelineRequest {
    TimelineRequest {
        source: "proxy".to_string(),
        method: "GET".to_string(),
        scheme: "http".to_string(),
        host: "example.com".to_string(),
        port: 80,
        path: "/".to_string(),
        query: Some("a=1".to_string()),
        url: url.to_string(),
        http_version: "HTTP/1.1".to_string(),
        request_headers: b"Host: example.com\r\n".to_vec(),
        request_body: b"hello body".to_vec(),
        request_body_size: 10,
        request_body_truncated: false,
        started_at: "2024-01-01T00:00:00Z".to_string(),
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
        response_body: b"response body".to_vec(),
        response_body_size: 13,
        response_body_truncated: false,
        http_version: "HTTP/1.1".to_string(),
        received_at: "2024-01-01T00:00:01Z".to_string(),
    }
}

#[test]
fn fts_search_finds_request() {
    let file = NamedTempFile::new().unwrap();
    let config = SqliteConfig {
        fts: crate::sqlite::FtsConfig {
            enabled: true,
            index_headers: true,
            index_request_body: true,
            index_response_body: true,
        },
    };
    let store = SqliteStore::open_with_config(file.path(), config).unwrap();

    let id = store.insert_request(sample_request("http://example.com/one")).unwrap().request_id;
    store.insert_response(sample_response(id)).unwrap();

    let query = TimelineQuery {
        search: Some("response".to_string()),
        ..TimelineQuery::default()
    };
    let results = store.query_requests(&query, TimelineSort::StartedAtDesc).unwrap();
    assert_eq!(results.len(), 1);
}
