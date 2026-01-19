use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::{DeflateDecoder, GzDecoder};
use flate2::write::{DeflateEncoder, GzEncoder};

use crate::CodecError;

pub fn gzip_compress(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(input)
        .map_err(|err| CodecError::Compression(err.to_string()))?;
    encoder
        .finish()
        .map_err(|err| CodecError::Compression(err.to_string()))
}

pub fn gzip_decompress(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut decoder = GzDecoder::new(input);
    let mut output = Vec::new();
    decoder
        .read_to_end(&mut output)
        .map_err(|err| CodecError::Compression(err.to_string()))?;
    Ok(output)
}

pub fn deflate_compress(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(input)
        .map_err(|err| CodecError::Compression(err.to_string()))?;
    encoder
        .finish()
        .map_err(|err| CodecError::Compression(err.to_string()))
}

pub fn deflate_decompress(input: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut decoder = DeflateDecoder::new(input);
    let mut output = Vec::new();
    decoder
        .read_to_end(&mut output)
        .map_err(|err| CodecError::Compression(err.to_string()))?;
    Ok(output)
}
