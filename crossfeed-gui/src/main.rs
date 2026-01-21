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
    PaneGrid, Space, button, column, container, pane_grid, row, scrollable, stack, text,
    text_input, tooltip,
};
use iced::{Alignment, Background, Color, Element, Length, Subscription, Task, Theme};
use serde::{Deserialize, Serialize};

const APP_NAME: &str = "Crossfeed";
const CONFIG_FILENAME: &str = "gui.toml";
const THEME_FILENAME: &str = "theme.toml";
const MENU_HEIGHT: f32 = 36.0;
const MENU_BUTTON_WIDTH: f32 = 96.0;
const MENU_SPACING: f32 = 8.0;
const MENU_PADDING_X: f32 = 8.0;
const MENU_PADDING_Y: f32 = 6.0;

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
    ToggleMenu(MenuKind),
    LoadedTheme(Result<ThemeConfig, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusArea {
    Timeline,
    Detail,
    Response,
    ProjectPicker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    File,
    Edit,
    View,
    Help,
}

#[derive(Debug, Clone)]
struct MenuItem {
    label: &'static str,
    message: Option<Message>,
    enabled: bool,
    tooltip: Option<String>,
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
    active_menu: Option<MenuKind>,
    theme: ThemePalette,
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
        let config_task = Task::perform(load_gui_config(config_path.clone()), Message::LoadedConfig);
        let theme_task = Task::perform(load_theme_config(theme_config_path()), Message::LoadedTheme);
        (
            Self {
                screen: Screen::ProjectPicker(ProjectPickerState::default()),
                config: GuiConfig::default(),
                focus: FocusArea::ProjectPicker,
                proxy_state: ProxyRuntimeState::default(),
                active_menu: None,
                theme: ThemePalette::from_config(ThemeConfig::default()),
            },
            Task::batch([config_task, theme_task]),
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
            Message::LoadedTheme(result) => {
                if let Ok(theme) = result {
                    self.theme = ThemePalette::from_config(theme);
                }
                Task::none()
            }
            Message::OpenProjectRequested | Message::CreateProjectRequested => {
                self.active_menu = None;
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
                    self.active_menu = None;
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
                self.active_menu = None;
                if let Screen::Timeline(state) = &self.screen {
                    self.screen = Screen::ProjectSettings(ProjectSettingsState::from(state));
                    self.focus = FocusArea::ProjectPicker;
                }
                Task::none()
            }
            Message::CloseProjectSettings => {
                self.active_menu = None;
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
            Message::ToggleMenu(menu) => {
                if self.active_menu == Some(menu) {
                    self.active_menu = None;
                } else {
                    self.active_menu = Some(menu);
                }
                Task::none()
            }
            Message::KeyPressed(key, modifiers) => self.handle_key(key, modifiers),
        }
    }

    fn handle_key(&mut self, key: keyboard::Key, modifiers: Modifiers) -> Task<Message> {
        if modifiers.alt() {
            if let Key::Character(ch) = &key {
                let menu = match ch.to_ascii_lowercase().as_str() {
                    "f" => Some(MenuKind::File),
                    "e" => Some(MenuKind::Edit),
                    "v" => Some(MenuKind::View),
                    "h" => Some(MenuKind::Help),
                    _ => None,
                };
                if let Some(menu) = menu {
                    self.active_menu = Some(menu);
                    return Task::none();
                }
            }
        }
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
                if self.active_menu.is_some() {
                    self.active_menu = None;
                    return Task::none();
                }
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
            Screen::ProjectPicker(picker) => picker.view(&self.theme),
            Screen::Timeline(state) => self.wrap_with_menu(
                state.view(self.focus, &self.theme),
            ),
            Screen::ProjectSettings(settings) => self.wrap_with_menu(settings.view(&self.theme)),
        }
    }

    fn wrap_with_menu<'a>(&'a self, content: Element<'a, Message>) -> Element<'a, Message> {
        let base = container(column![self.menu_view(), content])
            .width(Length::Fill)
            .height(Length::Fill)
            .style({
                let theme = self.theme;
                move |_| background_style(theme)
            });
        if let Some(overlay) = self.menu_overlay() {
            let base: Element<'a, Message> = base.into();
            stack![base, overlay].into()
        } else {
            base.into()
        }
    }

    fn menu_view<'a>(&'a self) -> Element<'a, Message> {
        let address = format!(
            "{}:{}",
            self.proxy_state.listen_host, self.proxy_state.listen_port
        );
        let proxy_label = match &self.proxy_state.status {
            ProxyStatus::Running => format!("Proxy: running on {address}"),
            ProxyStatus::Starting => format!("Proxy: starting on {address}"),
            ProxyStatus::Stopped => format!("Proxy: stopped ({address})"),
            ProxyStatus::Error(_) => format!("Proxy: error ({address})"),
        };
        let proxy_text = match self.proxy_state.status {
            ProxyStatus::Error(_) => text_danger(proxy_label, 12, self.theme),
            _ => text_muted(proxy_label, 12, self.theme),
        };
        let menu_row = row![
            row![
                self.menu_button("File", MenuKind::File),
                self.menu_button("Edit", MenuKind::Edit),
                self.menu_button("View", MenuKind::View),
                self.menu_button("Help", MenuKind::Help),
            ]
            .spacing(MENU_SPACING),
            Space::new(Length::Fill, Length::Shrink),
            container(proxy_text).align_x(Alignment::End)
        ]
        .spacing(MENU_SPACING)
        .align_y(Alignment::Center);

        container(menu_row)
            .width(Length::Fill)
            .height(Length::Fixed(MENU_HEIGHT))
            .padding([MENU_PADDING_Y, MENU_PADDING_X])
            .style({
                let theme = self.theme;
                move |_| menu_bar_style(theme)
            })
            .into()
    }

    fn menu_overlay<'a>(&'a self) -> Option<Element<'a, Message>> {
        let menu = self.active_menu?;
        let offset = menu_offset(menu);
        let panel = match menu {
            MenuKind::File => menu_panel(
                vec![
                    MenuItem {
                        label: "Open Project...",
                        message: Some(Message::OpenProjectRequested),
                        enabled: true,
                        tooltip: None,
                    },
                    MenuItem {
                        label: "New Project...",
                        message: Some(Message::CreateProjectRequested),
                        enabled: true,
                        tooltip: None,
                    },
                ],
                &self.theme,
            ),
            MenuKind::Edit => {
                let retry_enabled = matches!(self.proxy_state.status, ProxyStatus::Error(_));
                let retry_tooltip = match &self.proxy_state.status {
                    ProxyStatus::Error(err) => Some(format!("Proxy error: {err}")),
                    ProxyStatus::Running => Some("Proxy is running".to_string()),
                    ProxyStatus::Starting => Some("Proxy is starting".to_string()),
                    ProxyStatus::Stopped => Some("Proxy is stopped".to_string()),
                };
                menu_panel(
                    vec![
                        MenuItem {
                            label: "Retry Proxy",
                            message: retry_enabled.then_some(Message::RetryProxyStart),
                            enabled: retry_enabled,
                            tooltip: retry_tooltip,
                        },
                        MenuItem {
                            label: "Proxy Settings...",
                            message: Some(Message::ShowProjectSettings),
                            enabled: true,
                            tooltip: None,
                        },
                    ],
                    &self.theme,
                )
            }
            MenuKind::View | MenuKind::Help => menu_panel_text("No actions yet", &self.theme),
        };

        let overlay = container(column![
            Space::new(Length::Shrink, Length::Fixed(MENU_HEIGHT)),
            row![
                Space::new(Length::Fixed(offset), Length::Shrink),
                panel
            ]
            .align_y(Alignment::Start)
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .align_y(Alignment::Start);

        Some(overlay.into())
    }

    fn menu_button<'a>(&'a self, label: &'static str, menu: MenuKind) -> Element<'a, Message> {
        menu_action_button(label, menu, self.active_menu == Some(menu), self.theme).into()
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
        Theme::Light
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
    fn view(&self, theme: &ThemePalette) -> Element<'_, Message> {
        let title = match self.intent {
            ProjectIntent::Open => "Open Crossfeed Project",
            ProjectIntent::Create => "Create Crossfeed Project",
        };
        let action_label = match self.intent {
            ProjectIntent::Open => "Open project",
            ProjectIntent::Create => "Create project",
        };
        let mut content = column![
            text_primary(title, 28, *theme),
            text_muted("Enter the project directory path.", 14, *theme),
            text_input("/path/to/project", &self.pending_path)
                .on_input(Message::ProjectPathChanged)
                .padding(10)
                .size(16)
                .style({
                    let theme = *theme;
                    move |_theme, status| text_input_style(theme, status)
                }),
            row![
                action_button(action_label, Message::ConfirmProject, *theme),
                action_button("Cancel", Message::CancelProject, *theme),
            ]
            .spacing(12),
            row![
                action_button("Open existing", Message::OpenProjectRequested, *theme),
                action_button("Create new", Message::CreateProjectRequested, *theme),
            ]
            .spacing(12),
        ]
        .spacing(16)
        .align_x(Alignment::Start);

        if let Some(error) = &self.error {
            content = content.push(text_danger(error, 14, *theme));
        }

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(40)
            .style({
                let theme = *theme;
                move |_| background_style(theme)
            })
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

    fn view(&self, theme: &ThemePalette) -> Element<'_, Message> {
        let content = column![
            text_primary("Project Settings", 28, *theme),
            text_muted("Proxy host", 14, *theme),
            text_input("127.0.0.1", &self.proxy_host)
                .on_input(Message::UpdateProxyHost)
                .padding(8)
                .style({
                    let theme = *theme;
                    move |_theme, status| text_input_style(theme, status)
                }),
            text_muted("Proxy port", 14, *theme),
            text_input("8888", &self.proxy_port)
                .on_input(Message::UpdateProxyPort)
                .padding(8)
                .style({
                    let theme = *theme;
                    move |_theme, status| text_input_style(theme, status)
                }),
            row![
                action_button("Save", Message::SaveProjectSettings, *theme),
                action_button("Close", Message::CloseProjectSettings, *theme),
            ]
            .spacing(12),
        ]
        .spacing(16)
        .align_x(Alignment::Start);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(40)
            .style({
                let theme = *theme;
                move |_| background_style(theme)
            })
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

    fn view(
        &self,
        focus: FocusArea,
        theme: &ThemePalette,
    ) -> Element<'_, Message> {
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
                        move |_| menu_bar_style(theme)
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

    fn timeline_view(&self, _focus: FocusArea, theme: ThemePalette) -> Element<'_, Message> {
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

    fn detail_view(&self, _focus: FocusArea, theme: ThemePalette) -> Element<'_, Message> {
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

    fn response_view(&self, _focus: FocusArea, theme: ThemePalette) -> Element<'_, Message> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThemeConfig {
    background: String,
    surface: String,
    header: String,
    text: String,
    muted_text: String,
    border: String,
    accent: String,
    danger: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            background: "#282828".to_string(),
            surface: "#3c3836".to_string(),
            header: "#504945".to_string(),
            text: "#ebdbb2".to_string(),
            muted_text: "#bdae93".to_string(),
            border: "#665c54".to_string(),
            accent: "#d79921".to_string(),
            danger: "#cc241d".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ThemePalette {
    background: Color,
    surface: Color,
    header: Color,
    text: Color,
    muted_text: Color,
    border: Color,
    accent: Color,
    danger: Color,
}

impl ThemePalette {
    fn from_config(config: ThemeConfig) -> Self {
        let fallback = ThemePalette::default();
        Self {
            background: parse_hex_color(&config.background, fallback.background),
            surface: parse_hex_color(&config.surface, fallback.surface),
            header: parse_hex_color(&config.header, fallback.header),
            text: parse_hex_color(&config.text, fallback.text),
            muted_text: parse_hex_color(&config.muted_text, fallback.muted_text),
            border: parse_hex_color(&config.border, fallback.border),
            accent: parse_hex_color(&config.accent, fallback.accent),
            danger: parse_hex_color(&config.danger, fallback.danger),
        }
    }
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self {
            background: Color::from_rgb8(0x28, 0x28, 0x28),
            surface: Color::from_rgb8(0x3c, 0x38, 0x36),
            header: Color::from_rgb8(0x50, 0x49, 0x45),
            text: Color::from_rgb8(0xeb, 0xdb, 0xb2),
            muted_text: Color::from_rgb8(0xbd, 0xae, 0x93),
            border: Color::from_rgb8(0x66, 0x5c, 0x54),
            accent: Color::from_rgb8(0xd7, 0x99, 0x21),
            danger: Color::from_rgb8(0xcc, 0x24, 0x1d),
        }
    }
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

fn theme_config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("crossfeed").join(THEME_FILENAME)
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

async fn load_theme_config(path: PathBuf) -> Result<ThemeConfig, String> {
    if !path.exists() {
        let default_theme = ThemeConfig::default();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let raw = toml::to_string_pretty(&default_theme).map_err(|err| err.to_string())?;
        std::fs::write(path, raw).map_err(|err| err.to_string())?;
        return Ok(default_theme);
    }
    let contents = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    toml::from_str(&contents).map_err(|err| err.to_string())
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

fn menu_offset(menu: MenuKind) -> f32 {
    let index = match menu {
        MenuKind::File => 0.0,
        MenuKind::Edit => 1.0,
        MenuKind::View => 2.0,
        MenuKind::Help => 3.0,
    };
    MENU_PADDING_X + index * (MENU_BUTTON_WIDTH + MENU_SPACING)
}

fn parse_hex_color(value: &str, fallback: Color) -> Color {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 && value.len() != 8 {
        return fallback;
    }
    let parse_pair = |slice: &str| u8::from_str_radix(slice, 16).ok();
    let r = parse_pair(&value[0..2]);
    let g = parse_pair(&value[2..4]);
    let b = parse_pair(&value[4..6]);
    match (r, g, b) {
        (Some(r), Some(g), Some(b)) => {
            if value.len() == 8 {
                let a = parse_pair(&value[6..8]).unwrap_or(255);
                Color::from_rgba8(r, g, b, f32::from(a) / 255.0)
            } else {
                Color::from_rgb8(r, g, b)
            }
        }
        _ => fallback,
    }
}

fn menu_bar_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(theme.header)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

fn menu_button_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
    active: bool,
) -> iced::widget::button::Style {
    let base = if active { theme.header } else { theme.surface };
    let background = match status {
        iced::widget::button::Status::Hovered => theme.header,
        iced::widget::button::Status::Pressed => theme.accent,
        _ => base,
    };
    iced::widget::button::Style {
        text_color: theme.text,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

fn menu_panel_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(theme.text),
        background: Some(Background::Color(theme.surface)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: theme.border,
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
    }
}

fn menu_item_button_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
    enabled: bool,
) -> iced::widget::button::Style {
    let background = if !enabled {
        theme.surface
    } else {
        match status {
            iced::widget::button::Status::Hovered => theme.header,
            iced::widget::button::Status::Pressed => theme.accent,
            _ => theme.surface,
        }
    };
    let text_color = if enabled { theme.text } else { theme.muted_text };
    iced::widget::button::Style {
        text_color,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

fn pane_border_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(theme.surface)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
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

fn detail_line(
    label: &'static str,
    value: impl Into<String>,
    theme: ThemePalette,
) -> Element<'static, Message> {
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

fn menu_panel(items: Vec<MenuItem>, theme: &ThemePalette) -> Element<'static, Message> {
    let mut content = iced::widget::Column::new().spacing(6);
    for item in items {
        let theme = *theme;
        let label_color = if item.enabled {
            theme.text
        } else {
            theme.muted_text
        };
        let label = text(item.label).size(12).color(label_color);
        let mut button = button(label)
            .padding([4, 10])
            .width(Length::Fill)
            .style(move |_theme, status| menu_item_button_style(theme, status, item.enabled));
        if let Some(message) = item.message.clone() {
            button = button.on_press(message);
        }
        let element: Element<'static, Message> = if let Some(tooltip_text) = item.tooltip.clone() {
            let tooltip_label = container(text(tooltip_text).size(12).color(theme.text))
                .padding(6)
                .style({
                    let theme = theme;
                    move |_| menu_panel_style(theme)
                });
            tooltip(button, tooltip_label, iced::widget::tooltip::Position::Bottom).into()
        } else {
            button.into()
        };
        content = content.push(element);
    }
    container(content)
        .padding(8)
        .width(Length::Fixed(200.0))
        .style({
            let theme = *theme;
            move |_| menu_panel_style(theme)
        })
        .into()
}

fn menu_panel_text(text_value: &'static str, theme: &ThemePalette) -> Element<'static, Message> {
    let label = text(text_value).size(12).style({
        let theme = *theme;
        move |_theme: &Theme| iced::widget::text::Style {
            color: Some(theme.muted_text),
        }
    });
    container(label)
        .padding(8)
        .width(Length::Fixed(200.0))
        .style({
            let theme = *theme;
            move |_| menu_panel_style(theme)
        })
        .into()
}

fn text_primary(value: impl Into<String>, size: u16, theme: ThemePalette) -> iced::widget::Text<'static> {
    text(value.into()).size(size).style(move |_theme: &Theme| {
        iced::widget::text::Style {
            color: Some(theme.text),
        }
    })
}

fn text_muted(value: impl Into<String>, size: u16, theme: ThemePalette) -> iced::widget::Text<'static> {
    text(value.into()).size(size).style(move |_theme: &Theme| {
        iced::widget::text::Style {
            color: Some(theme.muted_text),
        }
    })
}

fn text_danger(value: impl Into<String>, size: u16, theme: ThemePalette) -> iced::widget::Text<'static> {
    text(value.into()).size(size).style(move |_theme: &Theme| {
        iced::widget::text::Style {
            color: Some(theme.danger),
        }
    })
}

