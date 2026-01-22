use std::collections::HashMap;
use std::path::PathBuf;

use crossfeed_ingest::{TailCursor, TailUpdate, TimelineItem};
use crossfeed_storage::{
    ProjectConfig, ProjectPaths, ResponseSummary, SqliteStore, TimelineQuery, TimelineSort,
};
use iced::widget::{PaneGrid, container, pane_grid, text};
use iced::{Element, Length, Theme};
use serde::{Deserialize, Serialize};

use crate::app::Message;
use crate::theme::{ThemePalette, pane_border_style};
use crate::ui::panes::{
    response_preview_from_bytes, response_preview_placeholder, timeline_request_details_view,
    timeline_request_list_view,
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
        timeline_request_list_view(
            &self.timeline,
            &self.tags,
            &self.responses,
            self.selected,
            theme,
        )
    }

    fn detail_view(&self, _focus: crate::app::FocusArea, theme: ThemePalette) -> Element<'_, Message> {
        let selected = self.selected.and_then(|idx| self.timeline.get(idx));
        let response = selected.and_then(|item| self.responses.get(&item.id));
        timeline_request_details_view(selected, response, theme)
    }

    fn response_view(&self, _focus: crate::app::FocusArea, theme: ThemePalette) -> Element<'_, Message> {
        if let Some(selected) = self.selected.and_then(|idx| self.timeline.get(idx)) {
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
                let status_line = response
                    .reason
                    .clone()
                    .map(|reason| format!("{} {reason}", response.status_code))
                    .unwrap_or_else(|| response.status_code.to_string());
                let truncated = timeline_response
                    .as_ref()
                    .map(|resp| resp.response_body_truncated)
                    .unwrap_or(false);
                response_preview_from_bytes(
                    status_line,
                    response_headers,
                    body,
                    truncated,
                    theme,
                )
            } else {
                response_preview_placeholder("No response recorded yet", theme)
            }
        } else {
            response_preview_placeholder("Select a request to preview response", theme)
        }
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

    pub fn snapshot_layout(&self) -> PaneLayout {
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

pub fn default_pane_layout() -> PaneLayout {
    let (mut panes, root) = pane_grid::State::new(PaneKind::Timeline);
    let (right, _) = panes
        .split(pane_grid::Axis::Vertical, root, PaneKind::Detail)
        .expect("Default timeline split failed");
    let _ = panes
        .split(pane_grid::Axis::Horizontal, right, PaneKind::Response)
        .expect("Default timeline split failed");
    PaneLayout::from(&panes)
}
