use iced::widget::{button, container, text, tooltip};
use iced::{Alignment, Element, Length};

use crate::theme::{
    ThemePalette, menu_button_style, menu_item_button_style, menu_panel_style,
};

pub const MENU_HEIGHT: f32 = 36.0;
pub const MENU_BUTTON_WIDTH: f32 = 96.0;
pub const MENU_SPACING: f32 = 8.0;
pub const MENU_PADDING_X: f32 = 8.0;
pub const MENU_PADDING_Y: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    File,
    Edit,
    View,
    Help,
}

#[derive(Debug, Clone)]
pub struct MenuItem<Message> {
    pub label: &'static str,
    pub message: Option<Message>,
    pub enabled: bool,
    pub tooltip: Option<String>,
}

pub fn menu_offset(menu: MenuKind) -> f32 {
    let index = match menu {
        MenuKind::File => 0.0,
        MenuKind::Edit => 1.0,
        MenuKind::View => 2.0,
        MenuKind::Help => 3.0,
    };
    MENU_PADDING_X + index * (MENU_BUTTON_WIDTH + MENU_SPACING)
}

pub fn menu_action_button<Message: Clone + 'static>(
    label: &'static str,
    on_press: Message,
    active: bool,
    theme: ThemePalette,
) -> iced::widget::Button<'static, Message> {
    let label = container(text(label).size(14).color(theme.text))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);
    button(label)
        .on_press(on_press)
        .width(Length::Fixed(MENU_BUTTON_WIDTH))
        .height(Length::Fixed(MENU_HEIGHT - 2.0 * MENU_PADDING_Y))
        .padding(0)
        .style(move |_theme, status| menu_button_style(theme, status, active))
}

pub fn menu_panel<Message: Clone + 'static>(
    items: Vec<MenuItem<Message>>,
    theme: &ThemePalette,
) -> Element<'static, Message> {
    let mut content = iced::widget::Column::new().spacing(6);
    for item in items {
        let theme = *theme;
        let label_color = if item.enabled {
            theme.text
        } else {
            theme.muted_text
        };
        let label = text(item.label).size(12).color(label_color);
        let mut button = button(label)
            .padding([4, 10])
            .width(Length::Fill)
            .style(move |_theme, status| menu_item_button_style(theme, status, item.enabled));
        if let Some(message) = item.message.clone() {
            button = button.on_press(message);
        }
        let element: Element<'static, Message> = if let Some(tooltip_text) = item.tooltip.clone() {
            let tooltip_label = container(text(tooltip_text).size(12).color(theme.text))
                .padding(6)
                .style({
                    let theme = theme;
                    move |_| menu_panel_style(theme)
                });
            tooltip(button, tooltip_label, iced::widget::tooltip::Position::Bottom).into()
        } else {
            button.into()
        };
        content = content.push(element);
    }
    container(content)
        .padding(8)
        .width(Length::Fixed(200.0))
        .style({
            let theme = *theme;
            move |_| menu_panel_style(theme)
        })
        .into()
}

pub fn menu_panel_text<Message: 'static>(
    text_value: &'static str,
    theme: &ThemePalette,
) -> Element<'static, Message> {
    let label = text(text_value)
        .size(12)
        .style({
            let theme = *theme;
            move |_theme: &iced::Theme| iced::widget::text::Style {
                color: Some(theme.muted_text),
            }
        });
    container(label)
        .padding(8)
        .width(Length::Fixed(200.0))
        .style({
            let theme = *theme;
            move |_| menu_panel_style(theme)
        })
        .into()
}
