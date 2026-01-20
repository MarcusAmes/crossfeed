use crossfeed_codec as codec;

use crate::{FuzzError, Payload, TransformStep};

pub fn payload_to_bytes(payload: &Payload) -> Vec<u8> {
    match payload {
        Payload::Text(text) => text.as_bytes().to_vec(),
        Payload::Bytes(bytes) => bytes.clone(),
    }
}

pub fn apply_transform_pipeline(
    input: &[u8],
    steps: &[TransformStep],
) -> Result<Vec<u8>, FuzzError> {
    let mut current = input.to_vec();
    for step in steps {
        current = apply_transform(&current, step)?;
    }
    Ok(current)
}

fn apply_transform(input: &[u8], step: &TransformStep) -> Result<Vec<u8>, FuzzError> {
    match step {
        TransformStep::UrlEncodeBytes => Ok(codec::url_encode_bytes(input).into_bytes()),
        TransformStep::UrlEncodeStr => {
            Ok(codec::url_encode_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::UrlDecodeBytes => codec::url_decode_bytes(input).map_err(map_err),
        TransformStep::UrlDecodeStr => codec::url_decode_str(&codec::bytes_to_string_lossy(input))
            .map(|value| value.into_bytes())
            .map_err(map_err),
        TransformStep::Base64EncodeBytes => Ok(codec::base64_encode_bytes(input).into_bytes()),
        TransformStep::Base64EncodeStr => {
            Ok(codec::base64_encode_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::Base64DecodeBytes => codec::base64_decode_bytes(input).map_err(map_err),
        TransformStep::Base64DecodeStr => {
            codec::base64_decode_str(&codec::bytes_to_string_lossy(input)).map_err(map_err)
        }
        TransformStep::Base64UrlEncodeBytes => {
            Ok(codec::base64url_encode_bytes(input).into_bytes())
        }
        TransformStep::Base64UrlEncodeStr => {
            Ok(codec::base64url_encode_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::Base64UrlDecodeBytes => {
            codec::base64url_decode_bytes(input).map_err(map_err)
        }
        TransformStep::Base64UrlDecodeStr => {
            codec::base64url_decode_str(&codec::bytes_to_string_lossy(input)).map_err(map_err)
        }
        TransformStep::HexEncodeBytes => Ok(codec::hex_encode_bytes(input).into_bytes()),
        TransformStep::HexEncodeStr => {
            Ok(codec::hex_encode_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::HexDecodeBytes => codec::hex_decode_bytes(input).map_err(map_err),
        TransformStep::HexDecodeStr => {
            codec::hex_decode_str(&codec::bytes_to_string_lossy(input)).map_err(map_err)
        }
        TransformStep::Base32EncodeBytes => Ok(codec::base32_encode_bytes(input).into_bytes()),
        TransformStep::Base32EncodeStr => {
            Ok(codec::base32_encode_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::Base32DecodeBytes => codec::base32_decode_bytes(input).map_err(map_err),
        TransformStep::Base32DecodeStr => {
            codec::base32_decode_str(&codec::bytes_to_string_lossy(input)).map_err(map_err)
        }
        TransformStep::Base58EncodeBytes => Ok(codec::base58_encode_bytes(input).into_bytes()),
        TransformStep::Base58EncodeStr => {
            Ok(codec::base58_encode_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::Base58DecodeBytes => codec::base58_decode_bytes(input).map_err(map_err),
        TransformStep::Base58DecodeStr => {
            codec::base58_decode_str(&codec::bytes_to_string_lossy(input)).map_err(map_err)
        }
        TransformStep::HtmlEscapeStr => {
            Ok(codec::html_escape_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::HtmlUnescapeStr => {
            Ok(codec::html_unescape_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::Rot13Str => {
            Ok(codec::rot13_str(&codec::bytes_to_string_lossy(input)).into_bytes())
        }
        TransformStep::GzipCompress => codec::gzip_compress(input).map_err(map_err),
        TransformStep::GzipDecompress => codec::gzip_decompress(input).map_err(map_err),
        TransformStep::DeflateCompress => codec::deflate_compress(input).map_err(map_err),
        TransformStep::DeflateDecompress => codec::deflate_decompress(input).map_err(map_err),
        TransformStep::Md5Hex => Ok(codec::md5_hex(input).into_bytes()),
        TransformStep::Md5Bytes => Ok(hex::decode(codec::md5_hex(input)).map_err(map_err)?),
        TransformStep::Sha1Hex => Ok(codec::sha1_hex(input).into_bytes()),
        TransformStep::Sha1Bytes => Ok(hex::decode(codec::sha1_hex(input)).map_err(map_err)?),
        TransformStep::Sha224Hex => Ok(codec::sha224_hex(input).into_bytes()),
        TransformStep::Sha224Bytes => Ok(hex::decode(codec::sha224_hex(input)).map_err(map_err)?),
        TransformStep::Sha256Hex => Ok(codec::sha256_hex(input).into_bytes()),
        TransformStep::Sha256Bytes => Ok(hex::decode(codec::sha256_hex(input)).map_err(map_err)?),
        TransformStep::Sha384Hex => Ok(codec::sha384_hex(input).into_bytes()),
        TransformStep::Sha384Bytes => Ok(hex::decode(codec::sha384_hex(input)).map_err(map_err)?),
        TransformStep::Sha512Hex => Ok(codec::sha512_hex(input).into_bytes()),
        TransformStep::Sha512Bytes => Ok(hex::decode(codec::sha512_hex(input)).map_err(map_err)?),
    }
}

fn map_err(error: impl std::fmt::Display) -> FuzzError {
    FuzzError::Transform(error.to_string())
}
