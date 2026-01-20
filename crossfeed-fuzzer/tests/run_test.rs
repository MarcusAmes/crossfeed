use crossfeed_fuzzer::{AnalysisConfig, FuzzResult, FuzzRunConfig, analyze_response, run_fuzz};
use crossfeed_storage::{TimelineRequest, TimelineResponse};

fn sample_request() -> TimelineRequest {
    TimelineRequest {
        source: "fuzzer".to_string(),
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

fn sample_response(body: &[u8]) -> TimelineResponse {
    TimelineResponse {
        timeline_request_id: 0,
        status_code: 200,
        reason: Some("OK".to_string()),
        response_headers: b"Content-Length: 0\r\n".to_vec(),
        response_body: body.to_vec(),
        response_body_size: body.len(),
        response_body_truncated: false,
        http_version: "HTTP/1.1".to_string(),
        received_at: "now".to_string(),
    }
}

#[test]
fn analysis_matches_grep_and_extract() {
    let analysis = AnalysisConfig {
        grep: vec!["needle".to_string()],
        extract: vec!["n(eed)le".to_string()],
    };
    let result = analyze_response(b"needle", &analysis).unwrap();
    assert_eq!(result.grep_matches, vec!["needle".to_string()]);
    assert_eq!(result.extracts[0], vec!["eed".to_string()]);
}

#[test]
fn run_fuzz_streams_results() {
    let analysis = AnalysisConfig::default();
    let config = FuzzRunConfig::default();
    let template = crossfeed_fuzzer::FuzzTemplate {
        request_bytes: Vec::new(),
        placeholders: Vec::new(),
    };
    let specs = Vec::new();

    let responses = vec![
        (sample_request(), sample_response(b"one")),
        (sample_request(), sample_response(b"two")),
    ];

    let mut ids = Vec::new();
    let mut sender = |_: TimelineRequest, _: TimelineResponse| {
        let id = ids.len() as i64 + 1;
        ids.push(id);
        Ok(id)
    };

    let stream = run_fuzz(
        &template,
        &specs,
        &analysis,
        &config,
        &mut sender,
        responses,
    );
    let stream = std::pin::pin!(stream);
    let collected: Vec<Result<FuzzResult, _>> = futures_executor::block_on_stream(stream).collect();
    assert_eq!(collected.len(), 2);
}
