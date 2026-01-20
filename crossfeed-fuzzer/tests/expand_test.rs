use crossfeed_fuzzer::{
    AnalysisConfig, FuzzRunConfig, Payload, PlaceholderSpec, TransformStep, expand_fuzz_requests,
    parse_template,
};

#[test]
fn expands_single_placeholder() {
    let bytes = b"GET /?q=<<CFUZZ:1>> HTTP/1.1\r\n\r\n";
    let template = parse_template(bytes, &FuzzRunConfig::default().placeholder_prefix).unwrap();
    let specs = vec![PlaceholderSpec {
        index: 1,
        payloads: vec![
            Payload::Text("a".to_string()),
            Payload::Text("b".to_string()),
        ],
        transforms: Vec::new(),
        prefix: None,
        suffix: None,
    }];
    let requests = expand_fuzz_requests(&template, &specs).unwrap();
    assert_eq!(requests.len(), 2);
}

#[test]
fn expands_multiple_placeholders() {
    let bytes = b"GET /?a=<<CFUZZ:1>>&b=<<CFUZZ:2>> HTTP/1.1\r\n\r\n";
    let template = parse_template(bytes, &FuzzRunConfig::default().placeholder_prefix).unwrap();
    let specs = vec![
        PlaceholderSpec {
            index: 1,
            payloads: vec![
                Payload::Text("x".to_string()),
                Payload::Text("y".to_string()),
            ],
            transforms: vec![TransformStep::Base64EncodeBytes],
            prefix: None,
            suffix: None,
        },
        PlaceholderSpec {
            index: 2,
            payloads: vec![
                Payload::Text("1".to_string()),
                Payload::Text("2".to_string()),
            ],
            transforms: Vec::new(),
            prefix: Some(b"p".to_vec()),
            suffix: Some(b"s".to_vec()),
        },
    ];
    let requests = expand_fuzz_requests(&template, &specs).unwrap();
    assert_eq!(requests.len(), 4);
    let joined = String::from_utf8(requests[0].clone()).unwrap();
    assert!(joined.contains("/"));
}

#[test]
fn analysis_defaults_empty() {
    let config = AnalysisConfig::default();
    assert!(config.grep.is_empty());
    assert!(config.extract.is_empty());
}
