use std::collections::HashMap;
use std::path::PathBuf;

use crossfeed_codec::{deflate_decompress, gzip_decompress};
use crossfeed_ingest::{TailCursor, TailUpdate, TimelineItem};
use crossfeed_storage::{ProjectConfig, ProjectPaths, ResponseSummary, SqliteStore, TimelineQuery, TimelineSort};
use iced::widget::{PaneGrid, button, column, container, pane_grid, row, scrollable, text};
use iced::{Element, Length, Theme};
use serde::{Deserialize, Serialize};

use crate::app::Message;
use crate::theme::{
    ThemePalette, badge_style, pane_border_style, text_muted, text_primary, timeline_row_style,
};

#[derive(Debug, Clone)]
pub struct TimelineState {
    panes: pane_grid::State<PaneKind>,
    pub timeline: Vec<TimelineItem>,
    pub selected: Option<usize>,
    pub project_root: PathBuf,
    pub project_paths: ProjectPaths,
    pub project_config: ProjectConfig,
    pub store_path: PathBuf,
    pub tags: HashMap<i64, Vec<String>>,
    pub responses: HashMap<i64, ResponseSummary>,
    pub tail_cursor: TailCursor,
}

impl TimelineState {
    pub fn new(project_paths: ProjectPaths, project_config: ProjectConfig) -> Result<Self, String> {
        let store_path = project_paths.database.clone();
        let store = SqliteStore::open(&store_path)?;
        let requests = store
            .query_request_summaries(&TimelineQuery::default(), TimelineSort::StartedAtDesc)?;
        let ids: Vec<i64> = requests.iter().map(|item| item.id).collect();
        let tags = store.get_request_tags(&ids)?;
        let responses = store.get_response_summaries(&ids)?;
        let timeline: Vec<TimelineItem> = requests.into_iter().map(TimelineItem::from).collect();

        let (mut panes, root) = pane_grid::State::new(PaneKind::Timeline);
        let (right, _) = panes
            .split(pane_grid::Axis::Vertical, root, PaneKind::Detail)
            .ok_or_else(|| "Unable to split timeline pane".to_string())?;
        let _ = panes
            .split(pane_grid::Axis::Horizontal, right, PaneKind::Response)
            .ok_or_else(|| "Unable to split detail pane".to_string())?;

        let tail_cursor = TailCursor::from_items(&timeline);

        Ok(Self {
            panes,
            timeline,
            selected: None,
            project_root: project_paths.root.clone(),
            project_paths,
            project_config,
            store_path,
            tags,
            responses,
            tail_cursor,
        })
    }

