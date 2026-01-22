use std::path::PathBuf;

use crossfeed_ingest::{
    ProjectContext, ProxyRuntimeConfig, TailCursor, TailUpdate,
    create_collection_and_add_request, create_replay_collection, create_replay_from_timeline,
    duplicate_replay_request, get_latest_replay_execution, get_latest_replay_response,
    get_replay_active_version, list_replay_collections, list_replay_requests_in_collection,
    list_replay_requests_unassigned, move_replay_request_to_collection,
    update_replay_collection_color, update_replay_collection_name, update_replay_collection_sort,
    update_replay_request_name, update_replay_request_sort,
    open_or_create_project, start_proxy, tail_query,
};
use crossfeed_storage::{ProjectConfig, ProjectPaths, SqliteStore};
use std::collections::HashMap;

use iced::event;
use iced::mouse;
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::{
    PaneGrid, Space, column, container, mouse_area, pane_grid, row, stack, text, text_input,
    text_editor,
};
use iced::{Alignment, Element, Length, Point, Subscription, Task, Theme};
use serde::{Deserialize, Serialize};

use crate::menu::{
    MENU_HEIGHT, MENU_PADDING_X, MENU_PADDING_Y, MENU_SPACING, MenuItem, MenuKind,
    menu_action_button, menu_offset, menu_panel, menu_panel_text,
};
use crate::project_picker::ProjectPickerState;
use crate::project_settings::ProjectSettingsState;
use crate::replay::{ReplayLayout, ReplayState, default_replay_layout};
use crate::theme::{
    ThemeConfig, ThemePalette, action_button, background_style, load_theme_config, menu_bar_style,
    menu_item_button_style, menu_panel_style, pane_border_style, tab_button_style, text_danger,
    text_input_style, text_muted, text_primary, theme_config_path,
};
use crate::timeline::{PaneLayout, TimelineState};
use crate::ui::panes::{
    PaneModuleKind, response_preview_from_bytes, response_preview_placeholder,
    timeline_request_details_view, timeline_request_list_view,
};
use crate::timeline::default_pane_layout;

pub const APP_NAME: &str = "Crossfeed";
const CONFIG_FILENAME: &str = "gui.toml";
const TAB_BAR_PADDING_X: f32 = 8.0;
const TAB_BAR_PADDING_Y: f32 = 6.0;
const TAB_BAR_SPACING: f32 = 8.0;
const TAB_BUTTON_PADDING_X: f32 = 10.0;
const TAB_BUTTON_PADDING_Y: f32 = 4.0;
const TAB_CHAR_WIDTH: f32 = 7.5;
const TAB_MAX_WIDTH: f32 = 200.0;
const TAB_TEXT_FUDGE: f32 = 8.0;
const VIEW_SUBMENU_GAP: f32 = 6.0;
const TAB_BAR_HEIGHT: f32 = 36.0;

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
    ReplaySelect(i64),
    ReplayUpdateDetails(text_editor::Action),
    ReplayPaneDragged(pane_grid::DragEvent),
    ReplayPaneResized(pane_grid::ResizeEvent),
    ReplayLoaded(Result<ReplayListData, String>),
    ReplayActiveVersionLoaded(Result<Option<crossfeed_storage::ReplayVersion>, String>),
    ReplayResponseLoaded(Result<Option<crossfeed_storage::TimelineResponse>, String>),
    ReplayToggleCollection(i64),
    ReplayListCursor(iced::Point),
    ReplayContextMenuOpen(i64),
    ReplayContextMenuClose,
    ReplayDuplicate(i64),
    ReplayRenamePrompt(i64),
    ReplayPromptLabel(String),
    ReplayPromptConfirm,
    ReplayPromptCancel,
    ReplayAddToCollectionMenu(bool),
    ReplayCollectionMenuHover(bool),
    ReplayCollectionMenuBridgeHover(bool),
    ReplayCollectionMenuExit,
    ReplayAddToCollection(i64),
    ReplayNewCollectionPrompt(i64),
    ReplayCreateCollection,
    ReplayCollectionMenuOpen(i64),
    ReplayCollectionMenuClose,
    ReplayCollectionRenamePrompt(i64),
    ReplayCollectionColorMenuHover(bool),
    ReplayCollectionColorBridgeHover(bool),
    ReplayCollectionColorExit,
    ReplayCollectionSetColor(i64, Option<String>),
    ReplayCreatedFromTimeline(Result<i64, String>),
    ReplayDragStart(i64, Option<i64>),
    ReplayDragHover(ReplayDropTarget),
    ReplayDragHoverClear,
    ReplayDragEnd,
    TimelineListCursor(Point),
    TimelineContextMenuOpen(i64),
    TimelineContextMenuClose,
    TimelineSendToReplay(i64),
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
    CloseMenu,
    AddPaneToTab(PaneModuleKind),
    ViewPanesHover(bool),
    ViewPanesSubmenuHover(bool),
    ViewPanesBridgeHover(bool),
    ViewPanesRegionExit,
    CustomPaneDragged(pane_grid::DragEvent),
    CustomPaneResized(pane_grid::ResizeEvent),
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

