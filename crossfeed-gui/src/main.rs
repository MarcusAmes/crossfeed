use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crossfeed_storage::{
    ProjectConfig, ProjectLayout, ProjectPaths, ResponseSummary, SqliteStore, TimelineQuery,
    TimelineRequestSummary, TimelineSort,
};
use iced::event;
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::{
    PaneGrid, button, column, container, pane_grid, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};
use serde::{Deserialize, Serialize};

const APP_NAME: &str = "Crossfeed";
const CONFIG_FILENAME: &str = "gui.toml";

fn main() -> iced::Result {
    iced::application(APP_NAME, AppState::update, AppState::view)
        .subscription(AppState::subscription)
        .theme(AppState::theme)
        .run_with(AppState::new)
}

#[derive(Debug, Clone)]
enum Screen {
    ProjectPicker(ProjectPickerState),
    Timeline(TimelineState),
}

#[derive(Debug, Clone)]
enum Message {
    LoadedConfig(Result<GuiConfig, String>),
    OpenProjectRequested,
    CreateProjectRequested,
    ProjectPathChanged(String),
    ConfirmProject,
    CancelProject,
    ProjectOpened(Result<TimelineState, String>),
    PaneDragged(pane_grid::DragEvent),
    PaneResized(pane_grid::ResizeEvent),
    TimelineSelected(usize),
    KeyPressed(keyboard::Key, Modifiers),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusArea {
    Timeline,
    Detail,
    Response,
    ProjectPicker,
}

#[derive(Debug, Clone)]
struct AppState {
    screen: Screen,
    config: GuiConfig,
    focus: FocusArea,
}

impl AppState {
    fn new() -> (Self, Task<Message>) {
        let config_path = gui_config_path();
        let task = Task::perform(load_gui_config(config_path.clone()), Message::LoadedConfig);
        (
            Self {
                screen: Screen::ProjectPicker(ProjectPickerState::default()),
                config: GuiConfig::default(),
                focus: FocusArea::ProjectPicker,
            },
            task,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LoadedConfig(result) => {
                if let Ok(config) = result {
                    self.config = config.clone();
                    if let Some(path) = config.last_project.clone() {
                        if let Screen::ProjectPicker(picker) = &mut self.screen {
                            picker.pending_path = path.to_string_lossy().into_owned();
                        }
                        if path.exists() {
                            return Task::perform(
                                open_project(path, ProjectIntent::Open),
                                Message::ProjectOpened,
                            );
                        }
                    }
                }
                Task::none()
            }
            Message::OpenProjectRequested | Message::CreateProjectRequested => {
                let mut picker = ProjectPickerState::default();
                picker.intent = match message {
                    Message::CreateProjectRequested => ProjectIntent::Create,
                    _ => ProjectIntent::Open,
                };
                if let Screen::ProjectPicker(current) = &self.screen {
                    picker.pending_path = current.pending_path.clone();
                    picker.error = None;
                } else if let Some(path) = self.config.last_project.clone() {
                    picker.pending_path = path.to_string_lossy().into_owned();
                }
                self.screen = Screen::ProjectPicker(picker);
                self.focus = FocusArea::ProjectPicker;
                Task::none()
            }
            Message::ProjectPathChanged(path) => {
                if let Screen::ProjectPicker(picker) = &mut self.screen {
                    picker.pending_path = path;
                }
                Task::none()
            }
            Message::ConfirmProject => {
                let path = if let Screen::ProjectPicker(picker) = &self.screen {
                    PathBuf::from(picker.pending_path.trim())
                } else {
                    PathBuf::new()
                };
                if path.as_os_str().is_empty() {
                    return Task::none();
                }
                let intent = match &self.screen {
                    Screen::ProjectPicker(picker) => picker.intent,
                    _ => ProjectIntent::Open,
                };
                Task::perform(open_project(path, intent), Message::ProjectOpened)
            }
            Message::CancelProject => Task::none(),
            Message::ProjectOpened(result) => match result {
                Ok(mut timeline) => {
                    self.focus = FocusArea::Timeline;
                    self.config.last_project = Some(timeline.project_root.clone());
                    if let Some(layout) = self.config.pane_layout.take() {
                        timeline.apply_layout(layout);
                    }
                    self.screen = Screen::Timeline(timeline);
                    Task::perform(
                        save_gui_config(gui_config_path(), self.config.clone()),
                        |_| Message::CancelProject,
                    )
                }
                Err(error) => {
                    if let Screen::ProjectPicker(picker) = &mut self.screen {
                        picker.error = Some(error);
                    }
                    Task::none()
                }
            },
            Message::PaneDragged(event) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    if let Some(snapshot) = state.handle_pane_drag(event) {
                        self.config.pane_layout = Some(snapshot);
                    }
                }
                Task::none()
            }
            Message::PaneResized(event) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    if let Some(snapshot) = state.handle_pane_resize(event) {
                        self.config.pane_layout = Some(snapshot);
                    }
                }
                Task::none()
            }
            Message::TimelineSelected(index) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    state.selected = Some(index);
                }
                Task::none()
            }
            Message::KeyPressed(key, modifiers) => self.handle_key(key, modifiers),
        }
    }

    fn handle_key(&mut self, key: keyboard::Key, modifiers: Modifiers) -> Task<Message> {
        match key {
            Key::Named(keyboard::key::Named::ArrowDown) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    state.select_next();
                }
            }
            Key::Named(keyboard::key::Named::ArrowUp) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    state.select_prev();
                }
            }
            Key::Named(keyboard::key::Named::Enter) => {
                if self.focus == FocusArea::Timeline {
                    self.focus = FocusArea::Detail;
                }
            }
            Key::Named(keyboard::key::Named::Escape) => {
                if matches!(self.focus, FocusArea::Detail | FocusArea::Response) {
                    self.focus = FocusArea::Timeline;
                }
            }
            Key::Named(keyboard::key::Named::Tab) => {
                if self.focus != FocusArea::ProjectPicker {
                    let backward = modifiers.shift();
                    self.focus = match (self.focus, backward) {
                        (FocusArea::Timeline, false) => FocusArea::Detail,
                        (FocusArea::Detail, false) => FocusArea::Response,
                        (FocusArea::Response, false) => FocusArea::Timeline,
                        (FocusArea::Timeline, true) => FocusArea::Response,
                        (FocusArea::Detail, true) => FocusArea::Timeline,
                        (FocusArea::Response, true) => FocusArea::Detail,
                        (FocusArea::ProjectPicker, _) => FocusArea::ProjectPicker,
                    };
                }
            }
            Key::Character(ch)
                if ch == "1" && modifiers.control() && self.focus != FocusArea::ProjectPicker =>
            {
                self.focus = FocusArea::Timeline;
            }
            Key::Character(ch)
                if ch == "2" && modifiers.control() && self.focus != FocusArea::ProjectPicker =>
            {
                self.focus = FocusArea::Detail;
            }
            Key::Character(ch)
                if ch == "3" && modifiers.control() && self.focus != FocusArea::ProjectPicker =>
            {
                self.focus = FocusArea::Response;
            }
            _ => {}
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::ProjectPicker(picker) => picker.view(),
            Screen::Timeline(state) => state.view(self.focus),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _status, _id| match event {
            event::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                Some(Message::KeyPressed(key, modifiers))
            }
            _ => None,
        })
    }

    fn theme(&self) -> Theme {
        Theme::TokyoNight
    }
}

