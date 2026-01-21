use std::collections::VecDeque;

use super::hpack::HpackDecoder;
use super::types::{
    DataFrame, Frame, FrameHeader, FramePayload, FrameType, GoAwayFrame, HeadersFrame, Http2Error,
    Http2ErrorKind, Http2Warning, Http2WarningKind, PingFrame, PriorityFrame, RstStreamFrame,
    SettingsFrame, WindowUpdateFrame,
};

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
const FRAME_HEADER_LEN: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Http2ParseStatus {
    NeedMore {
        warnings: Vec<Http2Warning>,
    },
    Complete {
        frame: Frame,
        warnings: Vec<Http2Warning>,
    },
    Error {
        error: Http2Error,
        warnings: Vec<Http2Warning>,
    },
}

pub struct Http2Parser {
    buffer: Vec<u8>,
    warnings: Vec<Http2Warning>,
    preface_seen: bool,
    max_frame_size: usize,
    hpack: HpackDecoder,
    header_block: Option<HeaderBlockBuffer>,
    settings_received: bool,
    pending_headers: VecDeque<PendingHeaderBlock>,
    max_pending_headers: usize,
    decoded_headers_count: usize,
}

impl Default for Http2Parser {
    fn default() -> Self {
        Self {
            buffer: Vec::new(),
            warnings: Vec::new(),
            preface_seen: false,
            max_frame_size: 16 * 1024,
            hpack: HpackDecoder::new(),
            header_block: None,
            settings_received: false,
            pending_headers: VecDeque::new(),
            max_pending_headers: 8,
            decoded_headers_count: 0,
        }
    }
}

impl Http2Parser {
    pub fn new() -> Self {
        let mut parser = Self::default();
        parser.hpack.set_max_table_size(65_536);
        parser
    }

    pub fn new_without_preface() -> Self {
        let mut parser = Self::default();
        parser.hpack.set_max_table_size(65_536);
        parser.preface_seen = true;
        parser
    }

    pub fn with_max_frame_size(max_frame_size: usize) -> Self {
        let mut parser = Self {
            buffer: Vec::new(),
            warnings: Vec::new(),
            preface_seen: false,
            max_frame_size,
            hpack: HpackDecoder::new(),
            header_block: None,
            settings_received: false,
            pending_headers: VecDeque::new(),
            max_pending_headers: 8,
            decoded_headers_count: 0,
        };
        parser.hpack.set_max_table_size(65_536);
        parser
    }

    pub fn set_max_frame_size(&mut self, max_frame_size: usize) {
        self.max_frame_size = max_frame_size;
    }

    pub fn set_max_header_table_size(&mut self, size: u32) {
        self.hpack.set_max_table_size(size);
    }

    pub fn set_settings_received(&mut self, received: bool) {
        self.settings_received = received;
    }

    pub fn push(&mut self, bytes: &[u8]) -> Http2ParseStatus {
        self.buffer.extend_from_slice(bytes);
        self.try_parse()
    }

    fn try_parse(&mut self) -> Http2ParseStatus {
        if !self.preface_seen {
            if self.buffer.len() < PREFACE.len() {
                return Http2ParseStatus::NeedMore {
                    warnings: self.warnings.clone(),
                };
            }
            if &self.buffer[..PREFACE.len()] != PREFACE {
                let error = Http2Error {
                    kind: Http2ErrorKind::InvalidPreface,
                    offset: 0,
                };
                let warnings = std::mem::take(&mut self.warnings);
                return Http2ParseStatus::Error { error, warnings };
            }
            self.buffer.drain(..PREFACE.len());
            self.preface_seen = true;
        }

        if self.settings_received {
            if let Some(pending) = self.pending_headers.pop_front() {
                match self.hpack.decode(&pending.header_block) {
                    Ok(decoded) => {
                        self.decoded_headers_count += 1;
                        let frame = Frame {
                            header: FrameHeader {
                                length: pending.header_block.len(),
                                frame_type: FrameType::Headers,
                                flags: pending.flags,
                                stream_id: pending.stream_id,
                            },
                            payload: FramePayload::Headers(HeadersFrame {
                                end_stream: pending.end_stream,
                                end_headers: true,
                                header_block: pending.header_block,
                                headers: decoded,
                            }),
                        };
                        let warnings = std::mem::take(&mut self.warnings);
                        return Http2ParseStatus::Complete { frame, warnings };
                    }
                    Err(error) => {
                        let warnings = std::mem::take(&mut self.warnings);
                        return Http2ParseStatus::Error { error, warnings };
                    }
                }
            }
        }

        match parse_frame(&self.buffer, self.max_frame_size, &mut self.warnings) {
            Ok(ParseFrameResult::NeedMore) => Http2ParseStatus::NeedMore {
                warnings: self.warnings.clone(),
            },
            Ok(ParseFrameResult::Complete { frame, consumed }) => {
                self.buffer.drain(..consumed);
                match self.attach_header_block(frame) {
                    Ok(Some(frame)) => {
                        let warnings = std::mem::take(&mut self.warnings);
                        Http2ParseStatus::Complete { frame, warnings }
                    }
                    Ok(None) => Http2ParseStatus::NeedMore {
                        warnings: self.warnings.clone(),
                    },
                    Err(error) => {
                        let warnings = std::mem::take(&mut self.warnings);
                        Http2ParseStatus::Error { error, warnings }
                    }
                }
            }
            Err(error) => {
                let warnings = std::mem::take(&mut self.warnings);
                Http2ParseStatus::Error { error, warnings }
            }
        }
    }

