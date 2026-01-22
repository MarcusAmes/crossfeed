use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossfeed_storage::{ReplayCollection, ReplayRequest, ReplayVersion, TimelineResponse};
use iced::mouse;
use iced::widget::{
    PaneGrid, button, column, container, mouse_area, pane_grid, text, text_editor,
};
use iced::{Element, Length, Theme};
use iced::font::Weight;
use iced::widget::text_editor::Content;
use serde::{Deserialize, Serialize};

use crate::app::{Message, ReplayDropTarget};
use crate::theme::{
    ThemePalette, pane_border_style, replay_collection_header_style, replay_row_style,
    text_editor_style, text_muted, text_primary,
};
use crate::ui::panes::{
    pane_scroll, pane_text_editor, response_preview_from_bytes, response_preview_placeholder,
};

#[derive(Debug)]
pub struct ReplayState {
    panes: pane_grid::State<ReplayPaneKind>,
    store_path: Option<PathBuf>,
    collections: Vec<ReplayCollection>,
    requests_by_collection: HashMap<Option<i64>, Vec<ReplayRequest>>,
    collapsed_collections: HashSet<i64>,
    selected_request_id: Option<i64>,
    latest_response: Option<TimelineResponse>,
    active_version: Option<ReplayVersion>,
    editor_content: Content,
}

impl Default for ReplayState {
    fn default() -> Self {
        let mut state = Self {
            panes: pane_grid::State::new(ReplayPaneKind::List).0,
            store_path: None,
            collections: Vec::new(),
            requests_by_collection: HashMap::new(),
            collapsed_collections: HashSet::new(),
            selected_request_id: None,
            latest_response: None,
            active_version: None,
            editor_content: Content::with_text("GET /api/example\nHost: example.com\n\n"),
        };
        state.apply_layout(default_replay_layout());
        state
    }
}

