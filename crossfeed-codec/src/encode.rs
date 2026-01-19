use std::borrow::Cow;

use base64::Engine;
use percent_encoding::percent_decode;

use crate::CodecError;

pub fn url_encode_bytes(input: &[u8]) -> String {
    percent_encoding::percent_encode(input, percent_encoding::NON_ALPHANUMERIC).to_string()
}

pub fn url_encode_str(input: &str) -> String {
    url_encode_bytes(input.as_bytes())
}

pub fn url_decode_bytes(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    let decoded = percent_decode(input)
        .decode_utf8()
        .map_err(|err| CodecError::Url(err.to_string()))?;
    Ok(decoded.into_owned().into_bytes())
}

pub fn url_decode_str(input: &str) -> Result<String, CodecError> {
    let decoded = percent_decode(input.as_bytes()).decode_utf8_lossy();
    Ok(decoded.into_owned())
}

pub fn base64_encode_bytes(input: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(input)
}

pub fn base64_encode_str(input: &str) -> String {
    base64_encode_bytes(input.as_bytes())
}

pub fn base64_decode_bytes(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map_err(|err| CodecError::Base64(err.to_string()))
}

pub fn base64_decode_str(input: &str) -> Result<Vec<u8>, CodecError> {
    base64_decode_bytes(input.as_bytes())
}

pub fn base64url_encode_bytes(input: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE.encode(input)
}

pub fn base64url_encode_str(input: &str) -> String {
    base64url_encode_bytes(input.as_bytes())
}

pub fn base64url_decode_bytes(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    base64::engine::general_purpose::URL_SAFE
        .decode(input)
        .map_err(|err| CodecError::Base64Url(err.to_string()))
}

pub fn base64url_decode_str(input: &str) -> Result<Vec<u8>, CodecError> {
    base64url_decode_bytes(input.as_bytes())
}

pub fn hex_encode_bytes(input: &[u8]) -> String {
    hex::encode(input)
}

pub fn hex_encode_str(input: &str) -> String {
    hex_encode_bytes(input.as_bytes())
}

pub fn hex_decode_bytes(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    hex::decode(input).map_err(|err| CodecError::Hex(err.to_string()))
}

pub fn hex_decode_str(input: &str) -> Result<Vec<u8>, CodecError> {
    hex_decode_bytes(input.as_bytes())
}

pub fn base32_encode_bytes(input: &[u8]) -> String {
    data_encoding::BASE32.encode(input)
}

pub fn base32_encode_str(input: &str) -> String {
    base32_encode_bytes(input.as_bytes())
}

pub fn base32_decode_bytes(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    data_encoding::BASE32
        .decode(input)
        .map_err(|err| CodecError::Base32(err.to_string()))
}

pub fn base32_decode_str(input: &str) -> Result<Vec<u8>, CodecError> {
    base32_decode_bytes(input.as_bytes())
}

pub fn base58_encode_bytes(input: &[u8]) -> String {
    bs58::encode(input).into_string()
}

pub fn base58_encode_str(input: &str) -> String {
    base58_encode_bytes(input.as_bytes())
}

pub fn base58_decode_bytes(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    let input = std::str::from_utf8(input).map_err(|err| CodecError::Base58(err.to_string()))?;
    bs58::decode(input)
        .into_vec()
        .map_err(|err| CodecError::Base58(err.to_string()))
}

pub fn base58_decode_str(input: &str) -> Result<Vec<u8>, CodecError> {
    base58_decode_bytes(input.as_bytes())
}

pub fn html_escape_str(input: &str) -> String {
    html_escape::encode_safe(input).to_string()
}

pub fn html_unescape_str(input: &str) -> String {
    html_escape::decode_html_entities(input).to_string()
}

pub fn rot13_str(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        let mapped = match ch {
            'a'..='z' => (((ch as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((ch as u8 - b'A' + 13) % 26) + b'A') as char,
            _ => ch,
        };
        output.push(mapped);
    }
    output
}

pub fn bytes_to_string_lossy(input: &[u8]) -> String {
    Cow::from(String::from_utf8_lossy(input)).into_owned()
}

pub fn string_to_bytes(input: &str) -> Vec<u8> {
    input.as_bytes().to_vec()
}
