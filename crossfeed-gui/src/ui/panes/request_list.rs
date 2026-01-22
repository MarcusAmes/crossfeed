use std::collections::HashMap;

use crossfeed_ingest::TimelineItem;
use crossfeed_storage::ResponseSummary;
use iced::widget::{button, column, container, row, scrollable};
use iced::{Element, Length};

use crate::app::Message;
use crate::theme::{ThemePalette, badge_style, text_muted, text_primary, timeline_row_style};
use crate::ui::panes::format_bytes;

pub fn timeline_request_list_view(
    items: &[TimelineItem],
    tags: &HashMap<i64, Vec<String>>,
    responses: &HashMap<i64, ResponseSummary>,
    selected: Option<usize>,
    theme: ThemePalette,
) -> Element<'static, Message> {
    let mut content = column![].spacing(12);

    for (index, item) in items.iter().enumerate() {
        let is_selected = selected == Some(index);
        let tags = tags.get(&item.id).cloned().unwrap_or_default();
        let response = responses.get(&item.id);
        let status = response.map(|resp| resp.status_code);
        let row = timeline_row(item, status, &tags, is_selected, theme)
            .on_press(Message::TimelineSelected(index));
        content = content.push(row);
    }

    scrollable(content).into()
}

fn timeline_row(
    item: &TimelineItem,
    status: Option<u16>,
    tags: &[String],
    selected: bool,
    theme: ThemePalette,
) -> iced::widget::Button<'static, Message> {
    let status_text = status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "-".to_string());
    let tag_label = if tags.is_empty() {
        "".to_string()
    } else if tags.len() <= 3 {
        tags.join(" · ")
    } else {
        format!("{} · +{}", tags[..3].join(" · "), tags.len() - 3)
    };
    let info = format!(
        "{} {}{}",
        item.host,
        item.path,
        item.completed_at
            .as_ref()
            .map(|value| format!(" ({value})"))
            .unwrap_or_default()
    );
    let duration = item
        .duration_ms
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "-".to_string());
    let body_size = format_bytes(item.request_body_size, item.request_body_truncated);

    let row = column![
        row![
            badge(item.method.clone(), theme),
            badge(status_text.clone(), theme),
            text_primary(info, 14, theme),
        ]
        .spacing(8),
        row![
            text_muted(format!("{} • {}", duration, body_size), 12, theme),
            text_muted(tag_label, 12, theme),
        ]
        .spacing(8),
    ]
    .spacing(4);

    button(row)
        .padding(10)
        .width(Length::Fill)
        .style(move |_theme, status| timeline_row_style(theme, status, selected))
}

fn badge(label: String, theme: ThemePalette) -> Element<'static, Message> {
    container(text_primary(label, 12, theme))
        .padding(6)
        .style(move |_| badge_style(theme))
        .into()
}
