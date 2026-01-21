use std::sync::OnceLock;

use crate::http2::types::{HeaderField, Http2Error, Http2ErrorKind};
use hpack::{Decoder, Encoder};

pub struct HpackDecoder {
    inner: Decoder<'static>,
    max_table_size: u32,
}

static HPACK_SELF_TEST: OnceLock<()> = OnceLock::new();

impl HpackDecoder {
    pub fn new() -> Self {
        run_hpack_self_test();
        Self {
            inner: Decoder::new(),
            max_table_size: 0,
        }
    }

    pub fn set_max_table_size(&mut self, size: u32) {
        self.inner.set_max_table_size(size as usize);
        self.max_table_size = size;
    }

    pub fn max_table_size(&self) -> u32 {
        self.max_table_size
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
            .map_err(|_err| Http2Error {
                kind: Http2ErrorKind::HpackDecode,
                offset: 0,
            })
    }
}

fn run_hpack_self_test() {
    HPACK_SELF_TEST.get_or_init(|| {
        let mut decoder = Decoder::new();
        let _ = decoder.decode(b"\x82");
    });
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
        self.inner.encode(
            headers
                .iter()
                .map(|header| (header.name.as_slice(), header.value.as_slice())),
        )
    }
}
