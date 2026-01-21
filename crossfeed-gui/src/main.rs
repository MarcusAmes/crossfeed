mod app;
mod menu;
mod project_picker;
mod project_settings;
mod theme;
mod timeline;

fn main() -> iced::Result {
    iced::application(app::APP_NAME, app::AppState::update, app::AppState::view)
        .subscription(app::AppState::subscription)
        .theme(app::AppState::theme)
        .run_with(app::AppState::new)
}
