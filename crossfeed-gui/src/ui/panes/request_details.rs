use crossfeed_ingest::TimelineItem;
use crossfeed_storage::ResponseSummary;
use iced::widget::{column, container, row};
use iced::Element;

use crate::app::Message;
use crate::theme::{ThemePalette, text_muted, text_primary};
use crate::ui::panes::{format_bytes, pane_scroll};

pub fn timeline_request_details_view(
    selected: Option<&TimelineItem>,
    response: Option<&ResponseSummary>,
    theme: ThemePalette,
) -> Element<'static, Message> {
    let content = if let Some(selected) = selected {
        let status_text = response
            .map(|resp| resp.status_code.to_string())
            .unwrap_or_else(|| "Pending".to_string());
        let duration_text = selected
            .duration_ms
            .map(|value| format!("{value} ms"))
            .unwrap_or_else(|| "-".to_string());
        let response_size = response
            .map(|resp| format_bytes(resp.body_size, resp.body_truncated))
            .unwrap_or_else(|| "-".to_string());
        let completed = selected
            .completed_at
            .as_deref()
            .unwrap_or("Pending")
            .to_string();
        let scope_current = selected
            .scope_status_current
            .as_deref()
            .unwrap_or("-")
            .to_string();
        let request_size =
            format_bytes(selected.request_body_size, selected.request_body_truncated);

        column![
            detail_line("URL", selected.url.clone(), theme),
            detail_line("Method", selected.method.clone(), theme),
            detail_line("Status", status_text, theme),
            detail_line("HTTP", selected.http_version.clone(), theme),
            detail_line("Started", selected.started_at.clone(), theme),
            detail_line("Completed", completed, theme),
            detail_line("Duration", duration_text, theme),
            detail_line("Source", selected.source.clone(), theme),
            detail_line("Scope", selected.scope_status_at_capture.clone(), theme),
            detail_line("Scope current", scope_current, theme),
            detail_line("Request size", request_size, theme),
            detail_line("Response size", response_size, theme),
        ]
    } else {
        column![text_muted("Select a request to view details", 16, theme)]
    };

    pane_scroll(container(content).padding(12).into())
}

fn detail_line(label: &'static str, value: impl Into<String>, theme: ThemePalette) -> Element<'static, Message> {
    let value = value.into();
    row![text_muted(label, 12, theme), text_primary(value, 14, theme)]
        .spacing(8)
        .into()
}