fn action_button(
    label: &str,
    message: Message,
    theme: ThemePalette,
) -> iced::widget::Button<'static, Message> {
    button(text_primary(label, 12, theme))
        .on_press(message)
        .style(move |_theme, status| action_button_style(theme, status))
}

fn menu_action_button(
    label: &'static str,
    menu: MenuKind,
    active: bool,
    theme: ThemePalette,
) -> iced::widget::Button<'static, Message> {
    let label = container(text(label).size(14).color(theme.text))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    button(label)
        .on_press(Message::ToggleMenu(menu))
        .width(Length::Fixed(MENU_BUTTON_WIDTH))
        .height(Length::Fixed(MENU_HEIGHT - 2.0 * MENU_PADDING_Y))
        .padding(0)
        .style(move |_theme, status| menu_button_style(theme, status, active))
}

fn text_input_style(theme: ThemePalette, status: iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    let border_color = match status {
        iced::widget::text_input::Status::Focused => theme.accent,
        iced::widget::text_input::Status::Hovered => theme.border,
        iced::widget::text_input::Status::Disabled => theme.border,
        iced::widget::text_input::Status::Active => theme.border,
    };
    iced::widget::text_input::Style {
        background: Background::Color(theme.surface),
        border: iced::border::Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        icon: theme.muted_text,
        placeholder: theme.muted_text,
        value: theme.text,
        selection: theme.accent,
    }
}

fn background_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(theme.background)),
        border: iced::border::Border {
            color: theme.border,
            width: 0.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

fn action_button_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = match status {
        iced::widget::button::Status::Hovered => theme.header,
        iced::widget::button::Status::Pressed => theme.accent,
        _ => theme.surface,
    };
    iced::widget::button::Style {
        text_color: theme.text,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

fn timeline_row_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
    selected: bool,
) -> iced::widget::button::Style {
    let base = if selected { theme.header } else { theme.surface };
    let background = match status {
        iced::widget::button::Status::Hovered => theme.header,
        iced::widget::button::Status::Pressed => theme.accent,
        _ => base,
    };
    iced::widget::button::Style {
        text_color: theme.text,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

fn badge_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(theme.text),
        background: Some(Background::Color(theme.header)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}
