use super::types::{SocksError, SocksErrorKind, SocksResponse};
use super::client::parse_socks_response;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocksParseStatus {
    NeedMore,
    Complete { response: SocksResponse },
    Error { error: SocksError },
}

#[derive(Debug, Default)]
pub struct SocksResponseParser {
    buffer: Vec<u8>,
}

impl SocksResponseParser {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn push(&mut self, bytes: &[u8]) -> SocksParseStatus {
        self.buffer.extend_from_slice(bytes);
        match parse_socks_response(&self.buffer) {
            Ok(response) => SocksParseStatus::Complete { response },
            Err(error) => match error.kind {
                SocksErrorKind::UnexpectedEof => SocksParseStatus::NeedMore,
                _ => SocksParseStatus::Error { error },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SocksResponseParser;
    use super::SocksParseStatus;

    #[test]
    fn parses_response_across_buffers() {
        let mut parser = SocksResponseParser::new();
        let part1 = [0x05, 0x00, 0x00, 0x01];
        let part2 = [127, 0, 0, 1, 0x00, 0x50];

        assert!(matches!(parser.push(&part1), SocksParseStatus::NeedMore));
        match parser.push(&part2) {
            SocksParseStatus::Complete { response } => {
                assert_eq!(response.port, 80);
            }
            other => panic!("unexpected status {other:?}"),
        }
    }
}
