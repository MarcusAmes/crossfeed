use crate::http2::types::{HeaderField, Http2Error, Http2ErrorKind};
use hpack::{Decoder, Encoder};

pub struct HpackDecoder {
    inner: Decoder<'static>,
}

impl HpackDecoder {
    pub fn new() -> Self {
        Self {
            inner: Decoder::new(),
        }
    }

    pub fn decode(&mut self, block: &[u8]) -> Result<Vec<HeaderField>, Http2Error> {
        self.inner
            .decode(block)
            .map(|headers| {
                headers
                    .into_iter()
                    .map(|(name, value)| HeaderField { name, value })
                    .collect()
            })
            .map_err(|_| Http2Error {
                kind: Http2ErrorKind::HpackDecode,
                offset: 0,
            })
    }
}

pub struct HpackEncoder {
    inner: Encoder<'static>,
}

impl HpackEncoder {
    pub fn new() -> Self {
        Self {
            inner: Encoder::new(),
        }
    }

    pub fn encode(&mut self, headers: &[HeaderField]) -> Vec<u8> {
        self.inner
            .encode(headers.iter().map(|header| (header.name.as_slice(), header.value.as_slice())))
    }
}