    pub fn view(&self, focus: crate::app::FocusArea, theme: &ThemePalette) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |_, state, _| {
            let pane_content: Element<'_, Message> = match state {
                PaneKind::Timeline => self.timeline_view(focus, *theme),
                PaneKind::Detail => self.detail_view(focus, *theme),
                PaneKind::Response => self.response_view(focus, *theme),
            };
            let content = container(pane_content).style({
                let theme = *theme;
                move |_| pane_border_style(theme)
            });
            let title = state.title();
            let title_text = text(title).size(13).style({
                let theme = *theme;
                move |_theme: &Theme| iced::widget::text::Style {
                    color: Some(theme.text),
                }
            });
            pane_grid::Content::new(content).title_bar(
                pane_grid::TitleBar::new(title_text)
                    .padding(6)
                    .style({
                        let theme = *theme;
                        move |_| crate::theme::menu_bar_style(theme)
                    }),
            )
        })
        .on_drag(Message::PaneDragged)
        .on_resize(10, Message::PaneResized);

        container(grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn timeline_view(
        &self,
        _focus: crate::app::FocusArea,
        theme: ThemePalette,
    ) -> Element<'_, Message> {
        let mut content = column![].spacing(12);

        for (index, item) in self.timeline.iter().enumerate() {
            let is_selected = self.selected == Some(index);
            let tags = self.tags.get(&item.id).cloned().unwrap_or_default();
            let response = self.responses.get(&item.id);
            let status = response.map(|resp| resp.status_code);
            let row = timeline_row(item, status, &tags, is_selected, theme)
                .on_press(Message::TimelineSelected(index));
            content = content.push(row);
        }

        scrollable(content).into()
    }

    fn detail_view(&self, _focus: crate::app::FocusArea, theme: ThemePalette) -> Element<'_, Message> {
        let content = if let Some(selected) = self.selected.and_then(|idx| self.timeline.get(idx)) {
            let response = self.responses.get(&selected.id);
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

        scrollable(container(content).padding(12)).into()
    }

    fn response_view(&self, _focus: crate::app::FocusArea, theme: ThemePalette) -> Element<'_, Message> {
        let content = if let Some(selected) = self.selected.and_then(|idx| self.timeline.get(idx)) {
            let response = self.responses.get(&selected.id);

            if let Some(response) = response {
                let timeline_response = SqliteStore::open(&self.store_path)
                    .ok()
                    .and_then(|store| store.get_response_by_request_id(selected.id).ok())
                    .and_then(|opt| opt);
                let response_headers = timeline_response
                    .as_ref()
                    .map(|resp| resp.response_headers.as_slice())
                    .unwrap_or(&[]);
                let body = timeline_response
                    .as_ref()
                    .map(|resp| resp.response_body.as_slice())
                    .unwrap_or(&[]);
                let headers = render_response_headers(response_headers);
                let body_text = render_response_body(body, &headers);
                let status_line = response
                    .reason
                    .clone()
                    .map(|reason| format!("{} {reason}", response.status_code))
                    .unwrap_or_else(|| response.status_code.to_string());
                let body_label = if timeline_response
                    .as_ref()
                    .map(|resp| resp.response_body_truncated)
                    .unwrap_or(false)
                {
                    "Body (truncated)"
                } else {
                    "Body"
                };
                column![
                    detail_line("Status", status_line, theme),
                    text_muted("Headers", 14, theme),
                    container(text_primary(headers, 12, theme)).padding(10),
                    text_muted(body_label, 14, theme),
                    container(text_primary(body_text, 12, theme)).padding(10),
                ]
            } else {
                column![text_muted("No response recorded yet", 16, theme)]
            }
        } else {
            column![text_muted("Select a request to preview response", 16, theme)]
        };

        scrollable(container(content).padding(12)).into()
    }

    pub fn apply_tail_update(&mut self, update: Result<TailUpdate, String>) {
        let Ok(update) = update else {
            return;
        };
        if update.new_items.is_empty() {
            return;
        }
        for item in update.new_items.iter().rev() {
            self.timeline.insert(0, item.clone());
        }
        for (id, tags) in update.tags {
            self.tags.insert(id, tags);
        }
        for (id, response) in update.responses {
            self.responses.insert(id, response);
        }
        self.tail_cursor = update.cursor;
    }

    pub fn select_next(&mut self) {
        if self.timeline.is_empty() {
            self.selected = None;
            return;
        }
        let next = match self.selected {
            Some(index) => (index + 1).min(self.timeline.len() - 1),
            None => 0,
        };
        self.selected = Some(next);
    }

    pub fn select_prev(&mut self) {
        if self.timeline.is_empty() {
            self.selected = None;
            return;
        }
        let prev = match self.selected {
            Some(index) => index.saturating_sub(1),
            None => 0,
        };
        self.selected = Some(prev);
    }

    fn snapshot_layout(&self) -> PaneLayout {
        PaneLayout::from(&self.panes)
    }

    pub fn apply_layout(&mut self, layout: PaneLayout) {
        self.panes = pane_grid::State::with_configuration(layout.to_configuration());
    }

    pub fn handle_pane_drag(&mut self, event: pane_grid::DragEvent) -> Option<PaneLayout> {
        match event {
            pane_grid::DragEvent::Dropped { pane, target } => {
                self.panes.drop(pane, target);
                Some(self.snapshot_layout())
            }
            pane_grid::DragEvent::Picked { .. } => None,
            pane_grid::DragEvent::Canceled { .. } => None,
        }
    }

    pub fn handle_pane_resize(&mut self, event: pane_grid::ResizeEvent) -> Option<PaneLayout> {
        self.panes.resize(event.split, event.ratio);
        Some(self.snapshot_layout())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneKind {
    Timeline,
    Detail,
    Response,
}

impl PaneKind {
    fn title(self) -> &'static str {
        match self {
            PaneKind::Timeline => "Timeline",
            PaneKind::Detail => "Request Details",
            PaneKind::Response => "Response Preview",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneLayout {
    root: LayoutNode,
}

impl PaneLayout {
    pub fn from(state: &pane_grid::State<PaneKind>) -> Self {
        Self {
            root: LayoutNode::from(state.layout(), state),
        }
    }

    pub fn to_configuration(&self) -> pane_grid::Configuration<PaneKind> {
        self.root.to_configuration()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum LayoutNode {
    Pane(PaneKind),
    Split {
        axis: LayoutAxis,
        ratio: f32,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
}

impl LayoutNode {
    fn from(node: &pane_grid::Node, panes: &pane_grid::State<PaneKind>) -> Self {
        match node {
            pane_grid::Node::Pane(pane) => {
                let kind = panes.get(*pane).copied().unwrap_or(PaneKind::Timeline);
                LayoutNode::Pane(kind)
            }
            pane_grid::Node::Split {
                axis, ratio, a, b, ..
            } => LayoutNode::Split {
                axis: LayoutAxis::from(*axis),
                ratio: *ratio,
                a: Box::new(LayoutNode::from(a, panes)),
                b: Box::new(LayoutNode::from(b, panes)),
            },
        }
    }

    fn to_configuration(&self) -> pane_grid::Configuration<PaneKind> {
        match self {
            LayoutNode::Pane(pane) => pane_grid::Configuration::Pane(*pane),
            LayoutNode::Split { axis, ratio, a, b } => pane_grid::Configuration::Split {
                axis: axis.to_axis(),
                ratio: *ratio,
                a: Box::new(a.to_configuration()),
                b: Box::new(b.to_configuration()),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum LayoutAxis {
    Horizontal,
    Vertical,
}

impl LayoutAxis {
    fn from(axis: pane_grid::Axis) -> Self {
        match axis {
            pane_grid::Axis::Horizontal => LayoutAxis::Horizontal,
            pane_grid::Axis::Vertical => LayoutAxis::Vertical,
        }
    }

    fn to_axis(self) -> pane_grid::Axis {
        match self {
            LayoutAxis::Horizontal => pane_grid::Axis::Horizontal,
            LayoutAxis::Vertical => pane_grid::Axis::Vertical,
        }
    }
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

fn detail_line(label: &'static str, value: impl Into<String>, theme: ThemePalette) -> Element<'static, Message> {
    let value = value.into();
    row![text_muted(label, 12, theme), text_primary(value, 14, theme)]
        .spacing(8)
        .into()
}

fn format_bytes(bytes: usize, truncated: bool) -> String {
    let base = if bytes > 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes > 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    };
    if truncated {
        format!("{base} (truncated)")
    } else {
        base
    }
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