impl ReplayState {
    pub fn view(&self, theme: &ThemePalette) -> Element<'_, Message> {
        let grid = PaneGrid::new(&self.panes, |_, state, _| {
            let pane_content: Element<'_, Message> = match state {
                ReplayPaneKind::List => self.request_list_view(*theme),
                ReplayPaneKind::Editor => self.request_editor_view(*theme),
                ReplayPaneKind::Response => self.response_view(*theme),
            };
            let content = container(pane_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style({
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
        .on_drag(Message::ReplayPaneDragged)
        .on_resize(10, Message::ReplayPaneResized);

        container(grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub(crate) fn request_list_view(&self, theme: ThemePalette) -> Element<'_, Message> {
        let mut list = column![].spacing(8);
        for collection in &self.collections {
            let is_collapsed = self.collapsed_collections.contains(&collection.id);
            let header_label = if is_collapsed {
                format!("{} ▸", collection.name)
            } else {
                format!("{} ▾", collection.name)
            };
            let header = button(
                text(header_label)
                    .size(13)
                    .style({
                        let theme = theme;
                        move |_theme: &Theme| iced::widget::text::Style {
                            color: Some(theme.text),
                        }
                    })
                    .font(iced::Font {
                        weight: Weight::Bold,
                        ..iced::Font::default()
                    }),
            )
                .on_press(Message::ReplayToggleCollection(collection.id))
                .padding([4, 6])
                .style({
                    let theme = theme;
                    let is_open = !is_collapsed;
                    let color = collection.color.clone();
                    move |_theme, status| {
                        replay_collection_header_style(theme, status, is_open, color.as_deref())
                    }
                });
            let header_area = mouse_area(header)
                .on_enter(Message::ReplayDragHover(ReplayDropTarget::Collection {
                    collection_id: Some(collection.id),
                }))
                .on_right_press(Message::ReplayCollectionMenuOpen(collection.id))
                .interaction(mouse::Interaction::Pointer);
            list = list.push(header_area);

            if !is_collapsed {
                if let Some(requests) = self.requests_by_collection.get(&Some(collection.id)) {
                    for request in requests {
                        list = list.push(self.request_row(request, theme));
                    }
                }
            }
        }

        if let Some(requests) = self.requests_by_collection.get(&None) {
            let header = button(
                text("Uncategorized ▾")
                    .size(13)
                    .style({
                        let theme = theme;
                        move |_theme: &Theme| iced::widget::text::Style {
                            color: Some(theme.text),
                        }
                    })
                    .font(iced::Font {
                        weight: Weight::Bold,
                        ..iced::Font::default()
                    }),
            )
                .on_press(Message::ReplayToggleCollection(-1))
                .padding([4, 6])
                .style({
                    let theme = theme;
                    move |_theme, status| replay_collection_header_style(theme, status, true, None)
                });
            let header_area = mouse_area(header)
                .on_enter(Message::ReplayDragHover(ReplayDropTarget::Collection {
                    collection_id: None,
                }))
                .interaction(mouse::Interaction::Pointer);
            list = list.push(header_area);
            for request in requests {
                list = list.push(self.request_row(request, theme));
            }
        }

        let content = pane_scroll(list.into());
        mouse_area(content)
            .on_move(Message::ReplayListCursor)
            .on_release(Message::ReplayDragEnd)
            .on_exit(Message::ReplayDragHoverClear)
            .interaction(mouse::Interaction::Pointer)
            .into()
    }

    pub(crate) fn request_editor_view(&self, theme: ThemePalette) -> Element<'_, Message> {
        let editor = text_editor(&self.editor_content)
            .on_action(Message::ReplayUpdateDetails)
            .size(14)
            .width(1600.0)
            .height(Length::Fill)
            .style({
                let theme = theme;
                move |_theme, status| text_editor_style(theme, status)
            });

        pane_text_editor(editor)
    }

    fn response_view(&self, theme: ThemePalette) -> Element<'_, Message> {
        let response = match &self.latest_response {
            Some(response) => response,
            None => {
                return response_preview_placeholder("No replay execution yet", theme);
            }
        };
        let status_line = response
            .reason
            .clone()
            .map(|reason| format!("{} {reason}", response.status_code))
            .unwrap_or_else(|| response.status_code.to_string());
        response_preview_from_bytes(
            status_line,
            &response.response_headers,
            &response.response_body,
            response.response_body_truncated,
            theme,
        )
    }

    pub fn select(&mut self, request_id: i64) {
        self.selected_request_id = Some(request_id);
    }

    pub fn apply_editor_action(&mut self, action: text_editor::Action) {
        self.editor_content.perform(action);
    }

    pub fn set_store_path(&mut self, path: PathBuf) {
        self.store_path = Some(path);
    }

    pub fn store_path(&self) -> Option<&PathBuf> {
        self.store_path.as_ref()
    }

    pub fn set_replay_data(
        &mut self,
        collections: Vec<ReplayCollection>,
        requests_by_collection: HashMap<Option<i64>, Vec<ReplayRequest>>,
    ) {
        self.collections = collections;
        self.requests_by_collection = requests_by_collection;
    }

    pub fn collections(&self) -> &[ReplayCollection] {
        &self.collections
    }

    pub fn collection_name(&self, collection_id: i64) -> Option<String> {
        self.collections
            .iter()
            .find(|collection| collection.id == collection_id)
            .map(|collection| collection.name.clone())
    }

    pub fn requests_in_collection(&self, collection_id: Option<i64>) -> Option<&Vec<ReplayRequest>> {
        self.requests_by_collection.get(&collection_id)
    }

    pub fn toggle_collection(&mut self, collection_id: i64) {
        if collection_id == -1 {
            return;
        }
        if self.collapsed_collections.contains(&collection_id) {
            self.collapsed_collections.remove(&collection_id);
        } else {
            self.collapsed_collections.insert(collection_id);
        }
    }

    pub fn set_active_version(&mut self, version: Option<ReplayVersion>) {
        self.active_version = version.clone();
        if let Some(version) = version {
            let headers = String::from_utf8_lossy(&version.request_headers)
                .replace("\r\n", "\n")
                .trim_end()
                .to_string();
            let body = String::from_utf8_lossy(&version.request_body).to_string();
            let combined = if body.is_empty() {
                headers
            } else if headers.is_empty() {
                body
            } else {
                format!("{headers}\n\n{body}")
            };
            self.editor_content = Content::with_text(&combined);
        }
    }

    pub fn set_latest_response(&mut self, response: Option<TimelineResponse>) {
        self.latest_response = response;
    }

    pub fn request_row(&self, request: &ReplayRequest, theme: ThemePalette) -> Element<'_, Message> {
        let is_selected = self.selected_request_id == Some(request.id);
        let label = if is_selected {
            text_primary(request.name.clone(), 14, theme)
        } else {
            text_muted(request.name.clone(), 14, theme)
        };
        let row = button(label)
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_theme, status| replay_row_style(theme, status, is_selected));
        mouse_area(row)
            .on_press(Message::ReplayDragStart(request.id, request.collection_id))
            .on_right_press(Message::ReplayContextMenuOpen(request.id))
            .on_enter(Message::ReplayDragHover(ReplayDropTarget::Request {
                request_id: request.id,
                collection_id: request.collection_id,
            }))
            .interaction(mouse::Interaction::Pointer)
            .into()
    }

