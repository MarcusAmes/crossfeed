use iced::widget::{column, container, row, text_input};
use iced::{Alignment, Element, Length};

use crate::app::Message;
use crate::theme::{
    ThemePalette, action_button, background_style, text_input_style, text_muted, text_primary,
};
use crate::timeline::TimelineState;
use crossfeed_storage::{ProjectConfig, ProjectPaths};

#[derive(Debug, Clone)]
pub struct ProjectSettingsState {
    pub timeline_state: TimelineState,
    pub project_paths: ProjectPaths,
    pub project_config: ProjectConfig,
    pub proxy_host: String,
    pub proxy_port: String,
}

impl ProjectSettingsState {
    pub fn from(state: &TimelineState) -> Self {
        Self {
            timeline_state: state.clone(),
            project_paths: state.project_paths.clone(),
            project_config: state.project_config.clone(),
            proxy_host: state.project_config.proxy.listen_host.clone(),
            proxy_port: state.project_config.proxy.listen_port.to_string(),
        }
    }

    pub fn view(&self, theme: &ThemePalette) -> Element<'_, Message> {
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
