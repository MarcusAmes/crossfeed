use crossfeed_fuzzer::{FuzzRunConfig, parse_template};

#[test]
fn parses_placeholders_with_default_prefix() {
    let bytes = b"GET /?q=<<CFUZZ:1>> HTTP/1.1\r\nHost: example.com\r\n\r\n";
    let template = parse_template(bytes, &FuzzRunConfig::default().placeholder_prefix).unwrap();
    assert_eq!(template.placeholders.len(), 1);
    assert_eq!(template.placeholders[0].index, 1);
    assert_eq!(template.placeholders[0].ranges.len(), 1);
}

#[test]
fn parses_multiple_occurrences() {
    let bytes = b"POST / <<CFUZZ:1>> <<CFUZZ:1>>\r\n\r\n";
    let template = parse_template(bytes, &FuzzRunConfig::default().placeholder_prefix).unwrap();
    assert_eq!(template.placeholders.len(), 1);
    assert_eq!(template.placeholders[0].ranges.len(), 2);
}

#[test]
fn parses_custom_prefix() {
    let bytes = b"GET /?q=<<ALT:2>> HTTP/1.1\r\n\r\n";
    let template = parse_template(bytes, "<<ALT").unwrap();
    assert_eq!(template.placeholders.len(), 1);
    assert_eq!(template.placeholders[0].index, 2);
}
