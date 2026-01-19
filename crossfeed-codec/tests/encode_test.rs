use assert_matches::assert_matches;
use crossfeed_codec::*;

#[test]
fn url_roundtrip_bytes() {
    let input = b"hello world?=\n";
    let encoded = url_encode_bytes(input);
    let decoded = url_decode_bytes(encoded.as_bytes()).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn url_decode_str_is_lossy() {
    let input = "%26";
    let decoded = url_decode_str(input).unwrap();
    assert_eq!(decoded, "&")
}

#[test]
fn base64_roundtrip() {
    let input = b"hello";
    let encoded = base64_encode_bytes(input);
    let decoded = base64_decode_bytes(encoded.as_bytes()).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn base64url_roundtrip() {
    let input = b"hello?";
    let encoded = base64url_encode_bytes(input);
    let decoded = base64url_decode_bytes(encoded.as_bytes()).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn base64_invalid_errors() {
    let err = base64_decode_str("@@@").unwrap_err();
    assert_matches!(err, CodecError::Base64(_));
}

#[test]
fn hex_roundtrip() {
    let input = b"hello";
    let encoded = hex_encode_bytes(input);
    let decoded = hex_decode_bytes(encoded.as_bytes()).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn hex_invalid_errors() {
    let err = hex_decode_str("zz").unwrap_err();
    assert_matches!(err, CodecError::Hex(_));
}

#[test]
fn base32_roundtrip() {
    let input = b"hello";
    let encoded = base32_encode_bytes(input);
    let decoded = base32_decode_bytes(encoded.as_bytes()).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn base32_invalid_errors() {
    let err = base32_decode_str("@@@").unwrap_err();
    assert_matches!(err, CodecError::Base32(_));
}

#[test]
fn base58_roundtrip() {
    let input = b"hello";
    let encoded = base58_encode_bytes(input);
    let decoded = base58_decode_bytes(encoded.as_bytes()).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn base58_invalid_errors() {
    let err = base58_decode_str("0OIl").unwrap_err();
    assert_matches!(err, CodecError::Base58(_));
}

#[test]
fn html_escape_unescape_roundtrip() {
    let input = "<div>";
    let escaped = html_escape_str(input);
    let unescaped = html_unescape_str(&escaped);
    assert_eq!(unescaped, input);
}

#[test]
fn rot13_roundtrip() {
    let input = "Hello World";
    let encoded = rot13_str(input);
    let decoded = rot13_str(&encoded);
    assert_eq!(decoded, input);
}

#[test]
fn string_bytes_helpers() {
    let input = "hello";
    let bytes = string_to_bytes(input);
    let output = bytes_to_string_lossy(&bytes);
    assert_eq!(output, input);
}