    fn attach_header_block(&mut self, frame: Frame) -> Result<Option<Frame>, Http2Error> {
        match frame.payload {
            FramePayload::Headers(headers) => self.handle_headers_frame(frame.header, headers),
            FramePayload::Continuation(fragment) => {
                self.handle_continuation_frame(frame.header, fragment)
            }
            _ => Ok(Some(frame)),
        }
    }

    fn handle_headers_frame(
        &mut self,
        header: FrameHeader,
        headers: HeadersFrame,
    ) -> Result<Option<Frame>, Http2Error> {
        let block = HeaderBlockBuffer {
            stream_id: header.stream_id,
            end_stream: headers.end_stream,
            fragments: headers.header_block,
        };

        if headers.end_headers {
            if !self.settings_received {
                self.queue_pending_headers(block, header.flags)?;
                return Ok(None);
            }

            self.decoded_headers_count += 1;
            let decoded = self.hpack.decode(&block.fragments)?;
            let frame = Frame {
                header,
                payload: FramePayload::Headers(HeadersFrame {
                    end_stream: headers.end_stream,
                    end_headers: true,
                    header_block: block.fragments,
                    headers: decoded,
                }),
            };
            return Ok(Some(frame));
        }

        self.header_block = Some(block);
        Ok(None)
    }

    fn handle_continuation_frame(
        &mut self,
        header: FrameHeader,
        fragment: Vec<u8>,
    ) -> Result<Option<Frame>, Http2Error> {
        let Some(mut pending) = self.header_block.take() else {
            self.warnings.push(Http2Warning {
                kind: Http2WarningKind::HeadersContinuationMismatch,
                offset: 0,
            });
            return Ok(Some(Frame {
                header,
                payload: FramePayload::Continuation(fragment),
            }));
        };

        if pending.stream_id != header.stream_id {
            self.warnings.push(Http2Warning {
                kind: Http2WarningKind::HeadersContinuationMismatch,
                offset: 0,
            });
        }

        pending.fragments.extend_from_slice(&fragment);

        let end_headers = header.flags & 0x4 != 0;
        if !end_headers {
            self.header_block = Some(pending);
            return Ok(None);
        }

        if !self.settings_received {
            self.queue_pending_headers(pending, header.flags)?;
            return Ok(None);
        }

        self.decoded_headers_count += 1;
        let decoded = self.hpack.decode(&pending.fragments)?;
        let frame = Frame {
            header: FrameHeader {
                length: header.length,
                frame_type: FrameType::Headers,
                flags: header.flags,
                stream_id: pending.stream_id,
            },
            payload: FramePayload::Headers(HeadersFrame {
                end_stream: pending.end_stream,
                end_headers: true,
                header_block: pending.fragments,
                headers: decoded,
            }),
        };

        Ok(Some(frame))
    }

    fn queue_pending_headers(
        &mut self,
        pending: HeaderBlockBuffer,
        flags: u8,
    ) -> Result<(), Http2Error> {
        if self.pending_headers.len() >= self.max_pending_headers {
            return Err(Http2Error {
                kind: Http2ErrorKind::PendingHeadersOverflow,
                offset: 0,
            });
        }
        self.pending_headers.push_back(PendingHeaderBlock {
            stream_id: pending.stream_id,
            end_stream: pending.end_stream,
            flags,
            header_block: pending.fragments,
        });
        Ok(())
    }
}

