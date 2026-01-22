use iced::widget::{
    PaneGrid, button, column, container, pane_grid, scrollable, text, text_input,
};
use iced::{Element, Length, Theme};
use serde::{Deserialize, Serialize};

use crate::app::Message;
use crate::theme::{ThemePalette, pane_border_style, text_input_style, text_muted, text_primary};
use crate::ui::panes::{response_preview_from_bytes, response_preview_placeholder};

#[derive(Debug, Clone)]
pub struct ReplayState {
    panes: pane_grid::State<ReplayPaneKind>,
    requests: Vec<String>,
    selected: Option<usize>,
    request_body: String,
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
            request_body: "GET /api/example\nHost: example.com\n\n".to_string(),
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
        .on_drag(Message::ReplayPaneDragged)
        .on_resize(10, Message::ReplayPaneResized);

        container(grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn request_list_view(&self, theme: ThemePalette) -> Element<'_, Message> {
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

        scrollable(list).into()
    }

    fn request_editor_view(&self, theme: ThemePalette) -> Element<'_, Message> {
        let editor = text_input("Request details", &self.request_body)
            .on_input(Message::ReplayUpdateDetails)
            .padding(8)
            .style({
                let theme = theme;
                move |_theme, status| text_input_style(theme, status)
            })
            .size(14);

        let content = column![text_muted("Request editor", 12, theme), editor]
            .spacing(6)
            .height(Length::Fill);

        container(content).height(Length::Fill).into()
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
            self.request_body = format!(
                "GET /replay/{}\nHost: example.com\n\n",
                label.to_lowercase().replace(' ', "-")
            );
        }
    }

    pub fn update_request_body(&mut self, body: String) {
        self.request_body = body;
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
