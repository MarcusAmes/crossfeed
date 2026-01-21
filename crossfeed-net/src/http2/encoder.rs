use super::hpack::HpackEncoder;
use super::types::{
    Frame, FramePayload, FrameType, GoAwayFrame, HeaderField, PingFrame, PriorityFrame,
    RstStreamFrame, SettingsFrame, WindowUpdateFrame,
};

pub const DEFAULT_MAX_FRAME_SIZE: usize = 16 * 1024;

pub fn encode_frames(
    frame: &Frame,
    encoder: &mut HpackEncoder,
    max_frame_size: usize,
) -> Vec<Vec<u8>> {
    let stream_id = frame.header.stream_id;
    match &frame.payload {
        FramePayload::Data(data_frame) => {
            encode_data_frames(stream_id, data_frame.end_stream, &data_frame.payload, max_frame_size)
        }
        FramePayload::Headers(headers_frame) => encode_headers_from_fields(
            stream_id,
            headers_frame.end_stream,
            &headers_frame.headers,
            encoder,
            max_frame_size,
        ),
        FramePayload::Priority(priority_frame) => {
            vec![encode_priority_frame(stream_id, priority_frame)]
        }
        FramePayload::RstStream(rst_frame) => vec![encode_rst_stream_frame(stream_id, rst_frame)],
        FramePayload::Settings(settings_frame) => vec![encode_settings_frame(settings_frame)],
        FramePayload::Ping(ping_frame) => vec![encode_ping_frame(ping_frame)],
        FramePayload::GoAway(goaway_frame) => vec![encode_goaway_frame(goaway_frame)],
        FramePayload::WindowUpdate(window_frame) => vec![encode_window_update_frame(window_frame)],
        FramePayload::Continuation(payload) => vec![encode_raw_frame(
            FrameType::Continuation,
            frame.header.flags,
            stream_id,
            payload,
        )],
        FramePayload::Raw(payload) => vec![encode_raw_frame(
            frame.header.frame_type.clone(),
            frame.header.flags,
            stream_id,
            payload,
        )],
    }
}

pub fn encode_headers_from_fields(
    stream_id: u32,
    end_stream: bool,
    headers: &[HeaderField],
    encoder: &mut HpackEncoder,
    max_frame_size: usize,
) -> Vec<Vec<u8>> {
    let header_block = encoder.encode(headers);
    encode_headers_from_block(stream_id, end_stream, &header_block, max_frame_size)
}

pub fn encode_headers_from_block(
    stream_id: u32,
    end_stream: bool,
    header_block: &[u8],
    max_frame_size: usize,
) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut offset = 0;
    let total = header_block.len();
    let mut first = true;

    while offset < total || (total == 0 && first) {
        let remaining = total.saturating_sub(offset);
        let chunk_len = remaining.min(max_frame_size);
        let end_headers = offset + chunk_len >= total;
        let payload = &header_block[offset..offset + chunk_len];
        let (frame_type, flags) = if first {
            let mut flags = if end_headers { 0x4 } else { 0x0 };
            if end_stream {
                flags |= 0x1;
            }
            (FrameType::Headers, flags)
        } else {
            let flags = if end_headers { 0x4 } else { 0x0 };
            (FrameType::Continuation, flags)
        };
        frames.push(encode_raw_frame(frame_type, flags, stream_id, payload));
        offset += chunk_len;
        first = false;
        if total == 0 {
            break;
        }
    }

    frames
}

pub fn encode_data_frames(
    stream_id: u32,
    end_stream: bool,
    payload: &[u8],
    max_frame_size: usize,
) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut offset = 0;
    let total = payload.len();

    if total == 0 {
        let flags = if end_stream { 0x1 } else { 0x0 };
        frames.push(encode_raw_frame(FrameType::Data, flags, stream_id, &[]));
        return frames;
    }

    while offset < total {
        let remaining = total - offset;
        let chunk_len = remaining.min(max_frame_size);
        let chunk_end = offset + chunk_len;
        let is_last = chunk_end >= total;
        let flags = if end_stream && is_last { 0x1 } else { 0x0 };
        frames.push(encode_raw_frame(
            FrameType::Data,
            flags,
            stream_id,
            &payload[offset..chunk_end],
        ));
        offset = chunk_end;
    }

    frames
}