impl Default for FocusArea {
    fn default() -> Self {
        FocusArea::ProjectPicker
    }
}

#[derive(Debug, Clone)]
struct ProjectPickerState {
    intent: ProjectIntent,
    error: Option<String>,
    pending_path: String,
}

impl Default for ProjectPickerState {
    fn default() -> Self {
        Self {
            intent: ProjectIntent::Open,
            error: None,
            pending_path: String::new(),
        }
    }
}

impl ProjectPickerState {
    fn view(&self) -> Element<'_, Message> {
        let title = match self.intent {
            ProjectIntent::Open => "Open Crossfeed Project",
            ProjectIntent::Create => "Create Crossfeed Project",
        };
        let action_label = match self.intent {
            ProjectIntent::Open => "Open project",
            ProjectIntent::Create => "Create project",
        };
        let mut content = column![
            text(title).size(28),
            text("Enter the project directory path.").size(14),
            text_input("/path/to/project", &self.pending_path)
                .on_input(Message::ProjectPathChanged)
                .padding(10)
                .size(16),
            row![
                button(action_label).on_press(Message::ConfirmProject),
                button("Cancel").on_press(Message::CancelProject),
            ]
            .spacing(12),
            row![
                button("Open existing").on_press(Message::OpenProjectRequested),
                button("Create new").on_press(Message::CreateProjectRequested),
            ]
            .spacing(12),
        ]
        .spacing(16)
        .align_x(Alignment::Start);