#[derive(Debug)]
pub struct AppState {
    pub screen: Screen,
    pub config: GuiConfig,
    pub focus: FocusArea,
    pub proxy_state: ProxyRuntimeState,
    pub active_menu: Option<MenuKind>,
    pub theme: ThemePalette,
    pub replay_state: ReplayState,
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
    pub view_panes_open: bool,
    pub view_panes_hover: bool,
    pub view_panes_submenu_hover: bool,
    pub view_panes_bridge_hover: bool,
    pub custom_tabs: HashMap<String, pane_grid::State<PaneModuleKind>>,
    pub timeline_list_cursor: Option<Point>,
    pub timeline_context_menu: Option<TimelineContextMenu>,
    pub replay_list_cursor: Option<Point>,
    pub replay_context_menu: Option<ReplayContextMenu>,
    pub replay_collection_context_menu: Option<ReplayCollectionContextMenu>,
    pub replay_prompt_label: String,
    pub replay_prompt_mode: Option<ReplayPromptMode>,
    pub replay_prompt_input_id: text_input::Id,
    pub replay_collection_menu_open: bool,
    pub replay_collection_hover: bool,
    pub replay_collection_menu_hover: bool,
    pub replay_collection_bridge_hover: bool,
    pub replay_collection_color_open: bool,
    pub replay_collection_color_hover: bool,
    pub replay_collection_color_bridge_hover: bool,
    pub replay_drag: Option<ReplayDragState>,
    pub replay_drag_hover: Option<ReplayDropTarget>,
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

#[derive(Debug, Clone)]
pub struct ReplayContextMenu {
    pub request_id: i64,
    pub position: Point,
}

#[derive(Debug, Clone)]
pub struct ReplayCollectionContextMenu {
    pub collection_id: i64,
    pub position: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayDropTarget {
    Request {
        request_id: i64,
        collection_id: Option<i64>,
    },
    Collection {
        collection_id: Option<i64>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct ReplayDragState {
    pub request_id: i64,
    pub source_collection_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TimelineContextMenu {
    pub request_id: i64,
    pub position: Point,
}

#[derive(Debug, Clone)]
pub enum ReplayPromptMode {
    Rename(i64),
    NewCollection(i64),
    RenameCollection(i64),
}

#[derive(Debug, Clone)]
pub struct ReplayListData {
    pub collections: Vec<crossfeed_storage::ReplayCollection>,
    pub requests_by_collection: HashMap<Option<i64>, Vec<crossfeed_storage::ReplayRequest>>,
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
            replay_state: ReplayState::default(),
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
            view_panes_open: false,
            view_panes_hover: false,
            view_panes_submenu_hover: false,
            view_panes_bridge_hover: false,
            custom_tabs: HashMap::new(),
            timeline_list_cursor: None,
            timeline_context_menu: None,
            replay_list_cursor: None,
            replay_context_menu: None,
            replay_collection_context_menu: None,
            replay_prompt_label: String::new(),
            replay_prompt_mode: None,
            replay_prompt_input_id: text_input::Id::unique(),
            replay_collection_menu_open: false,
            replay_collection_hover: false,
            replay_collection_menu_hover: false,
            replay_collection_bridge_hover: false,
            replay_collection_color_open: false,
            replay_collection_color_hover: false,
            replay_collection_color_bridge_hover: false,
            replay_drag: None,
            replay_drag_hover: None,
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
                    self.sync_active_tab_layout();
                    self.replay_state.set_store_path(self.project_store_path());
                    Task::batch([
                        Task::perform(
                            save_gui_config(gui_config_path(), self.config.clone()),
                            |_| Message::CancelProject,
                        ),
                        self.load_replay_list(),
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
                        self.set_active_tab_layout(TabLayout::Timeline(snapshot));
                    }
                }
                Task::none()
            }
            Message::PaneResized(event) => {
                if let Screen::Timeline(state) = &mut self.screen {
                    if let Some(snapshot) = state.handle_pane_resize(event) {
                        self.set_active_tab_layout(TabLayout::Timeline(snapshot));
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
            Message::ReplaySelect(index) => {
                self.replay_state.select(index);
                Task::batch([
                    self.load_replay_active_version(index),
                    self.load_replay_response(index),
                ])
            }
            Message::ReplayUpdateDetails(body) => {
                self.replay_state.apply_editor_action(body);
                Task::none()
            }
            Message::ReplayPaneDragged(event) => {
                if let Some(layout) = self.replay_state.handle_pane_drag(event) {
                    self.set_active_tab_layout(TabLayout::Replay(layout));
                }
                Task::none()
            }
            Message::ReplayPaneResized(event) => {
                if let Some(layout) = self.replay_state.handle_pane_resize(event) {
                    self.set_active_tab_layout(TabLayout::Replay(layout));
                }
                Task::none()
            }
            Message::ReplayLoaded(result) => {
                if let Ok(data) = result {
                    self.replay_state.set_replay_data(data.collections, data.requests_by_collection);
                }
                Task::none()
            }
            Message::ReplayActiveVersionLoaded(result) => {
                if let Ok(version) = result {
                    self.replay_state.set_active_version(version);
                }
                Task::none()
            }
            Message::ReplayResponseLoaded(result) => {
                if let Ok(response) = result {
                    self.replay_state.set_latest_response(response);
                }
                Task::none()
            }
            Message::ReplayToggleCollection(collection_id) => {
                self.replay_state.toggle_collection(collection_id);
                Task::none()
            }
            Message::ReplayListCursor(point) => {
                self.replay_list_cursor = Some(point);
                Task::none()
            }
            Message::ReplayContextMenuOpen(request_id) => {
                if let Some(position) = self.replay_list_cursor {
                    self.replay_context_menu = Some(ReplayContextMenu {
                        request_id,
                        position: replay_list_to_window(position),
                    });
                    self.replay_collection_context_menu = None;
                    self.replay_collection_menu_open = false;
                    self.replay_collection_hover = false;
                    self.replay_collection_menu_hover = false;
                    self.replay_collection_bridge_hover = false;
                }
                Task::none()
            }
            Message::ReplayContextMenuClose => {
                self.replay_context_menu = None;
                self.replay_collection_context_menu = None;
                self.replay_collection_menu_open = false;
                self.replay_collection_hover = false;
                self.replay_collection_menu_hover = false;
                self.replay_collection_bridge_hover = false;
                Task::none()
            }
            Message::ReplayDuplicate(request_id) => {
                self.replay_context_menu = None;
                self.replay_collection_menu_open = false;
                Task::batch([
                    self.duplicate_replay_request(request_id),
                    self.load_replay_list(),
                ])
            }
            Message::ReplayRenamePrompt(request_id) => {
                self.replay_context_menu = None;
                self.replay_prompt_label = self
                    .replay_state
                    .request_name(request_id)
                    .unwrap_or_default();
                self.replay_prompt_mode = Some(ReplayPromptMode::Rename(request_id));
                Task::batch([
                    text_input::focus(self.replay_prompt_input_id.clone()),
                    text_input::move_cursor_to_end(self.replay_prompt_input_id.clone()),
                ])
            }
            Message::ReplayPromptLabel(label) => {
                self.replay_prompt_label = label;
                Task::none()
            }
            Message::ReplayPromptConfirm => self.confirm_replay_prompt(),
            Message::ReplayPromptCancel => {
                self.replay_prompt_mode = None;
                Task::none()
            }
            Message::ReplayAddToCollectionMenu(hovered) => {
                self.replay_collection_hover = hovered;
                if hovered {
                    self.replay_collection_menu_open = true;
                }
                Task::none()
            }
            Message::ReplayCollectionMenuHover(hovered) => {
                self.replay_collection_menu_hover = hovered;
                if hovered {
                    self.replay_collection_menu_open = true;
                }
                Task::none()
            }
            Message::ReplayCollectionMenuBridgeHover(hovered) => {
                self.replay_collection_bridge_hover = hovered;
                if hovered {
                    self.replay_collection_menu_open = true;
                }
                Task::none()
            }
            Message::ReplayCollectionMenuExit => {
                self.replay_collection_menu_open = false;
                self.replay_collection_hover = false;
                self.replay_collection_menu_hover = false;
                self.replay_collection_bridge_hover = false;
                Task::none()
            }
            Message::ReplayAddToCollection(collection_id) => {
                let request_id = match self.replay_context_menu.as_ref() {
                    Some(menu) => menu.request_id,
                    None => return Task::none(),
                };
                self.replay_context_menu = None;
                self.replay_collection_menu_open = false;
                Task::batch([
                    self.move_replay_request_to_collection(request_id, Some(collection_id)),
                    self.load_replay_list(),
                ])
            }
            Message::ReplayNewCollectionPrompt(request_id) => {
                self.replay_context_menu = None;
                self.replay_prompt_label.clear();
                self.replay_prompt_mode = Some(ReplayPromptMode::NewCollection(request_id));
                Task::batch([
                    text_input::focus(self.replay_prompt_input_id.clone()),
                    text_input::move_cursor_to_end(self.replay_prompt_input_id.clone()),
                ])
            }
            Message::ReplayCreateCollection => self.confirm_replay_prompt(),
            Message::ReplayCollectionMenuOpen(collection_id) => {
                if let Some(position) = self.replay_list_cursor {
                    self.replay_collection_context_menu = Some(ReplayCollectionContextMenu {
                        collection_id,
                        position: replay_list_to_window(position),
                    });
                    self.replay_context_menu = None;
                    self.replay_collection_color_open = false;
                    self.replay_collection_color_hover = false;
                    self.replay_collection_color_bridge_hover = false;
                }
                Task::none()
            }
            Message::ReplayCollectionMenuClose => {
                self.replay_collection_context_menu = None;
                self.replay_collection_color_open = false;
                self.replay_collection_color_hover = false;
                self.replay_collection_color_bridge_hover = false;
                Task::none()
            }
            Message::ReplayCollectionRenamePrompt(collection_id) => {
                self.replay_collection_context_menu = None;
                self.replay_prompt_label = self
                    .replay_state
                    .collection_name(collection_id)
                    .unwrap_or_default();
                self.replay_prompt_mode = Some(ReplayPromptMode::RenameCollection(collection_id));
                Task::batch([
                    text_input::focus(self.replay_prompt_input_id.clone()),
                    text_input::move_cursor_to_end(self.replay_prompt_input_id.clone()),
                ])
            }
            Message::ReplayCollectionColorMenuHover(hovered) => {
                self.replay_collection_color_hover = hovered;
                if hovered {
                    self.replay_collection_color_open = true;
                }
                Task::none()
            }
            Message::ReplayCollectionColorBridgeHover(hovered) => {
                self.replay_collection_color_bridge_hover = hovered;
                if hovered {
                    self.replay_collection_color_open = true;
                }
                Task::none()
            }
            Message::ReplayCollectionColorExit => {
                self.replay_collection_color_open = false;
                self.replay_collection_color_hover = false;
                self.replay_collection_color_bridge_hover = false;
                Task::none()
            }
            Message::ReplayCollectionSetColor(collection_id, color) => {
                self.replay_collection_context_menu = None;
                self.replay_collection_color_open = false;
                Task::batch([
                    self.update_replay_collection_color(collection_id, color),
                    self.load_replay_list(),
                ])
            }
            Message::ReplayCreatedFromTimeline(result) => {
                if let Ok(request_id) = result {
                    self.replay_state.select(request_id);
                    return Task::batch([
                        self.load_replay_list(),
                        self.load_replay_active_version(request_id),
                        self.load_replay_response(request_id),
                    ]);
                }
                Task::none()
            }
            Message::ReplayDragStart(request_id, collection_id) => {
                self.replay_context_menu = None;
                self.replay_drag = Some(ReplayDragState {
                    request_id,
                    source_collection_id: collection_id,
                });
                self.replay_drag_hover = Some(ReplayDropTarget::Request {
                    request_id,
                    collection_id,
                });
                self.replay_state.select(request_id);
                Task::batch([
                    self.load_replay_active_version(request_id),
                    self.load_replay_response(request_id),
                ])
            }
            Message::ReplayDragHover(target) => {
                if self.replay_drag.is_some() {
                    self.replay_drag_hover = Some(target);
                }
                Task::none()
            }
            Message::ReplayDragHoverClear => {
                if self.replay_drag.is_some() {
                    self.replay_drag_hover = None;
                }
                Task::none()
            }
            Message::ReplayDragEnd => {
                let drag = self.replay_drag.take();
                let target = self.replay_drag_hover.take();
                if let (Some(drag), Some(target)) = (drag, target) {
                    self.apply_replay_drag(drag, target)
                } else {
                    Task::none()
                }
            }
            Message::TimelineListCursor(point) => {
                self.timeline_list_cursor = Some(point);
                Task::none()
            }
            Message::TimelineContextMenuOpen(request_id) => {
                if let Some(position) = self.timeline_list_cursor {
                    self.timeline_context_menu = Some(TimelineContextMenu {
                        request_id,
                        position: timeline_list_to_window(position),
                    });
                }
                Task::none()
            }
            Message::TimelineContextMenuClose => {
                self.timeline_context_menu = None;
                Task::none()
            }
            Message::TimelineSendToReplay(request_id) => {
                self.timeline_context_menu = None;
                self.send_timeline_to_replay(request_id)
            }
            Message::ToggleMenu(menu) => {
                if self.active_menu == Some(menu) {
                    self.active_menu = None;
                    self.view_tabs_open = false;
                    self.view_tabs_hover = false;
                    self.view_submenu_hover = false;
                    self.view_submenu_bridge_hover = false;
                    self.view_panes_open = false;
                    self.view_panes_hover = false;
                    self.view_panes_submenu_hover = false;
                    self.view_panes_bridge_hover = false;
                } else {
                    self.active_menu = Some(menu);
                    if menu != MenuKind::View {
                        self.view_tabs_open = false;
                        self.view_tabs_hover = false;
                        self.view_submenu_hover = false;
                        self.view_submenu_bridge_hover = false;
                        self.view_panes_open = false;
                        self.view_panes_hover = false;
                        self.view_panes_submenu_hover = false;
                        self.view_panes_bridge_hover = false;
                    }
                }
                Task::none()
            }
            Message::CloseMenu => {
                self.active_menu = None;
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
                self.view_panes_open = false;
                self.view_panes_hover = false;
                self.view_panes_submenu_hover = false;
                self.view_panes_bridge_hover = false;
                Task::none()
            }
            Message::OpenNewTabPrompt => {
                self.active_menu = None;
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
                self.view_panes_open = false;
                self.view_panes_hover = false;
                self.view_panes_submenu_hover = false;
                self.view_panes_bridge_hover = false;
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
                self.view_panes_open = false;
                self.view_panes_hover = false;
                self.view_panes_submenu_hover = false;
                self.view_panes_bridge_hover = false;
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
                self.view_panes_open = false;
                self.view_panes_hover = false;
                self.view_panes_submenu_hover = false;
                self.view_panes_bridge_hover = false;
                self.save_tabs_and_layouts()
            }
            Message::AddDefaultTab(kind) => {
                self.active_menu = None;
                self.view_tabs_open = false;
                self.view_tabs_hover = false;
                self.view_submenu_hover = false;
                self.view_submenu_bridge_hover = false;
                self.view_panes_open = false;
                self.view_panes_hover = false;
                self.view_panes_submenu_hover = false;
                self.view_panes_bridge_hover = false;
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
                    self.set_active_tab(tab_id);
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
            Message::AddPaneToTab(kind) => {
                self.active_menu = None;
                self.view_panes_open = false;
                self.view_panes_hover = false;
                self.view_panes_submenu_hover = false;
                self.view_panes_bridge_hover = false;
                self.add_pane_to_active_tab(kind);
                Task::none()
            }
            Message::ViewPanesHover(hovered) => {
                self.view_panes_hover = hovered;
                if hovered {
                    self.view_panes_open = true;
                }
                Task::none()
            }
            Message::ViewPanesSubmenuHover(hovered) => {
                self.view_panes_submenu_hover = hovered;
                if hovered {
                    self.view_panes_open = true;
                }
                Task::none()
            }
            Message::ViewPanesBridgeHover(hovered) => {
                self.view_panes_bridge_hover = hovered;
                if hovered {
                    self.view_panes_open = true;
                }
                Task::none()
            }
            Message::ViewPanesRegionExit => {
                self.view_panes_open = false;
                self.view_panes_hover = false;
                self.view_panes_submenu_hover = false;
                self.view_panes_bridge_hover = false;
                Task::none()
            }
            Message::CustomPaneDragged(event) => {
                let layout = if let Some(tab_id) = self.config.active_tab_id.clone() {
                    if let Some(state) = self.custom_tabs.get_mut(&tab_id) {
                        match event {
                            pane_grid::DragEvent::Dropped { pane, target } => {
                                state.drop(pane, target);
                            }
                            pane_grid::DragEvent::Picked { .. } => {}
                            pane_grid::DragEvent::Canceled { .. } => {}
                        }
                        Some(CustomLayout::from(state))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(layout) = layout {
                    self.set_active_tab_layout(TabLayout::Custom(layout));
                }
                Task::none()
            }
            Message::CustomPaneResized(event) => {
                let layout = if let Some(tab_id) = self.config.active_tab_id.clone() {
                    if let Some(state) = self.custom_tabs.get_mut(&tab_id) {
                        state.resize(event.split, event.ratio);
                        Some(CustomLayout::from(state))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(layout) = layout {
                    self.set_active_tab_layout(TabLayout::Custom(layout));
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

    fn save_tabs_and_layouts(&mut self) -> Task<Message> {
        let layout = match self.active_tab_kind() {
            Some(TabKind::Timeline) => match &self.screen {
                Screen::Timeline(state) => Some(TabLayout::Timeline(state.snapshot_layout())),
                _ => None,
            },
            Some(TabKind::Replay) => Some(TabLayout::Replay(self.replay_state.snapshot_layout())),
            Some(TabKind::Custom) => {
                let tab_id = self.config.active_tab_id.clone();
                tab_id
                    .as_ref()
                    .and_then(|id| self.custom_tabs.get(id))
                    .map(|state| TabLayout::Custom(CustomLayout::from(state)))
            }
            _ => None,
        };
        if let (Some(layout), Some(tab)) = (layout, self.active_tab_mut()) {
            tab.layout = Some(layout);
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
        if self.tab_prompt_mode.is_some() || self.replay_prompt_mode.is_some() {
            match key {
                Key::Named(keyboard::key::Named::Enter) => {
                    return self.confirm_active_prompt();
                }
                Key::Named(keyboard::key::Named::Escape) => {
                    return self.cancel_active_prompt();
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
                if self.replay_context_menu.is_some() {
                    self.replay_context_menu = None;
                    self.replay_collection_menu_open = false;
                    self.replay_collection_hover = false;
                    self.replay_collection_menu_hover = false;
                    self.replay_collection_bridge_hover = false;
                    return Task::none();
                }
                if self.replay_collection_context_menu.is_some() {
                    self.replay_collection_context_menu = None;
                    self.replay_collection_color_open = false;
                    self.replay_collection_color_hover = false;
                    self.replay_collection_color_bridge_hover = false;
                    return Task::none();
                }
                if self.timeline_context_menu.is_some() {
                    self.timeline_context_menu = None;
                    return Task::none();
                }
                if self.active_menu.is_some() {
                    self.active_menu = None;
                    self.view_tabs_open = false;
                    self.view_tabs_hover = false;
                    self.view_submenu_hover = false;
                    self.view_submenu_bridge_hover = false;
                    self.view_panes_open = false;
                    self.view_panes_hover = false;
                    self.view_panes_submenu_hover = false;
                    self.view_panes_bridge_hover = false;
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
                let active_tab = self.active_tab().cloned();
                let content = match active_tab.as_ref().map(|tab| tab.kind) {
                    Some(TabKind::Timeline) | None => {
                        if matches!(active_tab.as_ref().and_then(|tab| tab.layout.as_ref()), Some(TabLayout::Custom(_))) {
                            self.custom_tab_view(TabKind::Timeline, &self.theme)
                        } else {
                            state.view(
                                self.focus,
                                &self.theme,
                                Some(Message::TimelineContextMenuOpen),
                                Some(Message::TimelineListCursor),
                            )
                        }
                    }
                    Some(TabKind::Replay) => {
                        if matches!(active_tab.as_ref().and_then(|tab| tab.layout.as_ref()), Some(TabLayout::Custom(_))) {
                            self.custom_tab_view(TabKind::Replay, &self.theme)
                        } else {
                            self.replay_state.view(&self.theme)
                        }
                    }
                    Some(TabKind::Custom) => self.custom_tab_view(TabKind::Custom, &self.theme),
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
        if self.active_menu.is_some() {
            let backdrop = mouse_area(container(Space::new(Length::Fill, Length::Fill)))
                .on_press(Message::CloseMenu)
                .on_right_press(Message::CloseMenu)
                .interaction(mouse::Interaction::Pointer)
                .into();
            layers.push(backdrop);
        }
        if let Some(overlay) = self.menu_overlay() {
            layers.push(overlay);
        }
        if let Some(context_menu) = self.tab_context_menu_overlay() {
            layers.push(context_menu);
        }
        if let Some(timeline_menu) = self.timeline_context_menu_overlay() {
            layers.push(timeline_menu);
        }
        if let Some(replay_menu) = self.replay_context_menu_overlay() {
            layers.push(replay_menu);
        }
        if let Some(collection_menu) = self.replay_collection_context_menu_overlay() {
            layers.push(collection_menu);
        }
        if let Some(prompt) = self.replay_prompt_view() {
            layers.push(prompt);
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
        let panes_hover = self.view_panes_open;
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

        let panes_label = row![
            text("Panes").size(12).color(self.theme.text),
            Space::new(Length::Fill, Length::Shrink),
            text("▶").size(10).color(self.theme.muted_text),
        ]
        .align_y(Alignment::Center);
        let panes_button = iced::widget::button(panes_label)
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });
        let panes_area = mouse_area(panes_button)
            .on_enter(Message::ViewPanesHover(true))
            .on_exit(Message::ViewPanesHover(false))
            .interaction(mouse::Interaction::Pointer);

        let panel = container(column![save_button, tabs_area, panes_area].spacing(6))
            .padding(8)
            .width(Length::Fixed(200.0))
            .style({
                let theme = self.theme;
                move |_| menu_panel_style(theme)
            });

        let mut region: Element<'a, Message> = panel.into();

        if tabs_hover {
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
            region = submenu_region(
                region,
                submenu,
                VIEW_SUBMENU_GAP,
                Message::ViewSubmenuHover(true),
                Message::ViewSubmenuHover(false),
                Message::ViewSubmenuBridgeHover(true),
                Message::ViewSubmenuBridgeHover(false),
                Message::ViewTabsRegionExit,
            );
        }

        if panes_hover {
            let panes_menu = menu_panel(
                vec![
                    MenuItem {
                        label: "Request List",
                        message: Some(Message::AddPaneToTab(PaneModuleKind::RequestList)),
                        enabled: true,
                        tooltip: None,
                    },
                    MenuItem {
                        label: "Request Details",
                        message: Some(Message::AddPaneToTab(PaneModuleKind::RequestDetails)),
                        enabled: true,
                        tooltip: None,
                    },
                    MenuItem {
                        label: "Response Preview",
                        message: Some(Message::AddPaneToTab(PaneModuleKind::ResponsePreview)),
                        enabled: true,
                        tooltip: None,
                    },
                    MenuItem {
                        label: "Replay List",
                        message: Some(Message::AddPaneToTab(PaneModuleKind::ReplayList)),
                        enabled: true,
                        tooltip: None,
                    },
                    MenuItem {
                        label: "Replay Editor",
                        message: Some(Message::AddPaneToTab(PaneModuleKind::ReplayEditor)),
                        enabled: true,
                        tooltip: None,
                    },
                ],
                &self.theme,
            );
            region = submenu_region(
                region,
                panes_menu,
                VIEW_SUBMENU_GAP,
                Message::ViewPanesSubmenuHover(true),
                Message::ViewPanesSubmenuHover(false),
                Message::ViewPanesBridgeHover(true),
                Message::ViewPanesBridgeHover(false),
                Message::ViewPanesRegionExit,
            );
        }

        region
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
            let display_label = truncate_tab_label(tab.label.as_str(), TAB_MAX_WIDTH);
            let label = text(display_label.clone())
                .size(13)
                .color(self.theme.text)
                .wrapping(iced::widget::text::Wrapping::None);
            let width = tab_button_width(&display_label).min(TAB_MAX_WIDTH);
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
        Some(prompt_overlay(
            title,
            "Tab name",
            &self.tab_prompt_label,
            self.tab_prompt_input_id.clone(),
            Message::UpdateNewTabLabel,
            confirm_label,
            Message::ConfirmTabPrompt,
            Message::CancelTabPrompt,
            self.theme,
        ))
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
            if let Some(tab) = self.tab_for_kind_mut(TabKind::Timeline) {
                if tab.layout.is_none() {
                    tab.layout = Some(TabLayout::Timeline(layout));
                }
            }
        }
    }

    fn tab_for_kind_mut(&mut self, kind: TabKind) -> Option<&mut TabConfig> {
        self.config.tabs.iter_mut().find(|tab| tab.kind == kind)
    }

    fn tab_for_kind(&self, kind: TabKind) -> Option<&TabConfig> {
        self.config.tabs.iter().find(|tab| tab.kind == kind)
    }

    fn active_tab_mut(&mut self) -> Option<&mut TabConfig> {
        let id = self.config.active_tab_id.as_deref()?;
        self.config.tabs.iter_mut().find(|tab| tab.id == id)
    }

    fn active_tab(&self) -> Option<&TabConfig> {
        let id = self.config.active_tab_id.as_deref()?;
        self.config.tabs.iter().find(|tab| tab.id == id)
    }

    fn timeline_tab_layout(&self) -> Option<PaneLayout> {
        let tab = self
            .active_tab()
            .filter(|tab| tab.kind == TabKind::Timeline)
            .or_else(|| self.tab_for_kind(TabKind::Timeline))?;
        match &tab.layout {
            Some(TabLayout::Timeline(layout)) => Some(layout.clone()),
            _ => None,
        }
    }

    fn active_tab_kind(&self) -> Option<TabKind> {
        let id = self.config.active_tab_id.as_deref()?;
        self.config.tabs.iter().find(|tab| tab.id == id).map(|tab| tab.kind)
    }

    fn set_active_tab(&mut self, tab_id: String) {
        self.config.active_tab_id = Some(tab_id);
        self.sync_active_tab_layout();
    }

    fn sync_active_tab_layout(&mut self) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        match tab.kind {
            TabKind::Timeline => {
                if let Screen::Timeline(state) = &mut self.screen {
                    match tab.layout {
                        Some(TabLayout::Custom(_)) => {
                            self.ensure_custom_state(&tab);
                        }
                        Some(TabLayout::Timeline(layout)) => state.apply_layout(layout),
                        _ => state.apply_layout(default_pane_layout()),
                    }
                }
            }
            TabKind::Replay => {
                match tab.layout {
                    Some(TabLayout::Custom(_)) => {
                        self.ensure_custom_state(&tab);
                    }
                    Some(TabLayout::Replay(layout)) => self.replay_state.apply_layout(layout),
                    _ => self.replay_state.apply_layout(default_replay_layout()),
                }
            }
            TabKind::Custom => {
                self.ensure_custom_state(&tab);
            }
            _ => {}
        }
    }

    fn set_active_tab_layout(&mut self, layout: TabLayout) {
        if let Some(tab) = self.active_tab_mut() {
            tab.layout = Some(layout);
        }
    }

    fn project_store_path(&self) -> PathBuf {
        match &self.screen {
            Screen::Timeline(state) => state.store_path.clone(),
            Screen::ProjectSettings(settings) => settings.project_paths.database.clone(),
            Screen::ProjectPicker(_) => PathBuf::new(),
        }
    }

    fn ensure_custom_state(&mut self, tab: &TabConfig) {
        let layout = match tab.layout.clone() {
            Some(TabLayout::Custom(layout)) => layout,
            _ => default_custom_layout(),
        };
        self.custom_tabs
            .entry(tab.id.clone())
            .or_insert_with(|| pane_grid::State::with_configuration(layout.to_configuration()));
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

    fn custom_tab_view(&self, context: TabKind, theme: &ThemePalette) -> Element<'_, Message> {
        let Some(tab_id) = self.config.active_tab_id.as_ref() else {
            return self.placeholder_view(TabKind::Custom);
        };
        let Some(state) = self.custom_tabs.get(tab_id) else {
            return self.placeholder_view(TabKind::Custom);
        };

        let grid = PaneGrid::new(state, |_, pane_kind, _| {
            let pane_content = self.render_custom_pane(*pane_kind, context, *theme);
            let content = container(pane_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style({
                let theme = *theme;
                move |_| pane_border_style(theme)
            });
            let title_text = text(pane_kind.title()).size(13).style({
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
        .on_drag(Message::CustomPaneDragged)
        .on_resize(10, Message::CustomPaneResized);

        container(grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn render_custom_pane(
        &self,
        pane_kind: PaneModuleKind,
        context: TabKind,
        theme: ThemePalette,
    ) -> Element<'_, Message> {
        match pane_kind {
            PaneModuleKind::RequestList => match context {
                TabKind::Timeline => {
                    if let Screen::Timeline(state) = &self.screen {
                        timeline_request_list_view(
                            &state.timeline,
                            &state.tags,
                            &state.responses,
                            state.selected,
                            theme,
                            Some(Message::TimelineContextMenuOpen),
                            Some(Message::TimelineListCursor),
                        )
                    } else {
                        self.pane_placeholder("No timeline data", theme)
                    }
                }
                TabKind::Replay => self.replay_state.request_list_view(theme),
                _ => self.pane_placeholder("Request list", theme),
            },
            PaneModuleKind::RequestDetails => match context {
                TabKind::Timeline => {
                    if let Screen::Timeline(state) = &self.screen {
                        let selected = state.selected.and_then(|idx| state.timeline.get(idx));
                        let response = selected.and_then(|item| state.responses.get(&item.id));
                        timeline_request_details_view(selected, response, theme)
                    } else {
                        self.pane_placeholder("No timeline data", theme)
                    }
                }
                _ => self.pane_placeholder("Request details", theme),
            },
            PaneModuleKind::ResponsePreview => match context {
                TabKind::Timeline => {
                    if let Screen::Timeline(state) = &self.screen {
                        if let Some(selected) = state.selected.and_then(|idx| state.timeline.get(idx)) {
                            let response = state.responses.get(&selected.id);
                            if let Some(response) = response {
                                let timeline_response = SqliteStore::open(&state.store_path)
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
                                let status_line = response
                                    .reason
                                    .clone()
                                    .map(|reason| format!("{} {reason}", response.status_code))
                                    .unwrap_or_else(|| response.status_code.to_string());
                                let truncated = timeline_response
                                    .as_ref()
                                    .map(|resp| resp.response_body_truncated)
                                    .unwrap_or(false);
                                return response_preview_from_bytes(
                                    status_line,
                                    response_headers,
                                    body,
                                    truncated,
                                    theme,
                                );
                            }
                        }
                        response_preview_placeholder("No response recorded yet", theme)
                    } else {
                        response_preview_placeholder("No timeline data", theme)
                    }
                }
                TabKind::Replay => {
                    response_preview_placeholder("Replay response preview", theme)
                }
                _ => response_preview_placeholder("Response preview", theme),
            },
            PaneModuleKind::ReplayList => match context {
                TabKind::Replay => self.replay_state.request_list_view(theme),
                _ => self.pane_placeholder("Replay list", theme),
            },
            PaneModuleKind::ReplayEditor => match context {
                TabKind::Replay => self.replay_state.request_editor_view(theme),
                _ => self.pane_placeholder("Replay editor", theme),
            },
        }
    }

    fn pane_placeholder(&self, label: &str, theme: ThemePalette) -> Element<'_, Message> {
        container(column![text_muted(label.to_string(), 16, theme)])
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

    fn replay_context_menu_overlay<'a>(&'a self) -> Option<Element<'a, Message>> {
        let menu = self.replay_context_menu.as_ref()?;
        let mut items = column![].spacing(6);

        let duplicate = iced::widget::button(text("Duplicate").size(12).color(self.theme.text))
            .on_press(Message::ReplayDuplicate(menu.request_id))
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });
        let rename = iced::widget::button(text("Rename").size(12).color(self.theme.text))
            .on_press(Message::ReplayRenamePrompt(menu.request_id))
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });

        let collection_label = row![
            text("Add to Collection").size(12).color(self.theme.text),
            Space::new(Length::Fill, Length::Shrink),
            text("▶").size(10).color(self.theme.muted_text),
        ]
        .align_y(Alignment::Center);
        let collection_button = iced::widget::button(collection_label)
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });
        let collection_area = mouse_area(collection_button)
            .on_enter(Message::ReplayAddToCollectionMenu(true))
            .on_exit(Message::ReplayAddToCollectionMenu(false))
            .interaction(mouse::Interaction::Pointer);

        items = items.push(collection_area).push(rename).push(duplicate);

        let panel = container(items)
            .padding(8)
            .width(Length::Fixed(220.0))
            .style({
                let theme = self.theme;
                move |_| menu_panel_style(theme)
            });

        let mut region: Element<'a, Message> = panel.into();
        if self.replay_collection_menu_open {
            let mut submenu_content = column![].spacing(6);
            let new_button = iced::widget::button(text("New Collection...").size(12).color(self.theme.text))
                .on_press(Message::ReplayNewCollectionPrompt(menu.request_id))
                .padding([4, 10])
                .width(Length::Fill)
                .style({
                    let theme = self.theme;
                    move |_theme, status| menu_item_button_style(theme, status, true)
                });
            submenu_content = submenu_content.push(new_button);
            for collection in self.replay_state.collections() {
                let button = iced::widget::button(text(collection.name.clone()).size(12).color(self.theme.text))
                    .on_press(Message::ReplayAddToCollection(collection.id))
                    .padding([4, 10])
                    .width(Length::Fill)
                    .style({
                        let theme = self.theme;
                        move |_theme, status| menu_item_button_style(theme, status, true)
                    });
                submenu_content = submenu_content.push(button);
            }
            let submenu = container(submenu_content)
                .padding(8)
                .width(Length::Fixed(220.0))
                .style({
                    let theme = self.theme;
                    move |_| menu_panel_style(theme)
                });
            region = submenu_region(
                region,
                submenu.into(),
                VIEW_SUBMENU_GAP,
                Message::ReplayCollectionMenuHover(true),
                Message::ReplayCollectionMenuHover(false),
                Message::ReplayCollectionMenuBridgeHover(true),
                Message::ReplayCollectionMenuBridgeHover(false),
                Message::ReplayCollectionMenuExit,
            );
        }

        let backdrop = mouse_area(container(Space::new(Length::Fill, Length::Fill)))
            .on_press(Message::ReplayContextMenuClose)
            .on_right_press(Message::ReplayContextMenuClose)
            .interaction(mouse::Interaction::Pointer);

        let overlay = stack(vec![
            backdrop.into(),
            container(column![
                Space::new(Length::Shrink, Length::Fixed(menu.position.y)),
                row![
                    Space::new(Length::Fixed(menu.position.x), Length::Shrink),
                    region
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

    fn replay_collection_context_menu_overlay<'a>(&'a self) -> Option<Element<'a, Message>> {
        let menu = self.replay_collection_context_menu.as_ref()?;
        let mut items = column![].spacing(6);

        let rename = iced::widget::button(text("Rename collection").size(12).color(self.theme.text))
            .on_press(Message::ReplayCollectionRenamePrompt(menu.collection_id))
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });

        let color_label = row![
            text("Set Color").size(12).color(self.theme.text),
            Space::new(Length::Fill, Length::Shrink),
            text("▶").size(10).color(self.theme.muted_text),
        ]
        .align_y(Alignment::Center);
        let color_button = iced::widget::button(color_label)
            .padding([4, 10])
            .width(Length::Fill)
            .style({
                let theme = self.theme;
                move |_theme, status| menu_item_button_style(theme, status, true)
            });
        let color_area = mouse_area(color_button)
            .on_enter(Message::ReplayCollectionColorMenuHover(true))
            .on_exit(Message::ReplayCollectionColorMenuHover(false))
            .interaction(mouse::Interaction::Pointer);

        items = items.push(rename).push(color_area);

        let panel = container(items)
            .padding(8)
            .width(Length::Fixed(220.0))
            .style({
                let theme = self.theme;
                move |_| menu_panel_style(theme)
            });

        let mut region: Element<'a, Message> = panel.into();
        if self.replay_collection_color_open {
            let color_items = vec![
                ("Red", "#cc241d"),
                ("Orange", "#d65d0e"),
                ("Yellow", "#d79921"),
                ("Green", "#98971a"),
                ("Blue", "#458588"),
                ("Indigo", "#076678"),
                ("Violet", "#b16286"),
            ];
            let mut submenu_content = column![].spacing(6);
            for (label, hex) in color_items {
                let button = iced::widget::button(text(label).size(12).color(self.theme.text))
                    .on_press(Message::ReplayCollectionSetColor(
                        menu.collection_id,
                        Some(hex.to_string()),
                    ))
                    .padding([4, 10])
                    .width(Length::Fill)
                    .style({
                        let theme = self.theme;
                        move |_theme, status| menu_item_button_style(theme, status, true)
                    });
                submenu_content = submenu_content.push(button);
            }
            let clear_button = iced::widget::button(text("Default").size(12).color(self.theme.text))
                .on_press(Message::ReplayCollectionSetColor(menu.collection_id, None))
                .padding([4, 10])
                .width(Length::Fill)
                .style({
                    let theme = self.theme;
                    move |_theme, status| menu_item_button_style(theme, status, true)
                });
            submenu_content = submenu_content.push(clear_button);

            let submenu = container(submenu_content)
                .padding(8)
                .width(Length::Fixed(200.0))
                .style({
                    let theme = self.theme;
                    move |_| menu_panel_style(theme)
                });
            region = submenu_region(
                region,
                submenu.into(),
                VIEW_SUBMENU_GAP,
                Message::ReplayCollectionColorMenuHover(true),
                Message::ReplayCollectionColorMenuHover(false),
                Message::ReplayCollectionColorBridgeHover(true),
                Message::ReplayCollectionColorBridgeHover(false),
                Message::ReplayCollectionColorExit,
            );
        }

        let backdrop = mouse_area(container(Space::new(Length::Fill, Length::Fill)))
            .on_press(Message::ReplayCollectionMenuClose)
            .on_right_press(Message::ReplayCollectionMenuClose)
            .interaction(mouse::Interaction::Pointer);

        let overlay = stack(vec![
            backdrop.into(),
            container(column![
                Space::new(Length::Shrink, Length::Fixed(menu.position.y)),
                row![
                    Space::new(Length::Fixed(menu.position.x), Length::Shrink),
                    region
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

    fn timeline_context_menu_overlay<'a>(&'a self) -> Option<Element<'a, Message>> {
        let menu = self.timeline_context_menu.as_ref()?;
        let panel = container(
            column![
                iced::widget::button(text("Send to replay").size(12).color(self.theme.text))
                    .on_press(Message::TimelineSendToReplay(menu.request_id))
                    .padding([4, 10])
                    .width(Length::Fill)
                    .style({
                        let theme = self.theme;
                        move |_theme, status| menu_item_button_style(theme, status, true)
                    })
            ]
            .spacing(6),
        )
        .padding(8)
        .width(Length::Fixed(200.0))
        .style({
            let theme = self.theme;
            move |_| menu_panel_style(theme)
        });

        let backdrop = mouse_area(container(Space::new(Length::Fill, Length::Fill)))
            .on_press(Message::TimelineContextMenuClose)
            .on_right_press(Message::TimelineContextMenuClose)
            .interaction(mouse::Interaction::Pointer);

        let overlay = stack(vec![
            backdrop.into(),
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

    fn replay_prompt_view<'a>(&'a self) -> Option<Element<'a, Message>> {
        let mode = self.replay_prompt_mode.as_ref()?;
        let title = match mode {
            ReplayPromptMode::Rename(_) => "Rename replay request",
            ReplayPromptMode::NewCollection(_) => "New collection",
            ReplayPromptMode::RenameCollection(_) => "Rename collection",
        };
        let confirm_label = match mode {
            ReplayPromptMode::Rename(_) => "Save",
            ReplayPromptMode::NewCollection(_) => "Create",
            ReplayPromptMode::RenameCollection(_) => "Save",
        };
        Some(prompt_overlay(
            title,
            "Name",
            &self.replay_prompt_label,
            self.replay_prompt_input_id.clone(),
            Message::ReplayPromptLabel,
            confirm_label,
            Message::ReplayPromptConfirm,
            Message::ReplayPromptCancel,
            self.theme,
        ))
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
                    layout: Some(TabLayout::Custom(default_custom_layout())),
                });
                self.set_active_tab(id);
            }
            TabPromptMode::Rename(tab_id) => {
                if let Some(tab) = self.config.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    tab.label = label.to_string();
                }
            }
        }
        Task::none()
    }

    fn confirm_active_prompt(&mut self) -> Task<Message> {
        if self.tab_prompt_mode.is_some() {
            return self.confirm_tab_prompt();
        }
        if self.replay_prompt_mode.is_some() {
            return self.confirm_replay_prompt();
        }
        Task::none()
    }

    fn cancel_active_prompt(&mut self) -> Task<Message> {
        if self.tab_prompt_mode.is_some() {
            self.tab_prompt_mode = None;
            return Task::none();
        }
        if self.replay_prompt_mode.is_some() {
            self.replay_prompt_mode = None;
            return Task::none();
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
        self.set_active_tab(id);
        Task::none()
    }

    fn add_pane_to_active_tab(&mut self, pane: PaneModuleKind) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        if !matches!(tab.layout, Some(TabLayout::Custom(_))) {
            let custom_layout = custom_layout_for_tab(tab.kind);
            if let Some(active) = self.active_tab_mut() {
                active.layout = Some(TabLayout::Custom(custom_layout.clone()));
            }
            self.custom_tabs.insert(
                tab.id.clone(),
                pane_grid::State::with_configuration(custom_layout.to_configuration()),
            );
        }

        let layout = if let Some(state) = self.custom_tabs.get_mut(&tab.id) {
            let target = state.iter().next().map(|(pane, _)| *pane);
            if let Some(target) = target {
                let _ = state.split(pane_grid::Axis::Horizontal, target, pane);
                Some(CustomLayout::from(state))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(layout) = layout {
            self.set_active_tab_layout(TabLayout::Custom(layout));
        }
    }

    fn load_replay_list(&self) -> Task<Message> {
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        Task::perform(fetch_replay_list(path), Message::ReplayLoaded)
    }

    fn load_replay_active_version(&self, request_id: i64) -> Task<Message> {
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        Task::perform(get_replay_active_version(path, request_id), Message::ReplayActiveVersionLoaded)
    }

    fn load_replay_response(&self, request_id: i64) -> Task<Message> {
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        Task::perform(get_latest_replay_response(path, request_id), Message::ReplayResponseLoaded)
    }

    fn duplicate_replay_request(&self, request_id: i64) -> Task<Message> {
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        Task::perform(duplicate_replay_request(path, request_id), |_| Message::ReplayContextMenuClose)
    }

    fn move_replay_request_to_collection(
        &self,
        request_id: i64,
        collection_id: Option<i64>,
    ) -> Task<Message> {
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        Task::perform(move_replay_request_to_collection(path, request_id, collection_id), |_| {
            Message::ReplayContextMenuClose
        })
    }

    fn confirm_replay_prompt(&mut self) -> Task<Message> {
        let label = self.replay_prompt_label.trim();
        let Some(mode) = self.replay_prompt_mode.clone() else {
            return Task::none();
        };
        if label.is_empty() {
            return Task::none();
        }
        self.replay_prompt_mode = None;
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        match mode {
            ReplayPromptMode::Rename(request_id) => Task::batch([
                Task::perform(
                    update_replay_request_name(path, request_id, label.to_string()),
                    |_| Message::ReplayContextMenuClose,
                ),
                self.load_replay_list(),
            ]),
            ReplayPromptMode::NewCollection(request_id) => Task::batch([
                Task::perform(
                    create_collection_and_add_request(path.clone(), label.to_string(), request_id),
                    |_| Message::ReplayContextMenuClose,
                ),
                self.load_replay_list(),
            ]),
            ReplayPromptMode::RenameCollection(collection_id) => Task::batch([
                Task::perform(
                    update_replay_collection_name(path, collection_id, label.to_string()),
                    |_| Message::ReplayCollectionMenuClose,
                ),
                self.load_replay_list(),
            ]),
        }
    }

    fn update_replay_collection_color(
        &self,
        collection_id: i64,
        color: Option<String>,
    ) -> Task<Message> {
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        Task::perform(
            update_replay_collection_color(path, collection_id, color),
            |_| Message::ReplayCollectionMenuClose,
        )
    }

    fn apply_replay_drag(&self, drag: ReplayDragState, target: ReplayDropTarget) -> Task<Message> {
        if matches!(
            target,
            ReplayDropTarget::Request {
                request_id,
                collection_id,
            } if request_id == drag.request_id && collection_id == drag.source_collection_id
        ) {
            return Task::none();
        }
        let Some(path) = self.replay_state.store_path().cloned() else {
            return Task::none();
        };
        let target_collection = match target {
            ReplayDropTarget::Request { collection_id, .. } => collection_id,
            ReplayDropTarget::Collection { collection_id } => collection_id,
        };
        let target_request_id = match target {
            ReplayDropTarget::Request { request_id, .. } => Some(request_id),
            ReplayDropTarget::Collection { .. } => None,
        };
        let mut ordered: Vec<i64> = self
            .replay_state
            .requests_in_collection(target_collection)
            .map(|items| items.iter().map(|item| item.id).collect())
            .unwrap_or_default();
        ordered.retain(|id| *id != drag.request_id);
        if let Some(target_id) = target_request_id {
            let insert_at = ordered
                .iter()
                .position(|id| *id == target_id)
                .unwrap_or(ordered.len());
            ordered.insert(insert_at, drag.request_id);
        } else {
            ordered.push(drag.request_id);
        }

        let mut tasks = Vec::new();
        for (index, request_id) in ordered.iter().enumerate() {
            let sort_index = (ordered.len() - index) as i64;
            tasks.push(Task::perform(
                update_replay_request_sort(path.clone(), *request_id, target_collection, sort_index),
                |_| Message::ReplayContextMenuClose,
            ));
        }
        tasks.push(self.load_replay_list());
        tasks.push(self.load_replay_active_version(drag.request_id));
        tasks.push(self.load_replay_response(drag.request_id));
        Task::batch(tasks)
    }

    fn send_timeline_to_replay(&self, request_id: i64) -> Task<Message> {
        let path = self.project_store_path();
        if path.as_os_str().is_empty() {
            return Task::none();
        }
        Task::perform(
            create_replay_from_timeline(path, request_id),
            Message::ReplayCreatedFromTimeline,
        )
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TabLayout {
    Timeline(PaneLayout),
    Replay(ReplayLayout),
    Custom(CustomLayout),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomLayout {
    root: CustomLayoutNode,
}

impl CustomLayout {
    pub fn from(state: &pane_grid::State<PaneModuleKind>) -> Self {
        Self {
            root: CustomLayoutNode::from(state.layout(), state),
        }
    }

    pub fn to_configuration(&self) -> pane_grid::Configuration<PaneModuleKind> {
        self.root.to_configuration()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CustomLayoutNode {
    Pane(PaneModuleKind),
    Split {
        axis: CustomLayoutAxis,
        ratio: f32,
        a: Box<CustomLayoutNode>,
        b: Box<CustomLayoutNode>,
    },
}

impl CustomLayoutNode {
    fn from(node: &pane_grid::Node, panes: &pane_grid::State<PaneModuleKind>) -> Self {
        match node {
            pane_grid::Node::Pane(pane) => {
                let kind = panes.get(*pane).copied().unwrap_or(PaneModuleKind::RequestList);
                CustomLayoutNode::Pane(kind)
            }
            pane_grid::Node::Split { axis, ratio, a, b, .. } => CustomLayoutNode::Split {
                axis: CustomLayoutAxis::from(*axis),
                ratio: *ratio,
                a: Box::new(CustomLayoutNode::from(a, panes)),
                b: Box::new(CustomLayoutNode::from(b, panes)),
            },
        }
    }

    fn to_configuration(&self) -> pane_grid::Configuration<PaneModuleKind> {
        match self {
            CustomLayoutNode::Pane(pane) => pane_grid::Configuration::Pane(*pane),
            CustomLayoutNode::Split { axis, ratio, a, b } => pane_grid::Configuration::Split {
                axis: axis.to_axis(),
                ratio: *ratio,
                a: Box::new(a.to_configuration()),
                b: Box::new(b.to_configuration()),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum CustomLayoutAxis {
    Horizontal,
    Vertical,
}

impl CustomLayoutAxis {
    fn from(axis: pane_grid::Axis) -> Self {
        match axis {
            pane_grid::Axis::Horizontal => CustomLayoutAxis::Horizontal,
            pane_grid::Axis::Vertical => CustomLayoutAxis::Vertical,
        }
    }

    fn to_axis(self) -> pane_grid::Axis {
        match self {
            CustomLayoutAxis::Horizontal => pane_grid::Axis::Horizontal,
            CustomLayoutAxis::Vertical => pane_grid::Axis::Vertical,
        }
    }
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
    pub layout: Option<TabLayout>,
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
    text_width + TAB_BUTTON_PADDING_X * 2.0 + TAB_TEXT_FUDGE
}

fn truncate_tab_label(label: &str, max_width: f32) -> String {
    let max_chars = ((max_width - TAB_BUTTON_PADDING_X * 2.0 - TAB_TEXT_FUDGE) / TAB_CHAR_WIDTH)
        .floor()
        .max(0.0) as usize;
    if max_chars == 0 {
        return String::new();
    }
    let char_count = label.chars().count();
    if char_count <= max_chars {
        return label.to_string();
    }
    let suffix = "...";
    if max_chars <= suffix.len() {
        return suffix[..max_chars.min(suffix.len())].to_string();
    }
    let take_len = max_chars.saturating_sub(suffix.len());
    let truncated: String = label.chars().take(take_len).collect();
    format!("{truncated}{suffix}")
}

fn default_custom_layout() -> CustomLayout {
    CustomLayout {
        root: CustomLayoutNode::Pane(PaneModuleKind::RequestList),
    }
}

fn custom_layout_for_tab(kind: TabKind) -> CustomLayout {
    match kind {
        TabKind::Timeline => CustomLayout {
            root: CustomLayoutNode::Split {
                axis: CustomLayoutAxis::Vertical,
                ratio: 0.4,
                a: Box::new(CustomLayoutNode::Pane(PaneModuleKind::RequestList)),
                b: Box::new(CustomLayoutNode::Split {
                    axis: CustomLayoutAxis::Horizontal,
                    ratio: 0.5,
                    a: Box::new(CustomLayoutNode::Pane(PaneModuleKind::RequestDetails)),
                    b: Box::new(CustomLayoutNode::Pane(PaneModuleKind::ResponsePreview)),
                }),
            },
        },
        TabKind::Replay => CustomLayout {
            root: CustomLayoutNode::Split {
                axis: CustomLayoutAxis::Vertical,
                ratio: 0.15,
                a: Box::new(CustomLayoutNode::Pane(PaneModuleKind::ReplayList)),
                b: Box::new(CustomLayoutNode::Split {
                    axis: CustomLayoutAxis::Vertical,
                    ratio: 0.5,
                    a: Box::new(CustomLayoutNode::Pane(PaneModuleKind::ReplayEditor)),
                    b: Box::new(CustomLayoutNode::Pane(PaneModuleKind::ResponsePreview)),
                }),
            },
        },
        _ => default_custom_layout(),
    }
}

async fn fetch_replay_list(store_path: PathBuf) -> Result<ReplayListData, String> {
    let collections = list_replay_collections(store_path.clone()).await?;
    let mut requests_by_collection: HashMap<Option<i64>, Vec<crossfeed_storage::ReplayRequest>> = HashMap::new();
    let unassigned = list_replay_requests_unassigned(store_path.clone()).await?;
    requests_by_collection.insert(None, unassigned);
    for collection in &collections {
        let requests = list_replay_requests_in_collection(store_path.clone(), collection.id).await?;
        requests_by_collection.insert(Some(collection.id), requests);
    }
    Ok(ReplayListData {
        collections,
        requests_by_collection,
    })
}

fn default_layout_for(kind: TabKind) -> Option<TabLayout> {
    match kind {
        TabKind::Timeline => Some(TabLayout::Timeline(default_pane_layout())),
        TabKind::Replay => Some(TabLayout::Replay(default_replay_layout())),
        TabKind::Custom => Some(TabLayout::Custom(default_custom_layout())),
        _ => None,
    }
}

fn tab_bar_to_window(point: Point) -> Point {
    Point::new(
        point.x + TAB_BAR_PADDING_X,
        point.y + MENU_HEIGHT + TAB_BAR_PADDING_Y,
    )
}

fn timeline_list_to_window(point: Point) -> Point {
    Point::new(point.x, point.y + MENU_HEIGHT + TAB_BAR_HEIGHT)
}

fn replay_list_to_window(point: Point) -> Point {
    Point::new(point.x, point.y + MENU_HEIGHT + TAB_BAR_HEIGHT)
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

fn submenu_region<'a>(
    panel: Element<'a, Message>,
    submenu: Element<'a, Message>,
    bridge_width: f32,
    on_enter: Message,
    on_exit: Message,
    bridge_on_enter: Message,
    bridge_on_exit: Message,
    on_region_exit: Message,
) -> Element<'a, Message> {
    let submenu = mouse_area(submenu)
        .on_enter(on_enter)
        .on_exit(on_exit)
        .interaction(mouse::Interaction::Pointer);
    let region = submenu_with_bridge(
        panel,
        submenu.into(),
        bridge_width,
        bridge_on_enter,
        bridge_on_exit,
    );
    mouse_area(region)
        .on_exit(on_region_exit)
        .interaction(mouse::Interaction::Pointer)
        .into()
}

fn prompt_overlay<'a>(
    title: &str,
    input_placeholder: &str,
    value: &str,
    input_id: text_input::Id,
    on_input: fn(String) -> Message,
    confirm_label: &str,
    confirm: Message,
    cancel: Message,
    theme: ThemePalette,
) -> Element<'a, Message> {
    let prompt = container(
        column![
            text_primary(title.to_string(), 16, theme),
            text_input(input_placeholder, value)
                .id(input_id)
                .on_input(on_input)
                .padding(8)
                .style({
                    let theme = theme;
                    move |_theme, status| text_input_style(theme, status)
                }),
            row![
                action_button(confirm_label, confirm.clone(), theme),
                action_button("Cancel", cancel.clone(), theme),
            ]
            .spacing(12),
        ]
        .spacing(12),
    )
    .padding(16)
    .style({
        let theme = theme;
        move |_| menu_panel_style(theme)
    });

    let backdrop = mouse_area(container(Space::new(Length::Fill, Length::Fill)))
        .on_press(cancel.clone())
        .on_right_press(cancel)
        .interaction(mouse::Interaction::Pointer);

    let overlay = stack(vec![
        backdrop.into(),
        container(prompt)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into(),
    ]);

    container(overlay)
        .width(Length::Fill)
        .height(Length::Fill)
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