pub fn encode_settings_frame(settings: &SettingsFrame) -> Vec<u8> {
    if settings.ack {
        return encode_raw_frame(FrameType::Settings, 0x1, 0, &[]);
    }

    let mut payload = Vec::with_capacity(settings.settings.len() * 6);
    for (id, value) in &settings.settings {
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&value.to_be_bytes());
    }

    encode_raw_frame(FrameType::Settings, 0x0, 0, &payload)
}

pub fn encode_ping_frame(frame: &PingFrame) -> Vec<u8> {
    let flags = if frame.ack { 0x1 } else { 0x0 };
    encode_raw_frame(FrameType::Ping, flags, 0, &frame.opaque_data)
}

pub fn encode_goaway_frame(frame: &GoAwayFrame) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8 + frame.debug_data.len());
    payload.extend_from_slice(&(frame.last_stream_id & 0x7FFF_FFFF).to_be_bytes());
    payload.extend_from_slice(&frame.error_code.to_be_bytes());
    payload.extend_from_slice(&frame.debug_data);
    encode_raw_frame(FrameType::GoAway, 0x0, 0, &payload)
}

pub fn encode_window_update_frame(frame: &WindowUpdateFrame) -> Vec<u8> {
    let mut payload = Vec::with_capacity(4);
    payload.extend_from_slice(&(frame.increment & 0x7FFF_FFFF).to_be_bytes());
    encode_raw_frame(FrameType::WindowUpdate, 0x0, frame.stream_id, &payload)
}

pub fn encode_priority_frame(stream_id: u32, frame: &PriorityFrame) -> Vec<u8> {
    let mut payload = Vec::with_capacity(5);
    let dep = frame.stream_dependency & 0x7FFF_FFFF;
    let dep = if frame.exclusive { dep | 0x8000_0000 } else { dep };
    payload.extend_from_slice(&dep.to_be_bytes());
    payload.push(frame.weight);
    encode_raw_frame(FrameType::Priority, 0x0, stream_id, &payload)
}

pub fn encode_rst_stream_frame(stream_id: u32, frame: &RstStreamFrame) -> Vec<u8> {
    encode_raw_frame(FrameType::RstStream, 0x0, stream_id, &frame.error_code.to_be_bytes())
}

pub fn encode_raw_frame(frame_type: FrameType, flags: u8, stream_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(9 + payload.len());
    frame.extend_from_slice(&encode_frame_header(payload.len(), frame_type_id(&frame_type), flags, stream_id));
    frame.extend_from_slice(payload);
    frame
}

fn encode_frame_header(length: usize, frame_type: u8, flags: u8, stream_id: u32) -> [u8; 9] {
    let length = length.min(0x00FF_FFFF);
    let stream_id = stream_id & 0x7FFF_FFFF;
    [
        ((length >> 16) & 0xFF) as u8,
        ((length >> 8) & 0xFF) as u8,
        (length & 0xFF) as u8,
        frame_type,
        flags,
        ((stream_id >> 24) & 0xFF) as u8,
        ((stream_id >> 16) & 0xFF) as u8,
        ((stream_id >> 8) & 0xFF) as u8,
        (stream_id & 0xFF) as u8,
    ]
}

fn frame_type_id(frame_type: &FrameType) -> u8 {
    match frame_type {
        FrameType::Data => 0x0,
        FrameType::Headers => 0x1,
        FrameType::Priority => 0x2,
        FrameType::RstStream => 0x3,
        FrameType::Settings => 0x4,
        FrameType::PushPromise => 0x5,
        FrameType::Ping => 0x6,
        FrameType::GoAway => 0x7,
        FrameType::WindowUpdate => 0x8,
        FrameType::Continuation => 0x9,
        FrameType::Unknown(value) => *value,
    }
}
