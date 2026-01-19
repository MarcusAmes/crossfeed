use assert_matches::assert_matches;
use crossfeed_codec::*;

#[test]
fn gzip_roundtrip() {
    let input = b"hello gzip";
    let compressed = gzip_compress(input).unwrap();
    let decompressed = gzip_decompress(&compressed).unwrap();
    assert_eq!(decompressed, input);
}

#[test]
fn deflate_roundtrip() {
    let input = b"hello deflate";
    let compressed = deflate_compress(input).unwrap();
    let decompressed = deflate_decompress(&compressed).unwrap();
    assert_eq!(decompressed, input);
}

#[test]
fn gzip_invalid_errors() {
    let err = gzip_decompress(b"not gzip").unwrap_err();
    assert_matches!(err, CodecError::Compression(_));
}

#[test]
fn deflate_invalid_errors() {
    let err = deflate_decompress(b"not deflate").unwrap_err();
    assert_matches!(err, CodecError::Compression(_));
}