#[derive(Debug)]
struct HeaderBlockBuffer {
    stream_id: u32,
    end_stream: bool,
    fragments: Vec<u8>,
}

#[derive(Debug)]
struct PendingHeaderBlock {
    stream_id: u32,
    end_stream: bool,
    flags: u8,
    header_block: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
enum ParseFrameResult {
    NeedMore,
    Complete { frame: Frame, consumed: usize },
}

fn parse_frame(
    buffer: &[u8],
    max_frame_size: usize,
    warnings: &mut Vec<Http2Warning>,
) -> Result<ParseFrameResult, Http2Error> {
    if buffer.len() < FRAME_HEADER_LEN {
        return Ok(ParseFrameResult::NeedMore);
    }

    let length = ((buffer[0] as usize) << 16) | ((buffer[1] as usize) << 8) | buffer[2] as usize;
    let frame_type = buffer[3];
    let flags = buffer[4];
    let stream_id = ((buffer[5] as u32) << 24)
        | ((buffer[6] as u32) << 16)
        | ((buffer[7] as u32) << 8)
        | buffer[8] as u32;
    let stream_id = stream_id & 0x7FFF_FFFF;

    if length > max_frame_size {
        warnings.push(Http2Warning {
            kind: Http2WarningKind::FrameTooLarge {
                declared: length,
                max: max_frame_size,
            },
            offset: 0,
        });
    }

    let total_len = FRAME_HEADER_LEN + length;
    if buffer.len() < total_len {
        return Ok(ParseFrameResult::NeedMore);
    }

    let payload = &buffer[FRAME_HEADER_LEN..total_len];
    let frame_type = match frame_type {
        0x0 => FrameType::Data,
        0x1 => FrameType::Headers,
        0x2 => FrameType::Priority,
        0x3 => FrameType::RstStream,
        0x4 => FrameType::Settings,
        0x5 => FrameType::PushPromise,
        0x6 => FrameType::Ping,
        0x7 => FrameType::GoAway,
        0x8 => FrameType::WindowUpdate,
        0x9 => FrameType::Continuation,
        other => {
            warnings.push(Http2Warning {
                kind: Http2WarningKind::UnknownFrameType(other),
                offset: 3,
            });
            FrameType::Unknown(other)
        }
    };

    let header = FrameHeader {
        length,
        frame_type: frame_type.clone(),
        flags,
        stream_id,
    };

    let payload = decode_payload(frame_type, flags, stream_id, payload)?;

    Ok(ParseFrameResult::Complete {
        frame: Frame { header, payload },
        consumed: total_len,
    })
}

fn decode_payload(
    frame_type: FrameType,
    flags: u8,
    stream_id: u32,
    payload: &[u8],
) -> Result<FramePayload, Http2Error> {
    match frame_type {
        FrameType::Data => Ok(FramePayload::Data(DataFrame {
            end_stream: flags & 0x1 != 0,
            payload: payload.to_vec(),
        })),
        FrameType::Headers => {
            let header_block = extract_header_block(payload, flags)?;
            Ok(FramePayload::Headers(HeadersFrame {
                end_stream: flags & 0x1 != 0,
                end_headers: flags & 0x4 != 0,
                header_block,
                headers: Vec::new(),
            }))
        }
        FrameType::Priority => {
            if payload.len() < 5 {
                return Err(Http2Error {
                    kind: Http2ErrorKind::IncompleteFrame,
                    offset: 0,
                });
            }
            let dep = ((payload[0] as u32) << 24)
                | ((payload[1] as u32) << 16)
                | ((payload[2] as u32) << 8)
                | payload[3] as u32;
            let exclusive = dep & 0x8000_0000 != 0;
            let stream_dependency = dep & 0x7FFF_FFFF;
            let weight = payload[4];

            Ok(FramePayload::Priority(PriorityFrame {
                stream_dependency,
                weight,
                exclusive,
            }))
        }
        FrameType::RstStream => {
            if payload.len() < 4 {
                return Err(Http2Error {
                    kind: Http2ErrorKind::IncompleteFrame,
                    offset: 0,
                });
            }
            let error_code = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
            Ok(FramePayload::RstStream(RstStreamFrame { error_code }))
        }
        FrameType::Settings => {
            let ack = flags & 0x1 != 0;
            if ack {
                return Ok(FramePayload::Settings(SettingsFrame {
                    settings: Vec::new(),
                    ack: true,
                }));
            }
            if payload.len() % 6 != 0 {
                return Err(Http2Error {
                    kind: Http2ErrorKind::InvalidFrameHeader,
                    offset: 0,
                });
            }
            let mut settings = Vec::new();
            for chunk in payload.chunks(6) {
                let id = u16::from_be_bytes([chunk[0], chunk[1]]);
                let value = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
                settings.push((id, value));
            }
            Ok(FramePayload::Settings(SettingsFrame {
                settings,
                ack: false,
            }))
        }
        FrameType::Ping => {
            if payload.len() != 8 {
                return Err(Http2Error {
                    kind: Http2ErrorKind::InvalidFrameHeader,
                    offset: 0,
                });
            }
            let mut opaque_data = [0u8; 8];
            opaque_data.copy_from_slice(payload);
            Ok(FramePayload::Ping(PingFrame {
                opaque_data,
                ack: flags & 0x1 != 0,
            }))
        }
        FrameType::GoAway => {
            if payload.len() < 8 {
                return Err(Http2Error {
                    kind: Http2ErrorKind::IncompleteFrame,
                    offset: 0,
                });
            }
            let last_stream_id =
                u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) & 0x7FFF_FFFF;
            let error_code = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
            let debug_data = payload[8..].to_vec();
            Ok(FramePayload::GoAway(GoAwayFrame {
                last_stream_id,
                error_code,
                debug_data,
            }))
        }
        FrameType::WindowUpdate => {
            if payload.len() < 4 {
                return Err(Http2Error {
                    kind: Http2ErrorKind::IncompleteFrame,
                    offset: 0,
                });
            }
            let increment =
                u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) & 0x7FFF_FFFF;
            Ok(FramePayload::WindowUpdate(WindowUpdateFrame {
                stream_id,
                increment,
            }))
        }
        FrameType::Continuation => Ok(FramePayload::Continuation(payload.to_vec())),
        FrameType::PushPromise | FrameType::Unknown(_) => Ok(FramePayload::Raw(payload.to_vec())),
    }
}

