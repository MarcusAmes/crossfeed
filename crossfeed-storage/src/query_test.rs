use tempfile::NamedTempFile;

use crate::{
    SqliteConfig, SqliteStore, TimelineQuery, TimelineRequest, TimelineResponse, TimelineSort,
    TimelineStore,
};

fn sample_request(url: &str, path: &str, method: &str, source: &str) -> TimelineRequest {
    TimelineRequest {
        source: source.to_string(),
        method: method.to_string(),
        scheme: "http".to_string(),
        host: "example.com".to_string(),
        port: 80,
        path: path.to_string(),
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

fn sample_response(request_id: i64, status_code: u16) -> TimelineResponse {
    TimelineResponse {
        timeline_request_id: request_id,
        status_code,
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

    let id = store
        .insert_request(sample_request(
            "http://example.com/one",
            "/one",
            "GET",
            "proxy",
        ))
        .unwrap()
        .request_id;
    store.insert_response(sample_response(id, 200)).unwrap();

    let query = TimelineQuery {
        search: Some("response".to_string()),
        ..TimelineQuery::default()
    };
    let results = store
        .query_requests(&query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn query_filters_by_path_variants() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    let first_id = store
        .insert_request(sample_request(
            "http://example.com/api/v1/users",
            "/api/v1/users",
            "GET",
            "proxy",
        ))
        .unwrap()
        .request_id;
    store
        .insert_response(sample_response(first_id, 200))
        .unwrap();

    let second_id = store
        .insert_request(sample_request(
            "http://example.com/admin",
            "/admin",
            "GET",
            "proxy",
        ))
        .unwrap()
        .request_id;
    store
        .insert_response(sample_response(second_id, 200))
        .unwrap();

    let exact_query = TimelineQuery {
        path_exact: Some("/admin".to_string()),
        ..TimelineQuery::default()
    };
    let exact_results = store
        .query_requests(&exact_query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(exact_results.len(), 1);
    assert_eq!(exact_results[0].path, "/admin");

    let prefix_query = TimelineQuery {
        path_prefix: Some("/api".to_string()),
        ..TimelineQuery::default()
    };
    let prefix_results = store
        .query_requests(&prefix_query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(prefix_results.len(), 1);
    assert_eq!(prefix_results[0].path, "/api/v1/users");

    let contains_query = TimelineQuery {
        path_contains: Some("v1".to_string()),
        ..TimelineQuery::default()
    };
    let contains_results = store
        .query_requests(&contains_query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(contains_results.len(), 1);
    assert_eq!(contains_results[0].path, "/api/v1/users");
}

#[test]
fn query_filters_by_status_and_excludes_missing() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    let first_id = store
        .insert_request(sample_request(
            "http://example.com/ok",
            "/ok",
            "GET",
            "proxy",
        ))
        .unwrap()
        .request_id;
    store
        .insert_response(sample_response(first_id, 200))
        .unwrap();

    store
        .insert_request(sample_request(
            "http://example.com/pending",
            "/pending",
            "GET",
            "proxy",
        ))
        .unwrap();

    let query = TimelineQuery {
        status: Some(200),
        ..TimelineQuery::default()
    };
    let results = store
        .query_requests(&query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "/ok");
}

#[test]
fn query_filters_by_source_and_tags_any() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    let first_id = store
        .insert_request(sample_request(
            "http://example.com/proxy",
            "/proxy",
            "GET",
            "proxy",
        ))
        .unwrap()
        .request_id;
    store
        .insert_response(sample_response(first_id, 200))
        .unwrap();
    store.add_tags(first_id, &["critical", "auth"]).unwrap();

    let second_id = store
        .insert_request(sample_request(
            "http://example.com/replay",
            "/replay",
            "POST",
            "replay",
        ))
        .unwrap()
        .request_id;
    store
        .insert_response(sample_response(second_id, 201))
        .unwrap();
    store.add_tags(second_id, &["baseline"]).unwrap();

    let query = TimelineQuery {
        source: Some("proxy".to_string()),
        tags_any: vec!["auth".to_string(), "missing".to_string()],
        ..TimelineQuery::default()
    };
    let results = store
        .query_requests(&query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "/proxy");
}

#[test]
fn query_applies_pagination_and_sorting() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    for (idx, path) in ["/first", "/second", "/third"].iter().enumerate() {
        let mut request =
            sample_request(&format!("http://example.com{path}"), path, "GET", "proxy");
        request.started_at = format!("2024-01-01T00:00:0{}Z", idx);
        let id = store.insert_request(request).unwrap().request_id;
        store.insert_response(sample_response(id, 200)).unwrap();
    }

    let query = TimelineQuery {
        limit: 1,
        offset: 1,
        ..TimelineQuery::default()
    };
    let results = store
        .query_requests(&query, TimelineSort::StartedAtAsc)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "/second");
}

#[test]
fn high_volume_inserts_support_filtering() {
    let file = NamedTempFile::new().unwrap();
    let store = SqliteStore::open(file.path()).unwrap();

    for i in 0..5000 {
        let path = if i % 2 == 0 { "/api/items" } else { "/health" };
        let url = format!("http://example.com{path}?id={i}");
        let mut request = sample_request(&url, path, "GET", "proxy");
        request.started_at = format!("2024-01-01T00:00:{:02}Z", i % 60);
        let id = store.insert_request(request).unwrap().request_id;
        let status = if i % 2 == 0 { 200 } else { 404 };
        store.insert_response(sample_response(id, status)).unwrap();
    }

    let host_query = TimelineQuery {
        host: Some("example.com".to_string()),
        limit: 6000,
        ..TimelineQuery::default()
    };
    let host_results = store
        .query_requests(&host_query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(host_results.len(), 5000);

    let prefix_query = TimelineQuery {
        path_prefix: Some("/api".to_string()),
        limit: 6000,
        ..TimelineQuery::default()
    };
    let prefix_results = store
        .query_requests(&prefix_query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(prefix_results.len(), 2500);

    let status_query = TimelineQuery {
        status: Some(200),
        limit: 6000,
        ..TimelineQuery::default()
    };
    let status_results = store
        .query_requests(&status_query, TimelineSort::StartedAtDesc)
        .unwrap();
    assert_eq!(status_results.len(), 2500);
}
