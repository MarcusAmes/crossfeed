use crossfeed_fuzzer::{Payload, TransformStep, apply_transform_pipeline, payload_to_bytes};

#[test]
fn applies_prefix_suffix_and_transforms() {
    let payload = Payload::Text("hello".to_string());
    let mut value = payload_to_bytes(&payload);
    let mut prefixed = b"pre-".to_vec();
    prefixed.extend_from_slice(&value);
    prefixed.extend_from_slice(b"-post");
    value = prefixed;

    let steps = vec![
        TransformStep::Base64EncodeBytes,
        TransformStep::Md5Hex,
        TransformStep::UrlEncodeBytes,
    ];
    let output = apply_transform_pipeline(&value, &steps).unwrap();
    let output_text = String::from_utf8(output).unwrap();
    assert!(!output_text.is_empty());
}

#[test]
fn chain_handles_string_transforms() {
    let payload = Payload::Text("<tag>".to_string());
    let bytes = payload_to_bytes(&payload);
    let steps = vec![TransformStep::HtmlEscapeStr, TransformStep::Rot13Str];
    let output = apply_transform_pipeline(&bytes, &steps).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(!text.contains('<'));
}
