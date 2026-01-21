use iced::widget::{column, container, row, text_input};
use iced::{Alignment, Element, Length};

use crate::app::{Message, ProjectIntent};
use crate::theme::{
    ThemePalette, action_button, background_style, text_danger, text_input_style, text_muted,
    text_primary,
};

#[derive(Debug, Clone)]
pub struct ProjectPickerState {
    pub intent: ProjectIntent,
    pub error: Option<String>,
    pub pending_path: String,
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
    pub fn view(&self, theme: &ThemePalette) -> Element<'_, Message> {
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