    pub fn request_name(&self, request_id: i64) -> Option<String> {
        for requests in self.requests_by_collection.values() {
            if let Some(request) = requests.iter().find(|request| request.id == request_id) {
                return Some(request.name.clone());
            }
        }
        None
    }

    pub fn snapshot_layout(&self) -> ReplayLayout {
        ReplayLayout::from(&self.panes)
    }

    pub fn apply_layout(&mut self, layout: ReplayLayout) {
        self.panes = pane_grid::State::with_configuration(layout.to_configuration());
    }

    pub fn handle_pane_drag(&mut self, event: pane_grid::DragEvent) -> Option<ReplayLayout> {
        match event {
            pane_grid::DragEvent::Dropped { pane, target } => {
                self.panes.drop(pane, target);
                Some(self.snapshot_layout())
            }
            pane_grid::DragEvent::Picked { .. } => None,
            pane_grid::DragEvent::Canceled { .. } => None,
        }
    }

    pub fn handle_pane_resize(&mut self, event: pane_grid::ResizeEvent) -> Option<ReplayLayout> {
        self.panes.resize(event.split, event.ratio);
        Some(self.snapshot_layout())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReplayPaneKind {
    List,
    Editor,
    Response,
}

impl ReplayPaneKind {
    fn title(self) -> &'static str {
        match self {
            ReplayPaneKind::List => "Replay Requests",
            ReplayPaneKind::Editor => "Replay Editor",
            ReplayPaneKind::Response => "Replay Response",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayLayout {
    root: ReplayLayoutNode,
}

impl ReplayLayout {
    pub fn from(state: &pane_grid::State<ReplayPaneKind>) -> Self {
        Self {
            root: ReplayLayoutNode::from(state.layout(), state),
        }
    }

    pub fn to_configuration(&self) -> pane_grid::Configuration<ReplayPaneKind> {
        self.root.to_configuration()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ReplayLayoutNode {
    Pane(ReplayPaneKind),
    Split {
        axis: ReplayLayoutAxis,
        ratio: f32,
        a: Box<ReplayLayoutNode>,
        b: Box<ReplayLayoutNode>,
    },
}

impl ReplayLayoutNode {
    fn from(node: &pane_grid::Node, panes: &pane_grid::State<ReplayPaneKind>) -> Self {
        match node {
            pane_grid::Node::Pane(pane) => {
                let kind = panes.get(*pane).copied().unwrap_or(ReplayPaneKind::List);
                ReplayLayoutNode::Pane(kind)
            }
            pane_grid::Node::Split { axis, ratio, a, b, .. } => ReplayLayoutNode::Split {
                axis: ReplayLayoutAxis::from(*axis),
                ratio: *ratio,
                a: Box::new(ReplayLayoutNode::from(a, panes)),
                b: Box::new(ReplayLayoutNode::from(b, panes)),
            },
        }
    }

    fn to_configuration(&self) -> pane_grid::Configuration<ReplayPaneKind> {
        match self {
            ReplayLayoutNode::Pane(pane) => pane_grid::Configuration::Pane(*pane),
            ReplayLayoutNode::Split { axis, ratio, a, b } => {
                pane_grid::Configuration::Split {
                    axis: axis.to_axis(),
                    ratio: *ratio,
                    a: Box::new(a.to_configuration()),
                    b: Box::new(b.to_configuration()),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ReplayLayoutAxis {
    Horizontal,
    Vertical,
}

impl ReplayLayoutAxis {
    fn from(axis: pane_grid::Axis) -> Self {
        match axis {
            pane_grid::Axis::Horizontal => ReplayLayoutAxis::Horizontal,
            pane_grid::Axis::Vertical => ReplayLayoutAxis::Vertical,
        }
    }

    fn to_axis(self) -> pane_grid::Axis {
        match self {
            ReplayLayoutAxis::Horizontal => pane_grid::Axis::Horizontal,
            ReplayLayoutAxis::Vertical => pane_grid::Axis::Vertical,
        }
    }
}

pub fn default_replay_layout() -> ReplayLayout {
    ReplayLayout {
        root: ReplayLayoutNode::Split {
            axis: ReplayLayoutAxis::Vertical,
            ratio: 0.15,
            a: Box::new(ReplayLayoutNode::Pane(ReplayPaneKind::List)),
            b: Box::new(ReplayLayoutNode::Split {
                axis: ReplayLayoutAxis::Vertical,
                ratio: 0.5,
                a: Box::new(ReplayLayoutNode::Pane(ReplayPaneKind::Editor)),
                b: Box::new(ReplayLayoutNode::Pane(ReplayPaneKind::Response)),
            }),
        },
    }
}
