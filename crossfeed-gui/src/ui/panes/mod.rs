pub mod request_details;
pub mod request_list;
pub mod response_preview;

pub use request_details::timeline_request_details_view;
pub use request_list::timeline_request_list_view;
pub use response_preview::{
    response_preview_from_bytes, response_preview_placeholder,
};

use serde::{Deserialize, Serialize};
use iced::widget::{container, scrollable, text};
use iced::{Element, Length};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneModuleKind {
    RequestList,
    RequestDetails,
    ResponsePreview,
    ReplayList,
    ReplayEditor,
}

impl PaneModuleKind {
    pub fn title(self) -> &'static str {
        match self {
            PaneModuleKind::RequestList => "Request List",
            PaneModuleKind::RequestDetails => "Request Details",
            PaneModuleKind::ResponsePreview => "Response Preview",
            PaneModuleKind::ReplayList => "Replay Requests",
            PaneModuleKind::ReplayEditor => "Replay Editor",
        }
    }
}

pub fn pane_root<'a, Message: 'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub fn pane_scroll<'a, Message: 'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    container(scrollable(content))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub fn pane_text_editor<'a, Message: 'a>(
    editor: iced::widget::TextEditor<'a, impl text::Highlighter, Message, iced::Theme, iced::Renderer>,
) -> Element<'a, Message> {
    pane_root(container(editor).padding(12).width(Length::Fill).height(Length::Fill).into())
}

pub fn format_bytes(bytes: usize, truncated: bool) -> String {
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
