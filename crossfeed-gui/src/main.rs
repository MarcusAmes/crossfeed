use std::collections::HashMap;
use std::path::PathBuf;

use crossfeed_codec::{deflate_decompress, gzip_decompress};
use crossfeed_ingest::{
    ProjectContext, ProxyRuntimeConfig, TailCursor, TailUpdate, TimelineItem,
    open_or_create_project, start_proxy, tail_query,
};
use crossfeed_storage::{
    ProjectConfig, ProjectPaths, ResponseSummary, SqliteStore, TimelineQuery, TimelineSort,
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
    ProjectSettings(ProjectSettingsState),
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
    ShowProjectSettings,
    SaveProjectSettings,
    CloseProjectSettings,
    UpdateProxyHost(String),
    UpdateProxyPort(String),
    RetryProxyStart,
    TailTick,
    TailLoaded(Result<TailUpdate, String>),
    ProxyStarted(Result<(), String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusArea {
    Timeline,
    Detail,
    Response,
    ProjectPicker,
}

#[derive(Debug, Clone)]
struct ProxyRuntimeState {
    status: ProxyStatus,
    listen_host: String,
    listen_port: u16,
}

impl ProxyRuntimeState {
    fn new(config: &ProjectConfig) -> Self {
        Self {
            status: ProxyStatus::Stopped,
            listen_host: config.proxy.listen_host.clone(),
            listen_port: config.proxy.listen_port,
        }
    }
}

impl Default for ProxyRuntimeState {
    fn default() -> Self {
        Self {
            status: ProxyStatus::Stopped,
            listen_host: "127.0.0.1".to_string(),
            listen_port: 8888,
        }
    }
}

#[derive(Debug, Clone)]
enum ProxyStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

#[derive(Debug, Clone)]
struct AppState {
    screen: Screen,
    config: GuiConfig,
    focus: FocusArea,
    proxy_state: ProxyRuntimeState,
}

impl AppState {
    fn save_project_settings(&mut self) -> Task<Message> {
        let Screen::ProjectSettings(settings) = &self.screen else {
            return Task::none();
        };
        let project_paths = settings.project_paths.clone();
        let proxy_host = settings.proxy_host.clone();
        let proxy_port = settings.proxy_port.clone();
        let mut updated = settings.project_config.clone();
        let mut timeline = settings.timeline_state.clone();

        updated.proxy.listen_host = proxy_host.trim().to_string();
        let port = proxy_port
            .trim()
            .parse::<u16>()
            .unwrap_or(updated.proxy.listen_port);
        updated.proxy.listen_port = port;
        timeline.project_config = updated.clone();
        if let Err(err) = updated.save(&project_paths.config) {
            return Task::perform(async move { Err(err) }, Message::ProxyStarted);
        }
        self.screen = Screen::Timeline(timeline);
        self.focus = FocusArea::Timeline;
        self.proxy_state = ProxyRuntimeState::new(&updated);
        self.proxy_state.status = ProxyStatus::Starting;
        start_proxy_runtime(project_paths, updated)
    }

    fn retry_proxy_start(&mut self) -> Task<Message> {
        let Screen::Timeline(state) = &self.screen else {
            return Task::none();
        };
        self.proxy_state = ProxyRuntimeState::new(&state.project_config);
        self.proxy_state.status = ProxyStatus::Starting;
        start_proxy_runtime(state.project_paths.clone(), state.project_config.clone())
    }

    fn tail_tick(&mut self) -> Task<Message> {
        if let Screen::Timeline(state) = &self.screen {
            let request_ids = state
                .timeline
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>();
            return Task::perform(
                tail_query_gui(
                    state.store_path.clone(),
                    state.tail_cursor.clone(),
                    request_ids,
                ),
                Message::TailLoaded,
            );
        }
        Task::none()
    }
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
                proxy_state: ProxyRuntimeState::default(),
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
                    self.proxy_state = ProxyRuntimeState::new(&timeline.project_config);
                    let proxy_task = start_proxy_runtime(
                        timeline.project_paths.clone(),
                        timeline.project_config.clone(),
                    );
                    self.proxy_state.status = ProxyStatus::Starting;
                    self.screen = Screen::Timeline(timeline);
                    Task::batch([
                        Task::perform(
                            save_gui_config(gui_config_path(), self.config.clone()),
                            |_| Message::CancelProject,
                        ),
                        proxy_task,
                    ])
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
            Message::ShowProjectSettings => {
                if let Screen::Timeline(state) = &self.screen {
                    self.screen = Screen::ProjectSettings(ProjectSettingsState::from(state));
                    self.focus = FocusArea::ProjectPicker;
                }
                Task::none()
            }
            Message::CloseProjectSettings => {
                if let Screen::ProjectSettings(settings) = &self.screen {
                    self.screen = Screen::Timeline(settings.timeline_state.clone());
                    self.focus = FocusArea::Timeline;
                }
                Task::none()
            }
            Message::SaveProjectSettings => self.save_project_settings(),
            Message::UpdateProxyHost(value) => {
                if let Screen::ProjectSettings(settings) = &mut self.screen {
                    settings.proxy_host = value;
                }
                Task::none()
            }
            Message::UpdateProxyPort(value) => {
                if let Screen::ProjectSettings(settings) = &mut self.screen {
                    settings.proxy_port = value;
                }
                Task::none()
            }
            Message::RetryProxyStart => self.retry_proxy_start(),
            Message::TailTick => self.tail_tick(),
            Message::TailLoaded(result) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    state.apply_tail_update(result);
                }
                Task::none()
            }
            Message::ProxyStarted(result) => {
                self.proxy_state.status = match result {
                    Ok(()) => ProxyStatus::Running,
                    Err(err) => ProxyStatus::Error(err),
                };
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
            Screen::Timeline(state) => state.view(self.focus, &self.proxy_state),
            Screen::ProjectSettings(settings) => settings.view(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let key_events = event::listen_with(|event, _status, _id| match event {
            event::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                Some(Message::KeyPressed(key, modifiers))
            }
            _ => None,
        });
        let ticks =
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| Message::TailTick);
        Subscription::batch([key_events, ticks])
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

#[derive(Debug, Clone)]
struct ProjectSettingsState {
    timeline_state: TimelineState,
    project_paths: ProjectPaths,
    project_config: ProjectConfig,
    proxy_host: String,
    proxy_port: String,
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

impl ProjectSettingsState {
    fn from(state: &TimelineState) -> Self {
        Self {
            timeline_state: state.clone(),
            project_paths: state.project_paths.clone(),
            project_config: state.project_config.clone(),
            proxy_host: state.project_config.proxy.listen_host.clone(),
            proxy_port: state.project_config.proxy.listen_port.to_string(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let content = column![
            text("Project Settings").size(28),
            text("Proxy host").size(14),
            text_input("127.0.0.1", &self.proxy_host)
                .on_input(Message::UpdateProxyHost)
                .padding(8),
            text("Proxy port").size(14),
            text_input("8888", &self.proxy_port)
                .on_input(Message::UpdateProxyPort)
                .padding(8),
            row![
                button("Save").on_press(Message::SaveProjectSettings),
                button("Close").on_press(Message::CloseProjectSettings),
            ]
            .spacing(12),
        ]
        .spacing(16)
        .align_x(Alignment::Start);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(40)
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
    project_paths: ProjectPaths,
    project_config: ProjectConfig,
    store_path: PathBuf,
    tags: HashMap<i64, Vec<String>>,
    responses: HashMap<i64, ResponseSummary>,
    tail_cursor: TailCursor,
}

impl TimelineState {
    fn new(project_paths: ProjectPaths, project_config: ProjectConfig) -> Result<Self, String> {
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

    fn view(&self, focus: FocusArea, proxy_state: &ProxyRuntimeState) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |_, state, _| {
            let content: Element<'_, Message> = match state {
                PaneKind::Timeline => self.timeline_view(focus, proxy_state),
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

    fn timeline_view(
        &self,
        _focus: FocusArea,
        proxy_state: &ProxyRuntimeState,
    ) -> Element<'_, Message> {
        let proxy_status = match &proxy_state.status {
            ProxyStatus::Stopped => "Proxy stopped".to_string(),
            ProxyStatus::Starting => "Proxy starting...".to_string(),
            ProxyStatus::Running => format!(
                "Proxy running on {}:{}",
                proxy_state.listen_host, proxy_state.listen_port
            ),
            ProxyStatus::Error(err) => format!("Proxy error: {err}"),
        };
        let mut header = column![
            text("Timeline").size(20),
            text(format!("Project: {}", self.project_root.display())).size(12),
            text(proxy_status).size(12),
        ]
        .spacing(6);

        if matches!(proxy_state.status, ProxyStatus::Error(_)) {
            header = header.push(
                row![
                    button("Retry proxy").on_press(Message::RetryProxyStart),
                    button("Settings").on_press(Message::ShowProjectSettings),
                ]
                .spacing(12),
            );
        } else {
            header = header.push(button("Settings").on_press(Message::ShowProjectSettings));
        }

        let mut content = column![header].spacing(12);

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
                    detail_line("Status", status_line),
                    text("Headers").size(14),
                    container(text(headers)).padding(10),
                    text(body_label).size(14),
                    container(text(body_text)).padding(10),
                ]
            } else {
                column![text("No response recorded yet").size(16)]
            }
        } else {
            column![text("Select a request to preview response").size(16)]
        };

        scrollable(container(content).padding(12)).into()
    }

    fn apply_tail_update(&mut self, update: Result<TailUpdate, String>) {
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
    if intent == ProjectIntent::Open && !path.exists() {
        return Err("Project directory does not exist".to_string());
    }
    let context = open_or_create_project(&path)?;
    TimelineState::new(context.paths, context.config)
}

fn global_certs_dir() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or("Missing config directory")?;
    Ok(base.join("crossfeed").join("certs"))
}

fn start_proxy_runtime(
    project_paths: ProjectPaths,
    project_config: ProjectConfig,
) -> Task<Message> {
    let context = ProjectContext {
        paths: project_paths.clone(),
        config: project_config.clone(),
        store_path: project_paths.database.clone(),
    };
    let certs_dir = match global_certs_dir() {
        Ok(path) => path,
        Err(err) => return Task::perform(async move { Err(err) }, Message::ProxyStarted),
    };
    let config = ProxyRuntimeConfig::from_project(&context, certs_dir);
    Task::perform(start_proxy(context, config), Message::ProxyStarted)
}

async fn tail_query_gui(
    store_path: PathBuf,
    cursor: TailCursor,
    existing_ids: Vec<i64>,
) -> Result<TailUpdate, String> {
    tail_query(store_path, cursor, existing_ids, 200).await
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
