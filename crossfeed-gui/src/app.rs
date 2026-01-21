use std::path::PathBuf;

use crossfeed_ingest::{
    ProjectContext, ProxyRuntimeConfig, TailCursor, TailUpdate,
    open_or_create_project, start_proxy, tail_query,
};
use crossfeed_storage::{ProjectConfig, ProjectPaths};
use iced::event;
use iced::mouse;
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::{
    Space, column, container, mouse_area, pane_grid, row, stack, text, text_input,
};
use iced::{Alignment, Element, Length, Point, Subscription, Task, Theme};
use serde::{Deserialize, Serialize};

use crate::menu::{
    MENU_HEIGHT, MENU_PADDING_X, MENU_PADDING_Y, MENU_SPACING, MenuItem, MenuKind,
    menu_action_button, menu_offset, menu_panel, menu_panel_text,
};
use crate::project_picker::ProjectPickerState;
use crate::project_settings::ProjectSettingsState;
use crate::theme::{
    ThemeConfig, ThemePalette, action_button, background_style, load_theme_config, menu_bar_style,
    menu_item_button_style, menu_panel_style, tab_button_style, text_danger, text_input_style,
    text_muted, text_primary, theme_config_path,
};
use crate::timeline::{PaneLayout, TimelineState};
use crate::timeline::default_pane_layout;

pub const APP_NAME: &str = "Crossfeed";
const CONFIG_FILENAME: &str = "gui.toml";
const TAB_BAR_PADDING_X: f32 = 8.0;
const TAB_BAR_PADDING_Y: f32 = 6.0;
const TAB_BAR_SPACING: f32 = 8.0;
const TAB_BUTTON_PADDING_X: f32 = 10.0;
const TAB_BUTTON_PADDING_Y: f32 = 4.0;
const TAB_CHAR_WIDTH: f32 = 7.5;
const VIEW_SUBMENU_GAP: f32 = 6.0;

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
    OpenNewTabPrompt,
    UpdateNewTabLabel(String),
    OpenRenameTabPrompt(String),
    ConfirmTabPrompt,
    CancelTabPrompt,
    SaveTabsAndLayouts,
    AddDefaultTab(TabKind),
    OpenTabContextMenu(String),
    CloseTabContextMenu,
    DeleteTab(String),
    TabBarMoved(Point),
    TabDragStart(String),
    TabDragEnd,
    ViewTabsHover(bool),
    ViewSubmenuHover(bool),
    ViewSubmenuBridgeHover(bool),
    ViewTabsRegionExit,
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
    pub tab_prompt_label: String,
    pub tab_prompt_mode: Option<TabPromptMode>,
    pub tab_prompt_input_id: text_input::Id,
    pub tab_context_menu: Option<TabContextMenu>,
    pub last_tab_cursor: Option<Point>,
    pub dragging_tab: Option<TabDragState>,
    pub view_tabs_open: bool,
    pub view_tabs_hover: bool,
    pub view_submenu_hover: bool,
    pub view_submenu_bridge_hover: bool,
}

#[derive(Debug, Clone)]
pub enum TabPromptMode {
    New,
    Rename(String),
}

#[derive(Debug, Clone)]
pub struct TabContextMenu {
    pub tab_id: String,
    pub position: Point,
}

#[derive(Debug, Clone)]
pub struct TabDragState {
    pub tab_id: String,
    pub start_index: usize,
    pub hover_index: usize,
}