        if let Some(error) = &self.error {
            content = content.push(text(error).style(|theme: &Theme| {
                let palette = theme.extended_palette();
                iced::widget::text::Style {
                    color: Some(palette.danger.strong.color),
                }
            }));
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(40)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectIntent {
    Open,
    Create,
}

#[derive(Debug, Clone)]
struct TimelineState {
    panes: pane_grid::State<PaneKind>,
    timeline: Vec<TimelineItem>,
    selected: Option<usize>,
    project_root: PathBuf,
    store_path: PathBuf,
    tags: HashMap<i64, Vec<String>>,
    responses: HashMap<i64, ResponseSummary>,
}

impl TimelineState {
    fn new(project_root: PathBuf, store_path: PathBuf) -> Result<Self, String> {
        let store = SqliteStore::open(&store_path)?;
        let query = TimelineQuery::default();
        let requests = store.query_request_summaries(&query, TimelineSort::StartedAtDesc)?;
        let ids: Vec<i64> = requests.iter().map(|item| item.id).collect();
        let tags = store.get_request_tags(&ids)?;
        let responses = store.get_response_summaries(&ids)?;
        let timeline = requests.into_iter().map(TimelineItem::from).collect();

        let (mut panes, root) = pane_grid::State::new(PaneKind::Timeline);
        let (right, _) = panes
            .split(pane_grid::Axis::Vertical, root, PaneKind::Detail)
            .ok_or_else(|| "Unable to split timeline pane".to_string())?;
        let _ = panes
            .split(pane_grid::Axis::Horizontal, right, PaneKind::Response)
            .ok_or_else(|| "Unable to split detail pane".to_string())?;

        Ok(Self {
            panes,
            timeline,
            selected: None,
            project_root,
            store_path,
            tags,
            responses,
        })
    }

    fn view(&self, focus: FocusArea) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |_, state, _| {
            let content: Element<'_, Message> = match state {
                PaneKind::Timeline => self.timeline_view(focus),
                PaneKind::Detail => self.detail_view(focus),
                PaneKind::Response => self.response_view(focus),
            };
            let title = state.title();
            pane_grid::Content::new(content)
                .title_bar(pane_grid::TitleBar::new(text(title)).padding(6))
        })
        .on_drag(Message::PaneDragged)
        .on_resize(10, Message::PaneResized);

        container(grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn timeline_view(&self, _focus: FocusArea) -> Element<'_, Message> {
        let mut content = column![
            text("Timeline").size(20),
            text(format!("Project: {}", self.project_root.display())).size(12),
        ]
        .spacing(8);

        for (index, item) in self.timeline.iter().enumerate() {
            let is_selected = self.selected == Some(index);
            let tags = self.tags.get(&item.id).cloned().unwrap_or_default();
            let response = self.responses.get(&item.id);
            let status = response.map(|resp| resp.status_code);
            let row = timeline_row(item, status, &tags, is_selected)
                .on_press(Message::TimelineSelected(index));
            content = content.push(row);
        }

        scrollable(content).into()
    }

