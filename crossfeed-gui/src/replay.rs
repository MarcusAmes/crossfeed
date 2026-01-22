use iced::widget::{
    PaneGrid, button, column, container, pane_grid, text, text_editor,
};
use iced::{Element, Length, Theme};
use iced::widget::text_editor::Content;
use serde::{Deserialize, Serialize};

use crate::app::Message;
use crate::theme::{ThemePalette, pane_border_style, text_editor_style, text_muted, text_primary};
use crate::ui::panes::{
    pane_scroll, pane_text_editor, response_preview_from_bytes, response_preview_placeholder,
};

#[derive(Debug)]
pub struct ReplayState {
    panes: pane_grid::State<ReplayPaneKind>,
    requests: Vec<String>,
    selected: Option<usize>,
    editor_content: Content,
}

impl Default for ReplayState {
    fn default() -> Self {
        let mut state = Self {
            panes: pane_grid::State::new(ReplayPaneKind::List).0,
            requests: vec![
                "Login flow".to_string(),
                "Search results".to_string(),
                "Checkout".to_string(),
            ],
            selected: Some(0),
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
        for (index, request) in self.requests.iter().enumerate() {
            let is_selected = self.selected == Some(index);
            let label = if is_selected {
                text_primary(request.clone(), 14, theme)
            } else {
                text_muted(request.clone(), 14, theme)
            };
            let row = button(label)
                .on_press(Message::ReplaySelect(index))
                .padding([4, 8]);
            list = list.push(row);
        }

        pane_scroll(list.into())
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
        if self.selected.is_none() {
            return response_preview_placeholder("Select a replay to preview response", theme);
        }
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n";
        let body = b"Replay response preview";
        response_preview_from_bytes(
            "200 OK".to_string(),
            headers,
            body,
            false,
            theme,
        )
    }

    pub fn select(&mut self, index: usize) {
        self.selected = Some(index);
        if let Some(label) = self.requests.get(index) {
            let body = format!(
                "GET /replay/{}\nHost: example.com\n\n",
                label.to_lowercase().replace(' ', "-")
            );
            self.editor_content = Content::with_text(&body);
        }
    }

    pub fn apply_editor_action(&mut self, action: text_editor::Action) {
        self.editor_content.perform(action);
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