impl AppState {
    pub fn new() -> (Self, Task<Message>) {
        let config_path = gui_config_path();
        let config_task = Task::perform(load_gui_config(config_path.clone()), Message::LoadedConfig);
        let theme_task = Task::perform(load_theme_config(theme_config_path()), Message::LoadedTheme);
        let mut state = Self {
            screen: Screen::ProjectPicker(ProjectPickerState::default()),
            config: GuiConfig::default(),
            focus: FocusArea::ProjectPicker,
            proxy_state: ProxyRuntimeState::default(),
            active_menu: None,
            theme: ThemePalette::from_config(ThemeConfig::default()),
            tab_prompt_label: String::new(),
            tab_prompt_mode: None,
            tab_prompt_input_id: text_input::Id::unique(),
            tab_context_menu: None,
            last_tab_cursor: None,
            dragging_tab: None,
            view_tabs_open: false,
            view_tabs_hover: false,
            view_submenu_hover: false,
            view_submenu_bridge_hover: false,
        };
        state.ensure_tabs();
        (state, Task::batch([config_task, theme_task]))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LoadedConfig(result) => {
                if let Ok(config) = result {
                    self.config = config.clone();
                    self.ensure_tabs();
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
                    if let Some(layout) = self.timeline_tab_layout() {
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
                        if let Some(tab) = self.timeline_tab_mut() {
                            tab.layout = Some(snapshot);
                        }
                    }
                }
                Task::none()
            }
            Message::PaneResized(event) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    if let Some(snapshot) = state.handle_pane_resize(event) {
                        if let Some(tab) = self.timeline_tab_mut() {
                            tab.layout = Some(snapshot);
                        }
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
                    self.view_tabs_open = false;
                    self.view_tabs_hover = false;
                    self.view_submenu_hover = false;
                    self.view_submenu_bridge_hover = false;
                } else {
                    self.active_menu = Some(menu);
                    if menu != MenuKind::View {
                        self.view_tabs_open = false;
                        self.view_tabs_hover = false;
                        self.view_submenu_hover = false;
                        self.view_submenu_bridge_hover = false;
                    }
                }
                Task::none()
            }
            Message::OpenNewTabPrompt => {
                self.active_menu = None;
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
                self.tab_context_menu = None;
                self.tab_prompt_label.clear();
                self.tab_prompt_mode = Some(TabPromptMode::New);
                Task::batch([
                    text_input::focus(self.tab_prompt_input_id.clone()),
                    text_input::move_cursor_to_end(self.tab_prompt_input_id.clone()),
                ])
            }
            Message::UpdateNewTabLabel(label) => {
                self.tab_prompt_label = label;
                Task::none()
            }
            Message::OpenRenameTabPrompt(tab_id) => {
                self.active_menu = None;
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
                self.tab_context_menu = None;
                self.tab_prompt_label = self
                    .config
                    .tabs
                    .iter()
                    .find(|tab| tab.id == tab_id)
                    .map(|tab| tab.label.clone())
                    .unwrap_or_default();
                self.tab_prompt_mode = Some(TabPromptMode::Rename(tab_id));
                Task::batch([
                    text_input::focus(self.tab_prompt_input_id.clone()),
                    text_input::move_cursor_to_end(self.tab_prompt_input_id.clone()),
                ])
            }
            Message::ConfirmTabPrompt => self.confirm_tab_prompt(),
            Message::CancelTabPrompt => {
                self.tab_prompt_mode = None;
                Task::none()
            }
            Message::SaveTabsAndLayouts => {
                self.active_menu = None;
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
                self.save_tabs_and_layouts()
            }
            Message::AddDefaultTab(kind) => {
                self.active_menu = None;
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
                self.add_default_tab(kind)
            }
            Message::OpenTabContextMenu(tab_id) => {
                self.active_menu = None;
                if let Some(position) = self.last_tab_cursor {
                    self.tab_context_menu = Some(TabContextMenu {
                        tab_id,
                        position: tab_bar_to_window(position),
                    });
                }
                Task::none()
            }
            Message::CloseTabContextMenu => {
                self.tab_context_menu = None;
                Task::none()
            }
            Message::DeleteTab(tab_id) => {
                self.delete_tab(tab_id);
                self.tab_context_menu = None;
                Task::none()
            }
            Message::TabBarMoved(point) => {
                self.last_tab_cursor = Some(point);
                let hover = self.tab_index_for_position(point.x);
                if let (Some(drag), Some(index)) = (self.dragging_tab.as_mut(), hover) {
                    drag.hover_index = index;
                }
                Task::none()
            }
            Message::TabDragStart(tab_id) => {
                self.tab_context_menu = None;
                if let Some(index) = self.tab_index_by_id(&tab_id) {
                    self.dragging_tab = Some(TabDragState {
                        tab_id: tab_id.clone(),
                        start_index: index,
                        hover_index: index,
                    });
                    self.config.active_tab_id = Some(tab_id);
                }
                Task::none()
            }
            Message::TabDragEnd => {
                self.finish_tab_drag();
                Task::none()
            }
            Message::ViewTabsHover(hovered) => {
                self.view_tabs_hover = hovered;
                if hovered {
                    self.view_tabs_open = true;
                }
                Task::none()
            }
            Message::ViewSubmenuHover(hovered) => {
                self.view_submenu_hover = hovered;
                if hovered {
                    self.view_tabs_open = true;
                }
                Task::none()
            }
            Message::ViewSubmenuBridgeHover(hovered) => {
                self.view_submenu_bridge_hover = hovered;
                if hovered {
                    self.view_tabs_open = true;
                }
                Task::none()
            }
            Message::ViewTabsRegionExit => {
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
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

    fn save_tabs_and_layouts(&mut self) -> Task<Message> {
        let layout = match &self.screen {
            Screen::Timeline(state) => Some(state.snapshot_layout()),
            _ => None,
        };
        if let Some(tab) = self.timeline_tab_mut() {
            if let Some(layout) = layout {
                tab.layout = Some(layout);
            }
        }
        Task::perform(save_gui_config(gui_config_path(), self.config.clone()), |_| {
            Message::CancelProject
        })
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
        if self.tab_prompt_mode.is_some() {
            match key {
                Key::Named(keyboard::key::Named::Enter) => {
                    return self.confirm_tab_prompt();
                }
                Key::Named(keyboard::key::Named::Escape) => {
                    self.tab_prompt_mode = None;
                    return Task::none();
                }
                _ => {}
            }
        }
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
                if self.tab_context_menu.is_some() {
                    self.tab_context_menu = None;
                    return Task::none();
                }
                if self.active_menu.is_some() {
                    self.active_menu = None;
                    self.view_tabs_open = false;
                    self.view_tabs_hover = false;
                    self.view_submenu_hover = false;
                    self.view_submenu_bridge_hover = false;
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
            Screen::Timeline(state) => {
                let content = match self.active_tab_kind() {
                    Some(TabKind::Timeline) | None => state.view(self.focus, &self.theme),
                    Some(kind) => self.placeholder_view(kind),
                };
                self.wrap_with_menu(content)
            }
            Screen::ProjectSettings(settings) => self.wrap_with_menu(settings.view(&self.theme)),
        }
    }

    fn wrap_with_menu<'a>(&'a self, content: Element<'a, Message>) -> Element<'a, Message> {
        let base = container(column![self.menu_view(), self.tabs_view(), content])
            .width(Length::Fill)
            .height(Length::Fill)
            .style({
                let theme = self.theme;
                move |_| background_style(theme)
            });
        let base: Element<'a, Message> = base.into();
        let mut layers = vec![base];
        if let Some(overlay) = self.menu_overlay() {
            layers.push(overlay);
        }
        if let Some(context_menu) = self.tab_context_menu_overlay() {
            layers.push(context_menu);
        }
        if let Some(prompt) = self.tab_prompt_view() {
            layers.push(prompt);
        }
        stack(layers).into()
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
            MenuKind::View => self.view_menu_panel(),
            MenuKind::Help => menu_panel_text("No actions yet", &self.theme),
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

    fn view_menu_panel<'a>(&'a self) -> Element<'a, Message> {
        let tabs_hover = self.view_tabs_open;
        let save_button = iced::widget::button(text("Save Tabs & Layouts").size(12).color(self.theme.text))
            .on_press(Message::SaveTabsAndLayouts)
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });
        let tabs_label = row![
            text("Tabs").size(12).color(self.theme.text),
            Space::new(Length::Fill, Length::Shrink),
            text("▶").size(10).color(self.theme.muted_text),
        ]
        .align_y(Alignment::Center);
        let tabs_button = iced::widget::button(tabs_label)
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });
        let tabs_area = mouse_area(tabs_button)
            .on_enter(Message::ViewTabsHover(true))
            .on_exit(Message::ViewTabsHover(false))
            .interaction(mouse::Interaction::Pointer);

        let panel = container(column![save_button, tabs_area].spacing(6))
            .padding(8)
            .width(Length::Fixed(200.0))
            .style({
                let theme = self.theme;
                move |_| menu_panel_style(theme)
            });

        if !tabs_hover {
            return panel.into();
        }

        let submenu = menu_panel(
            vec![
                MenuItem {
                    label: "Add Timeline Tab",
                    message: Some(Message::AddDefaultTab(TabKind::Timeline)),
                    enabled: true,
                    tooltip: None,
                },
                MenuItem {
                    label: "Add Replay Tab",
                    message: Some(Message::AddDefaultTab(TabKind::Replay)),
                    enabled: true,
                    tooltip: None,
                },
                MenuItem {
                    label: "Add Fuzzer Tab",
                    message: Some(Message::AddDefaultTab(TabKind::Fuzzer)),
                    enabled: true,
                    tooltip: None,
                },
                MenuItem {
                    label: "Add Codec Tab",
                    message: Some(Message::AddDefaultTab(TabKind::Codec)),
                    enabled: true,
                    tooltip: None,
                },
            ],
            &self.theme,
        );
        let submenu = mouse_area(submenu)
            .on_enter(Message::ViewSubmenuHover(true))
            .on_exit(Message::ViewSubmenuHover(false))
            .interaction(mouse::Interaction::Pointer);

        let region = submenu_with_bridge(
            panel.into(),
            submenu.into(),
            VIEW_SUBMENU_GAP,
            Message::ViewSubmenuBridgeHover(true),
            Message::ViewSubmenuBridgeHover(false),
        );

        mouse_area(region)
            .on_exit(Message::ViewTabsRegionExit)
            .interaction(mouse::Interaction::Pointer)
            .into()
    }

    fn tabs_view<'a>(&'a self) -> Element<'a, Message> {
        let mut tabs_row = row![].spacing(TAB_BAR_SPACING).align_y(Alignment::Center);
        for tab in &self.config.tabs {
            let is_active = self
                .config
                .active_tab_id
                .as_deref()
                .map(|id| id == tab.id)
                .unwrap_or(false);
            let label = text(tab.label.as_str()).size(13).color(self.theme.text);
            let width = tab_button_width(tab.label.as_str());
            let button = iced::widget::button(label)
                .padding([TAB_BUTTON_PADDING_Y, TAB_BUTTON_PADDING_X])
                .width(Length::Fixed(width))
                .style(move |_theme, status| tab_button_style(self.theme, status, is_active));
            let interaction = if self
                .dragging_tab
                .as_ref()
                .map(|drag| drag.tab_id == tab.id)
                .unwrap_or(false)
            {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::Grab
            };
            let tab_area = mouse_area(button)
                .on_press(Message::TabDragStart(tab.id.clone()))
                .on_right_press(Message::OpenTabContextMenu(tab.id.clone()))
                .interaction(interaction);
            tabs_row = tabs_row.push(tab_area);
        }
        let add_label = text("+").size(14).color(self.theme.text);
        let add_button = iced::widget::button(add_label)
            .on_press(Message::OpenNewTabPrompt)
            .padding([TAB_BUTTON_PADDING_Y, TAB_BUTTON_PADDING_X])
            .style(move |_theme, status| tab_button_style(self.theme, status, false));
        tabs_row = tabs_row.push(add_button);

        let tabs_row = mouse_area(tabs_row)
            .on_move(Message::TabBarMoved)
            .on_release(Message::TabDragEnd)
            .interaction(mouse::Interaction::Pointer);

        container(tabs_row)
            .width(Length::Fill)
            .padding([TAB_BAR_PADDING_Y, TAB_BAR_PADDING_X])
            .style({
                let theme = self.theme;
                move |_| menu_bar_style(theme)
            })
            .into()
    }

    fn tab_prompt_view<'a>(&'a self) -> Option<Element<'a, Message>> {
        let mode = self.tab_prompt_mode.as_ref()?;
        let title = match mode {
            TabPromptMode::New => "New tab label",
            TabPromptMode::Rename(_) => "Rename tab",
        };
        let confirm_label = match mode {
            TabPromptMode::New => "Create",
            TabPromptMode::Rename(_) => "Save",
        };
        let prompt = container(
            column![
                text_primary(title, 16, self.theme),
                text_input("Tab name", &self.tab_prompt_label)
                    .id(self.tab_prompt_input_id.clone())
                    .on_input(Message::UpdateNewTabLabel)
                    .padding(8)
                    .style({
                        let theme = self.theme;
                        move |_theme, status| text_input_style(theme, status)
                    }),
                row![
                    action_button(confirm_label, Message::ConfirmTabPrompt, self.theme),
                    action_button("Cancel", Message::CancelTabPrompt, self.theme),
                ]
                .spacing(12),
            ]
            .spacing(12),
        )
        .padding(16)
        .style({
            let theme = self.theme;
            move |_| menu_panel_style(theme)
        });

        Some(
            container(prompt)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .into(),
        )
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

    fn ensure_tabs(&mut self) {
        if self.config.tabs.is_empty() {
            self.config.tabs = vec![
                TabConfig::with_layout("timeline", "Timeline", TabKind::Timeline),
                TabConfig::with_layout("replay", "Replay", TabKind::Replay),
                TabConfig::with_layout("fuzzer", "Fuzzer", TabKind::Fuzzer),
                TabConfig::with_layout("codec", "Codec", TabKind::Codec),
            ];
            self.config.active_tab_id = Some("timeline".to_string());
        }
        if self.config.active_tab_id.is_none() {
            self.config.active_tab_id = self.config.tabs.first().map(|tab| tab.id.clone());
        }
        if let Some(layout) = self.config.pane_layout.take() {
            if let Some(tab) = self.timeline_tab_mut() {
                if tab.layout.is_none() {
                    tab.layout = Some(layout);
                }
            }
        }
    }

    fn timeline_tab_mut(&mut self) -> Option<&mut TabConfig> {
        self.config.tabs.iter_mut().find(|tab| tab.kind == TabKind::Timeline)
    }

    fn timeline_tab_layout(&self) -> Option<PaneLayout> {
        self.config
            .tabs
            .iter()
            .find(|tab| tab.kind == TabKind::Timeline)
            .and_then(|tab| tab.layout.clone())
    }

    fn active_tab_kind(&self) -> Option<TabKind> {
        let id = self.config.active_tab_id.as_deref()?;
        self.config.tabs.iter().find(|tab| tab.id == id).map(|tab| tab.kind)
    }

    fn placeholder_view(&self, kind: TabKind) -> Element<'_, Message> {
        let label = match kind {
            TabKind::Replay => "Replay tab (empty)",
            TabKind::Fuzzer => "Fuzzer tab (empty)",
            TabKind::Codec => "Codec tab (empty)",
            TabKind::Custom => "Custom tab (empty)",
            TabKind::Timeline => "Timeline",
        };
        container(column![text_muted(label, 16, self.theme)])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into()
    }

    fn tab_context_menu_overlay<'a>(&'a self) -> Option<Element<'a, Message>> {
        let menu = self.tab_context_menu.as_ref()?;
        let panel = menu_panel(
            vec![
                MenuItem {
                    label: "Rename",
                    message: Some(Message::OpenRenameTabPrompt(menu.tab_id.clone())),
                    enabled: true,
                    tooltip: None,
                },
                MenuItem {
                    label: "Delete",
                    message: Some(Message::DeleteTab(menu.tab_id.clone())),
                    enabled: true,
                    tooltip: None,
                },
            ],
            &self.theme,
        );
        let background = mouse_area(container(Space::new(Length::Fill, Length::Fill)))
            .on_press(Message::CloseTabContextMenu)
            .on_right_press(Message::CloseTabContextMenu)
            .interaction(mouse::Interaction::Pointer);
        let overlay = stack(vec![
            background.into(),
            container(column![
                Space::new(Length::Shrink, Length::Fixed(menu.position.y)),
                row![
                    Space::new(Length::Fixed(menu.position.x), Length::Shrink),
                    panel
                ]
                .align_y(Alignment::Start)
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Start)
            .align_y(Alignment::Start)
            .into(),
        ]);

        Some(container(overlay).width(Length::Fill).height(Length::Fill).into())
    }

    fn confirm_tab_prompt(&mut self) -> Task<Message> {
        let label = self.tab_prompt_label.trim();
        let Some(mode) = self.tab_prompt_mode.clone() else {
            return Task::none();
        };
        if label.is_empty() {
            return Task::none();
        }
        self.tab_prompt_mode = None;
        match mode {
            TabPromptMode::New => {
                let id = format!("custom-{}", self.config.tabs.len() + 1);
                self.config.tabs.push(TabConfig {
                    id: id.clone(),
                    label: label.to_string(),
                    kind: TabKind::Custom,
                    layout: None,
                });
                self.config.active_tab_id = Some(id);
            }
            TabPromptMode::Rename(tab_id) => {
                if let Some(tab) = self.config.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    tab.label = label.to_string();
                }
            }
        }
        Task::none()
    }

    fn add_default_tab(&mut self, kind: TabKind) -> Task<Message> {
        let id = format!("{}-{}", kind.as_str(), self.config.tabs.len() + 1);
        let label = kind.default_label();
        let layout = default_layout_for(kind);
        self.config.tabs.push(TabConfig {
            id: id.clone(),
            label: label.to_string(),
            kind,
            layout,
        });
        self.config.active_tab_id = Some(id);
        Task::none()
    }

    fn delete_tab(&mut self, tab_id: String) {
        if let Some(index) = self.tab_index_by_id(&tab_id) {
            self.config.tabs.remove(index);
            if self.config.active_tab_id.as_deref() == Some(&tab_id) {
                let next_id = self
                    .config
                    .tabs
                    .get(index)
                    .or_else(|| self.config.tabs.get(index.saturating_sub(1)))
                    .map(|tab| tab.id.clone());
                self.config.active_tab_id = next_id;
            }
            if self.config.tabs.is_empty() {
                self.ensure_tabs();
            }
        }
    }

    fn finish_tab_drag(&mut self) {
        let Some(drag) = self.dragging_tab.take() else {
            return;
        };
        if drag.start_index == drag.hover_index || self.config.tabs.len() <= 1 {
            return;
        }
        let mut target = drag.hover_index.min(self.config.tabs.len() - 1);
        if drag.start_index < target {
            target = target.saturating_sub(1);
        }
        let tab = self.config.tabs.remove(drag.start_index);
        self.config.tabs.insert(target, tab);
    }

    fn tab_index_by_id(&self, tab_id: &str) -> Option<usize> {
        self.config.tabs.iter().position(|tab| tab.id == tab_id)
    }

    fn tab_index_for_position(&self, x: f32) -> Option<usize> {
        let mut cursor = 0.0;
        for (index, tab) in self.config.tabs.iter().enumerate() {
            let width = tab_button_width(tab.label.as_str());
            if x <= cursor + width * 0.5 {
                return Some(index);
            }
            cursor += width + TAB_BAR_SPACING;
        }
        if self.config.tabs.is_empty() {
            None
        } else {
            Some(self.config.tabs.len() - 1)
        }
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
    pub tabs: Vec<TabConfig>,
    pub active_tab_id: Option<String>,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            last_project: None,
            window_width: 1200.0,
            window_height: 800.0,
            pane_layout: None,
            tabs: Vec::new(),
            active_tab_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabKind {
    Timeline,
    Replay,
    Fuzzer,
    Codec,
    Custom,
}

impl TabKind {
    fn as_str(self) -> &'static str {
        match self {
            TabKind::Timeline => "timeline",
            TabKind::Replay => "replay",
            TabKind::Fuzzer => "fuzzer",
            TabKind::Codec => "codec",
            TabKind::Custom => "custom",
        }
    }

    fn default_label(self) -> &'static str {
        match self {
            TabKind::Timeline => "Timeline",
            TabKind::Replay => "Replay",
            TabKind::Fuzzer => "Fuzzer",
            TabKind::Codec => "Codec",
            TabKind::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabConfig {
    pub id: String,
    pub label: String,
    pub kind: TabKind,
    pub layout: Option<PaneLayout>,
}

impl TabConfig {
    fn with_layout(id: &str, label: &str, kind: TabKind) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            kind,
            layout: default_layout_for(kind),
        }
    }
}

fn tab_button_width(label: &str) -> f32 {
    let text_width = label.chars().count() as f32 * TAB_CHAR_WIDTH;
    text_width + TAB_BUTTON_PADDING_X * 2.0
}

fn default_layout_for(kind: TabKind) -> Option<PaneLayout> {
    match kind {
        TabKind::Timeline => Some(default_pane_layout()),
        _ => None,
    }
}

fn tab_bar_to_window(point: Point) -> Point {
    Point::new(
        point.x + TAB_BAR_PADDING_X,
        point.y + MENU_HEIGHT + TAB_BAR_PADDING_Y,
    )
}

fn submenu_with_bridge<'a>(
    panel: Element<'a, Message>,
    submenu: Element<'a, Message>,
    bridge_width: f32,
    on_enter: Message,
    on_exit: Message,
) -> Element<'a, Message> {
    let bridge = mouse_area(container(Space::new(
        Length::Fixed(bridge_width),
        Length::Shrink,
    )))
    .on_enter(on_enter)
    .on_exit(on_exit)
    .interaction(mouse::Interaction::Pointer);

    row![panel, bridge, submenu]
        .spacing(0)
        .align_y(Alignment::Start)
        .into()
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
