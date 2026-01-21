use std::path::PathBuf;

use crossfeed_ingest::{
    ProjectContext, ProxyRuntimeConfig, TailCursor, TailUpdate,
    open_or_create_project, start_proxy, tail_query,
};
use crossfeed_storage::{ProjectConfig, ProjectPaths};
use iced::event;
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::{Space, column, container, pane_grid, row, stack};
use iced::{Alignment, Element, Length, Subscription, Task, Theme};
use serde::{Deserialize, Serialize};

use crate::menu::{
    MENU_HEIGHT, MENU_PADDING_X, MENU_PADDING_Y, MENU_SPACING, MenuItem, MenuKind, menu_action_button,
    menu_offset, menu_panel, menu_panel_text,
};
use crate::project_picker::ProjectPickerState;
use crate::project_settings::ProjectSettingsState;
use crate::theme::{
    ThemeConfig, ThemePalette, background_style, load_theme_config, menu_bar_style, text_danger,
    text_muted, theme_config_path,
};
use crate::timeline::{PaneLayout, TimelineState};

pub const APP_NAME: &str = "Crossfeed";
const CONFIG_FILENAME: &str = "gui.toml";

#[derive(Debug, Clone)]
pub enum Screen {
    ProjectPicker(ProjectPickerState),
    Timeline(TimelineState),
    ProjectSettings(ProjectSettingsState),
}

#[derive(Debug, Clone)]
pub enum Message {
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
pub enum FocusArea {
    Timeline,
    Detail,
    Response,
    ProjectPicker,
}

#[derive(Debug, Clone)]
pub struct ProxyRuntimeState {
    pub status: ProxyStatus,
    pub listen_host: String,
    pub listen_port: u16,
}

impl ProxyRuntimeState {
    pub fn new(config: &ProjectConfig) -> Self {
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
pub enum ProxyStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub screen: Screen,
    pub config: GuiConfig,
    pub focus: FocusArea,
    pub proxy_state: ProxyRuntimeState,
    pub active_menu: Option<MenuKind>,
    pub theme: ThemePalette,
}

impl AppState {
    pub fn new() -> (Self, Task<Message>) {
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
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

    pub fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::ProjectPicker(picker) => picker.view(&self.theme),
            Screen::Timeline(state) => self.wrap_with_menu(state.view(self.focus, &self.theme)),
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
        menu_action_button(label, Message::ToggleMenu(menu), self.active_menu == Some(menu), self.theme).into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let key_events = event::listen_with(|event, _status, _id| match event {
            event::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                Some(Message::KeyPressed(key, modifiers))
            }
            _ => None,
        });
        let ticks = iced::time::every(std::time::Duration::from_millis(500)).map(|_| Message::TailTick);
        Subscription::batch([key_events, ticks])
    }

    pub fn theme(&self) -> Theme {
        Theme::Light
    }
}

impl Default for FocusArea {
    fn default() -> Self {
        FocusArea::ProjectPicker
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectIntent {
    Open,
    Create,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiConfig {
    pub last_project: Option<PathBuf>,
    pub window_width: f32,
    pub window_height: f32,
    pub pane_layout: Option<PaneLayout>,
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