    fn detail_view(&self, _focus: FocusArea) -> Element<'_, Message> {
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
                detail_line("URL", selected.url.clone()),
                detail_line("Method", selected.method.clone()),
                detail_line("Status", status_text),
                detail_line("HTTP", selected.http_version.clone()),
                detail_line("Started", selected.started_at.clone()),
                detail_line("Completed", completed),
                detail_line("Duration", duration_text),
                detail_line("Source", selected.source.clone()),
                detail_line("Scope", selected.scope_status_at_capture.clone()),
                detail_line("Scope current", scope_current),
                detail_line("Request size", request_size),
                detail_line("Response size", response_size),
            ]
        } else {
            column![text("Select a request to view details").size(16)]
        };

        scrollable(container(content).padding(12)).into()
    }

    fn response_view(&self, _focus: FocusArea) -> Element<'_, Message> {
        let content = if let Some(selected) = self.selected.and_then(|idx| self.timeline.get(idx)) {
            let response = self.responses.get(&selected.id);

            if let Some(response) = response {
                let store = SqliteStore::open(&self.store_path).ok();
                let body = store
                    .and_then(|store| store.get_response_by_request_id(selected.id).ok())
                    .and_then(|opt| opt)
                    .map(|resp| resp.response_body)
                    .unwrap_or_default();
                let preview = response_preview(&body);
                let status_line = response
                    .reason
                    .clone()
                    .map(|reason| format!("{} {reason}", response.status_code))
                    .unwrap_or_else(|| response.status_code.to_string());
                let header_count = response.header_count.to_string();
                column![
                    detail_line("Status", status_line),
                    detail_line("Headers", header_count),
                    text("Body preview").size(14),
                    container(text(preview)).padding(10),
                ]
            } else {
                column![text("No response recorded yet").size(16)]
            }
        } else {
            column![text("Select a request to preview response").size(16)]
        };

        scrollable(container(content).padding(12)).into()
    }

    fn select_next(&mut self) {
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

    fn select_prev(&mut self) {
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

    fn apply_layout(&mut self, layout: PaneLayout) {
        self.panes = pane_grid::State::with_configuration(layout.to_configuration());
    }

    fn handle_pane_drag(&mut self, event: pane_grid::DragEvent) -> Option<PaneLayout> {
        match event {
            pane_grid::DragEvent::Dropped { pane, target } => {
                self.panes.drop(pane, target);
                Some(self.snapshot_layout())
            }
            pane_grid::DragEvent::Picked { .. } => None,
            pane_grid::DragEvent::Canceled { .. } => None,
        }
    }

    fn handle_pane_resize(&mut self, event: pane_grid::ResizeEvent) -> Option<PaneLayout> {
        self.panes.resize(event.split, event.ratio);
        Some(self.snapshot_layout())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum PaneKind {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineItem {
    id: i64,
    source: String,
    method: String,
    host: String,
    path: String,
    url: String,
    started_at: String,
    duration_ms: Option<i64>,
    request_body_size: usize,
    request_body_truncated: bool,
    completed_at: Option<String>,
    http_version: String,
    scope_status_at_capture: String,
    scope_status_current: Option<String>,
}

impl From<TimelineRequestSummary> for TimelineItem {
    fn from(value: TimelineRequestSummary) -> Self {
        Self {
            id: value.id,
            source: value.source,
            method: value.method,
            host: value.host,
            path: value.path,
            url: value.url,
            started_at: value.started_at,
            duration_ms: value.duration_ms,
            request_body_size: value.request_body_size,
            request_body_truncated: value.request_body_truncated,
            completed_at: value.completed_at,
            http_version: value.http_version,
            scope_status_at_capture: value.scope_status_at_capture,
            scope_status_current: value.scope_status_current,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuiConfig {
    last_project: Option<PathBuf>,
    window_width: f32,
    window_height: f32,
    pane_layout: Option<PaneLayout>,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            last_project: None,
            window_width: 1200.0,
            window_height: 800.0,
            pane_layout: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaneLayout {
    root: LayoutNode,
}

impl PaneLayout {
    fn from(state: &pane_grid::State<PaneKind>) -> Self {
        Self {
            root: LayoutNode::from(state.layout(), state),
        }
    }

    fn to_configuration(&self) -> pane_grid::Configuration<PaneKind> {
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

fn gui_config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("crossfeed").join(CONFIG_FILENAME)
}

async fn load_gui_config(path: PathBuf) -> Result<GuiConfig, String> {
    if !path.exists() {
        return Ok(GuiConfig::default());
    }
    let contents = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    toml::from_str(&contents).map_err(|err| err.to_string())
}

async fn save_gui_config(path: PathBuf, config: GuiConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let raw = toml::to_string_pretty(&config).map_err(|err| err.to_string())?;
    std::fs::write(path, raw).map_err(|err| err.to_string())
}

async fn open_project(path: PathBuf, intent: ProjectIntent) -> Result<TimelineState, String> {
    let layout = ProjectLayout::default();
    let paths = ProjectPaths::new(&path, &layout);

    if intent == ProjectIntent::Create {
        ensure_dir(&paths.root)?;
        ensure_dir(&paths.exports_dir)?;
        ensure_dir(&paths.logs_dir)?;
        ProjectConfig::load_or_create(&paths.config)?;
        SqliteStore::open(&paths.database)?;
    } else if !paths.root.exists() {
        return Err("Project directory does not exist".to_string());
    }

    TimelineState::new(paths.root.clone(), paths.database.clone())
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|err| err.to_string())
}

fn timeline_row(
    item: &TimelineItem,
    status: Option<u16>,
    tags: &[String],
    selected: bool,
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
            badge(item.method.clone()),
            badge(status_text.clone()),
            text(info).size(14),
        ]
        .spacing(8),
        row![
            text(format!("{} • {}", duration, body_size)).size(12),
            text(tag_label).size(12),
        ]
        .spacing(8),
    ]
    .spacing(4);

    button(row)
        .padding(10)
        .width(Length::Fill)
        .style(move |theme: &Theme, status| {
            let palette = theme.extended_palette();
            let base = if selected {
                iced::widget::button::primary(theme, status)
            } else {
                iced::widget::button::secondary(theme, status)
            };
            base.with_background(palette.background.weak.color)
        })
}

fn badge(label: String) -> Element<'static, Message> {
    container(text(label).size(12))
        .padding(6)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            iced::widget::container::Style {
                text_color: Some(palette.primary.base.text),
                background: Some(palette.primary.weak.color.into()),
                border: iced::border::Border {
                    color: palette.primary.strong.color,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                shadow: iced::Shadow::default(),
            }
        })
        .into()
}

fn detail_line(label: &'static str, value: impl Into<String>) -> Element<'static, Message> {
    let value = value.into();
    row![text(label).size(12), text(value).size(14)]
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

fn response_preview(body: &[u8]) -> String {
    if body.is_empty() {
        return "(empty body)".to_string();
    }
    match std::str::from_utf8(body) {
        Ok(text) => text.chars().take(400).collect(),
        Err(_) => format!("binary ({} bytes)", body.len()),
    }
}
