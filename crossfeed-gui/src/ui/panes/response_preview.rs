use crossfeed_codec::{deflate_decompress, gzip_decompress};
use iced::widget::{column, container};
use iced::Element;

use crate::app::Message;
use crate::theme::{ThemePalette, text_muted, text_primary};
use crate::ui::panes::pane_scroll;

pub fn response_preview_from_bytes(
    status_line: String,
    response_headers: &[u8],
    response_body: &[u8],
    body_truncated: bool,
    theme: ThemePalette,
) -> Element<'static, Message> {
    let headers = render_response_headers(response_headers);
    let body_text = render_response_body(response_body, &headers);
    let body_label = if body_truncated {
        "Body (truncated)"
    } else {
        "Body"
    };
    let content = column![
        detail_line("Status", status_line, theme),
        text_muted("Headers", 14, theme),
        container(text_primary(headers, 12, theme)).padding(10),
        text_muted(body_label, 14, theme),
        container(text_primary(body_text, 12, theme)).padding(10),
    ];

    pane_scroll(container(content).padding(12).into())
}

pub fn response_preview_placeholder(
    message: &str,
    theme: ThemePalette,
) -> Element<'static, Message> {
    let content = column![text_muted(message, 16, theme)];
    pane_scroll(container(content).padding(12).into())
}

fn detail_line(label: &'static str, value: impl Into<String>, theme: ThemePalette) -> Element<'static, Message> {
    let value = value.into();
    iced::widget::row![text_muted(label, 12, theme), text_primary(value, 14, theme)]
        .spacing(8)
        .into()
}

fn render_response_headers(raw: &[u8]) -> String {
    if raw.is_empty() {
        return "(no headers)".to_string();
    }
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 2)
        .unwrap_or(raw.len());
    let header_bytes = &raw[..header_end];
    let text = String::from_utf8_lossy(header_bytes).replace("\r\n", "\n");
    if text.trim().is_empty() {
        "(no headers)".to_string()
    } else {
        text
    }
}

fn render_response_body(body: &[u8], headers: &str) -> String {
    if body.is_empty() {
        return "(empty body)".to_string();
    }
    let decoded = decode_response_body(body, headers);
    match std::str::from_utf8(&decoded) {
        Ok(text) => text.to_string(),
        Err(_) => hex_dump(&decoded),
    }
}

fn decode_response_body(body: &[u8], headers: &str) -> Vec<u8> {
    let encoding = find_header_value(headers, "content-encoding")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let encoding = encoding
        .split(',')
        .next()
        .map(|value| value.trim())
        .unwrap_or("");
    match encoding {
        "gzip" | "x-gzip" => gzip_decompress(body).unwrap_or_else(|_| body.to_vec()),
        "deflate" => deflate_decompress(body).unwrap_or_else(|_| body.to_vec()),
        _ => body.to_vec(),
    }
}

fn find_header_value(headers: &str, name: &str) -> Option<String> {
    headers.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn hex_dump(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "(empty body)".to_string();
    }
    let mut output = String::new();
    for (line_index, chunk) in bytes.chunks(16).enumerate() {
        if line_index > 0 {
            output.push('\n');
        }
        for (index, byte) in chunk.iter().enumerate() {
            if index > 0 {
                output.push(' ');
            }
            output.push_str(&format!("{:02x}", byte));
        }
    }
    output
}
