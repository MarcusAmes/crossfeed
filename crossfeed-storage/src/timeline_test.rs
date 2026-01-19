use crate::timeline::{
    BodyLimits, TimelineRecorder, TimelineRequest, TimelineResponse, TimelineStore,
};

use std::sync::{Arc, Mutex};

struct MockStore {
    last_request: Mutex<Option<TimelineRequest>>,
    last_response: Mutex<Option<TimelineResponse>>,
}

impl MockStore {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            last_request: Mutex::new(None),
            last_response: Mutex::new(None),
        })
    }
}

impl TimelineStore for Arc<MockStore> {
    fn insert_request(
        &self,
        request: TimelineRequest,
    ) -> Result<crate::timeline::TimelineInsertResult, String> {
        *self.last_request.lock().unwrap() = Some(request);
        Ok(crate::timeline::TimelineInsertResult { request_id: 42 })
    }

    fn insert_response(&self, response: TimelineResponse) -> Result<(), String> {
        *self.last_response.lock().unwrap() = Some(response);
        Ok(())
    }
}

#[test]
fn body_limits_default() {
    let limits = BodyLimits::default();
    assert_eq!(limits.request_max_bytes, 40 * 1024 * 1024);
    assert_eq!(limits.response_max_bytes, 40 * 1024 * 1024);
}

#[test]
fn mock_store_captures_requests() {
    let store = MockStore::new();
    let request = TimelineRequest {
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
    };

    store.insert_request(request).unwrap();
    assert!(store.last_request.lock().unwrap().is_some());
}

#[test]
fn recorder_truncates_request_body() {
    let store = MockStore::new();
    let limits = BodyLimits {
        request_max_bytes: 4,
        response_max_bytes: 10,
    };
    let recorder = TimelineRecorder::new(Box::new(store.clone()), limits);
    let request = TimelineRequest {
        source: "proxy".to_string(),
        method: "POST".to_string(),
        scheme: "http".to_string(),
        host: "example.com".to_string(),
        port: 80,
        path: "/".to_string(),
        query: None,
        url: "http://example.com/".to_string(),
        http_version: "HTTP/1.1".to_string(),
        request_headers: b"Host: example.com\r\n".to_vec(),
        request_body: b"0123456789".to_vec(),
        request_body_size: 10,
        request_body_truncated: false,
        started_at: "now".to_string(),
        completed_at: None,
        duration_ms: None,
        scope_status_at_capture: "in_scope".to_string(),
        scope_status_current: None,
        scope_rules_version: 1,
        capture_filtered: false,
        timeline_filtered: false,
    };

    recorder.record_request(request).unwrap();
    let stored = store.last_request.lock().unwrap();
    let stored = stored.as_ref().unwrap();
    assert_eq!(stored.request_body, b"0123".to_vec());
    assert!(stored.request_body_truncated);
}

#[test]
fn mock_store_captures_response() {
    let store = MockStore::new();
    let response = TimelineResponse {
        timeline_request_id: 42,
        status_code: 200,
        reason: Some("OK".to_string()),
        response_headers: b"Content-Length: 0\r\n".to_vec(),
        response_body: Vec::new(),
        response_body_size: 0,
        response_body_truncated: false,
        http_version: "HTTP/1.1".to_string(),
        received_at: "now".to_string(),
    };

    store.insert_response(response).unwrap();
    assert!(store.last_response.lock().unwrap().is_some());
}

#[test]
fn recorder_truncates_response_body() {
    let store = MockStore::new();
    let limits = BodyLimits {
        request_max_bytes: 4,
        response_max_bytes: 5,
    };
    let recorder = TimelineRecorder::new(Box::new(store.clone()), limits);
    let response = TimelineResponse {
        timeline_request_id: 42,
        status_code: 200,
        reason: Some("OK".to_string()),
        response_headers: b"Content-Length: 0\r\n".to_vec(),
        response_body: b"abcdefgh".to_vec(),
        response_body_size: 8,
        response_body_truncated: false,
        http_version: "HTTP/1.1".to_string(),
        received_at: "now".to_string(),
    };

    recorder.record_response(response).unwrap();
    let stored = store.last_response.lock().unwrap();
    let stored = stored.as_ref().unwrap();
    assert_eq!(stored.response_body, b"abcde".to_vec());
    assert!(stored.response_body_truncated);
}
