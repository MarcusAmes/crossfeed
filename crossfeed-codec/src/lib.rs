mod compress;
mod encode;
mod error;
mod hash;

pub use compress::{deflate_compress, deflate_decompress, gzip_compress, gzip_decompress};
pub use encode::{
    base32_decode_bytes, base32_decode_str, base32_encode_bytes, base32_encode_str,
    base58_decode_bytes, base58_decode_str, base58_encode_bytes, base58_encode_str,
    base64_decode_bytes, base64_decode_str, base64_encode_bytes, base64_encode_str,
    base64url_decode_bytes, base64url_decode_str, base64url_encode_bytes, base64url_encode_str,
    bytes_to_string_lossy, hex_decode_bytes, hex_decode_str, hex_encode_bytes, hex_encode_str,
    html_escape_str, html_unescape_str, rot13_str, string_to_bytes, url_decode_bytes,
    url_decode_str, url_encode_bytes, url_encode_str,
};
pub use error::CodecError;
pub use hash::{md5_hex, sha1_hex, sha224_hex, sha256_hex, sha384_hex, sha512_hex};