fn extract_header_block(payload: &[u8], flags: u8) -> Result<Vec<u8>, Http2Error> {
    let mut offset = 0;
    let mut pad_len = 0usize;

    if flags & 0x8 != 0 {
        if payload.is_empty() {
            return Err(Http2Error {
                kind: Http2ErrorKind::IncompleteFrame,
                offset: 0,
            });
        }
        pad_len = payload[0] as usize;
        offset = 1;
    }

    if flags & 0x20 != 0 {
        if payload.len() < offset + 5 {
            return Err(Http2Error {
                kind: Http2ErrorKind::IncompleteFrame,
                offset: 0,
            });
        }
        offset += 5;
    }

    if payload.len() < offset + pad_len {
        return Err(Http2Error {
            kind: Http2ErrorKind::IncompleteFrame,
            offset: 0,
        });
    }

    let end = payload.len() - pad_len;
    if end < offset {
        return Err(Http2Error {
            kind: Http2ErrorKind::IncompleteFrame,
            offset: 0,
        });
    }

    Ok(payload[offset..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::{Http2ParseStatus, Http2Parser};

    #[test]
    fn requires_preface() {
        let mut parser = Http2Parser::new();
        let status = parser.push(b"not preface");

        assert!(matches!(status, Http2ParseStatus::NeedMore { .. }));

        let status = parser.push(b"more data that completes the preface");
        assert!(matches!(status, Http2ParseStatus::Error { .. }));
    }

    #[test]
    fn parses_settings_frame() {
        let mut parser = Http2Parser::new();
        let mut input = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
        input.extend_from_slice(&[
            0x00, 0x00, 0x06, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, // header
            0x00, 0x01, 0x00, 0x00, 0x10, 0x00, // setting
        ]);

        let status = parser.push(&input);
        match status {
            Http2ParseStatus::Complete { frame, .. } => {
                assert_eq!(frame.header.length, 6);
            }
            other => panic!("unexpected status {other:?}"),
        }
    }

    #[test]
    fn parses_data_frame() {
        let mut parser = Http2Parser::new();
        let mut input = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
        input.extend_from_slice(&[
            0x00, 0x00, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // header
            b'h', b'e', b'l', b'l', b'o',
        ]);

        let status = parser.push(&input);
        match status {
            Http2ParseStatus::Complete { frame, .. } => {
                assert_eq!(frame.header.length, 5);
            }
            other => panic!("unexpected status {other:?}"),
        }
    }
}
