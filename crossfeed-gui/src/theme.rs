use std::path::PathBuf;

use iced::widget::text_input;
use iced::{Background, Color, Theme};
use serde::{Deserialize, Serialize};

pub const THEME_FILENAME: &str = "theme.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub background: String,
    pub surface: String,
    pub header: String,
    pub text: String,
    pub muted_text: String,
    pub border: String,
    pub accent: String,
    pub danger: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            background: "#282828".to_string(),
            surface: "#3c3836".to_string(),
            header: "#504945".to_string(),
            text: "#ebdbb2".to_string(),
            muted_text: "#bdae93".to_string(),
            border: "#665c54".to_string(),
            accent: "#d79921".to_string(),
            danger: "#cc241d".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub background: Color,
    pub surface: Color,
    pub header: Color,
    pub text: Color,
    pub muted_text: Color,
    pub border: Color,
    pub accent: Color,
    pub danger: Color,
}

impl ThemePalette {
    pub fn from_config(config: ThemeConfig) -> Self {
        let fallback = ThemePalette::default();
        Self {
            background: parse_hex_color(&config.background, fallback.background),
            surface: parse_hex_color(&config.surface, fallback.surface),
            header: parse_hex_color(&config.header, fallback.header),
            text: parse_hex_color(&config.text, fallback.text),
            muted_text: parse_hex_color(&config.muted_text, fallback.muted_text),
            border: parse_hex_color(&config.border, fallback.border),
            accent: parse_hex_color(&config.accent, fallback.accent),
            danger: parse_hex_color(&config.danger, fallback.danger),
        }
    }
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self {
            background: Color::from_rgb8(0x28, 0x28, 0x28),
            surface: Color::from_rgb8(0x3c, 0x38, 0x36),
            header: Color::from_rgb8(0x50, 0x49, 0x45),
            text: Color::from_rgb8(0xeb, 0xdb, 0xb2),
            muted_text: Color::from_rgb8(0xbd, 0xae, 0x93),
            border: Color::from_rgb8(0x66, 0x5c, 0x54),
            accent: Color::from_rgb8(0xd7, 0x99, 0x21),
            danger: Color::from_rgb8(0xcc, 0x24, 0x1d),
        }
    }
}

pub fn theme_config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("crossfeed").join(THEME_FILENAME)
}

pub async fn load_theme_config(path: PathBuf) -> Result<ThemeConfig, String> {
    if !path.exists() {
        let default_theme = ThemeConfig::default();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let raw = toml::to_string_pretty(&default_theme).map_err(|err| err.to_string())?;
        std::fs::write(path, raw).map_err(|err| err.to_string())?;
        return Ok(default_theme);
    }
    let contents = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    toml::from_str(&contents).map_err(|err| err.to_string())
}

pub fn parse_hex_color(value: &str, fallback: Color) -> Color {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 && value.len() != 8 {
        return fallback;
    }
    let parse_pair = |slice: &str| u8::from_str_radix(slice, 16).ok();
    let r = parse_pair(&value[0..2]);
    let g = parse_pair(&value[2..4]);
    let b = parse_pair(&value[4..6]);
    match (r, g, b) {
        (Some(r), Some(g), Some(b)) => {
            if value.len() == 8 {
                let a = parse_pair(&value[6..8]).unwrap_or(255);
                Color::from_rgba8(r, g, b, f32::from(a) / 255.0)
            } else {
                Color::from_rgb8(r, g, b)
            }
        }
        _ => fallback,
    }
}

pub fn menu_bar_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(theme.header)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

pub fn menu_button_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
    active: bool,
) -> iced::widget::button::Style {
    let base = if active { theme.header } else { theme.surface };
    let background = match status {
        iced::widget::button::Status::Hovered => theme.header,
        iced::widget::button::Status::Pressed => theme.accent,
        _ => base,
    };
    iced::widget::button::Style {
        text_color: theme.text,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

pub fn menu_panel_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(theme.text),
        background: Some(Background::Color(theme.surface)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: theme.border,
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
    }
}

pub fn menu_item_button_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
    enabled: bool,
) -> iced::widget::button::Style {
    let background = if !enabled {
        theme.surface
    } else {
        match status {
            iced::widget::button::Status::Hovered => theme.header,
            iced::widget::button::Status::Pressed => theme.accent,
            _ => theme.surface,
        }
    };
    let text_color = if enabled { theme.text } else { theme.muted_text };
    iced::widget::button::Style {
        text_color,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

pub fn pane_border_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(theme.surface)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

pub fn text_primary(
    value: impl Into<String>,
    size: u16,
    theme: ThemePalette,
) -> iced::widget::Text<'static> {
    iced::widget::text(value.into())
        .size(size)
        .style(move |_theme: &Theme| iced::widget::text::Style {
            color: Some(theme.text),
        })
}

pub fn text_muted(
    value: impl Into<String>,
    size: u16,
    theme: ThemePalette,
) -> iced::widget::Text<'static> {
    iced::widget::text(value.into())
        .size(size)
        .style(move |_theme: &Theme| iced::widget::text::Style {
            color: Some(theme.muted_text),
        })
}

pub fn text_danger(
    value: impl Into<String>,
    size: u16,
    theme: ThemePalette,
) -> iced::widget::Text<'static> {
    iced::widget::text(value.into())
        .size(size)
        .style(move |_theme: &Theme| iced::widget::text::Style {
            color: Some(theme.danger),
        })
}

pub fn action_button_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let background = match status {
        iced::widget::button::Status::Hovered => theme.header,
        iced::widget::button::Status::Pressed => theme.accent,
        _ => theme.surface,
    };
    iced::widget::button::Style {
        text_color: theme.text,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

pub fn action_button<Message: Clone + 'static>(
    label: &str,
    message: Message,
    theme: ThemePalette,
) -> iced::widget::Button<'static, Message> {
    iced::widget::button(text_primary(label, 12, theme))
        .on_press(message)
        .style(move |_theme, status| action_button_style(theme, status))
}

pub fn timeline_row_style(
    theme: ThemePalette,
    status: iced::widget::button::Status,
    selected: bool,
) -> iced::widget::button::Style {
    let base = if selected { theme.header } else { theme.surface };
    let background = match status {
        iced::widget::button::Status::Hovered => theme.header,
        iced::widget::button::Status::Pressed => theme.accent,
        _ => base,
    };
    iced::widget::button::Style {
        text_color: theme.text,
        background: Some(Background::Color(background)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

pub fn badge_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: Some(theme.text),
        background: Some(Background::Color(theme.header)),
        border: iced::border::Border {
            color: theme.border,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}

pub fn text_input_style(
    theme: ThemePalette,
    status: text_input::Status,
) -> text_input::Style {
    let border_color = match status {
        text_input::Status::Focused => theme.accent,
        text_input::Status::Hovered => theme.border,
        text_input::Status::Disabled => theme.border,
        text_input::Status::Active => theme.border,
    };
    text_input::Style {
        background: Background::Color(theme.surface),
        border: iced::border::Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        icon: theme.muted_text,
        placeholder: theme.muted_text,
        value: theme.text,
        selection: theme.accent,
    }
}

pub fn background_style(theme: ThemePalette) -> iced::widget::container::Style {
    iced::widget::container::Style {
        text_color: None,
        background: Some(Background::Color(theme.background)),
        border: iced::border::Border {
            color: theme.border,
            width: 0.0,
            radius: 0.0.into(),
        },
        shadow: iced::Shadow::default(),
    }
}
