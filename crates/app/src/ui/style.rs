//! Map configuration themes onto Iced widget styling.

use iced::theme::Palette;
use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme as IcedTheme};

use crate::config::Theme;
use crate::render::cells::{mix, to_color};

/// Build an Iced theme whose palette comes from the semantic theme tokens.
pub fn iced_theme(theme: &Theme) -> IcedTheme {
    let palette = Palette {
        background: to_color(theme.colors.background),
        text: to_color(theme.colors.foreground),
        primary: to_color(theme.colors.accent),
        success: to_color(theme.terminal.ansi[2]),
        warning: to_color(theme.terminal.ansi[3]),
        danger: to_color(theme.colors.error),
    };
    IcedTheme::custom(theme.name.clone(), palette)
}

/// Raised surface used by bars, panels and dialogs.
pub fn surface(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(to_color(theme.colors.surface))),
        text_color: Some(to_color(theme.colors.foreground)),
        border: Border {
            color: border_color(theme),
            width: theme.style.border_width,
            radius: theme.style.corner_radius.into(),
        },
        shadow: glow_shadow(theme),
        ..container::Style::default()
    }
}

/// Soft accent shadow for themes whose `[style]` requests a glow.
fn glow_shadow(theme: &Theme) -> iced::Shadow {
    if theme.style.glow <= 0.0 {
        return iced::Shadow::default();
    }
    iced::Shadow {
        color: Color {
            a: 0.35 * theme.style.glow,
            ..to_color(theme.colors.accent)
        },
        offset: iced::Vector::ZERO,
        blur_radius: 12.0 * theme.style.glow,
    }
}

/// Flat bar without rounded corners (tab strip, status bar).
pub fn bar(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(to_color(theme.colors.surface))),
        text_color: Some(to_color(theme.colors.foreground)),
        ..container::Style::default()
    }
}

/// Modal dialog with an accent edge.
pub fn dialog(theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: to_color(theme.colors.accent),
            width: theme.style.border_width.max(1.0),
            radius: (theme.style.corner_radius + 2.0).into(),
        },
        ..surface(theme)
    }
}

/// Dimmed backdrop behind modals.
pub fn backdrop() -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.45,
            ..Color::BLACK
        })),
        ..container::Style::default()
    }
}

pub fn border_color(theme: &Theme) -> Color {
    mix(
        to_color(theme.colors.surface),
        to_color(theme.colors.muted),
        0.35,
    )
}

/// Tab button: active tabs use the accent underline colour as their border.
pub fn tab(
    theme: &Theme,
    active: bool,
) -> impl Fn(&IcedTheme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let base = to_color(theme.colors.surface);
        let background = match (active, status) {
            (true, _) => to_color(theme.colors.background),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                mix(base, to_color(theme.colors.muted), 0.15)
            }
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: if active {
                to_color(theme.colors.foreground)
            } else {
                to_color(theme.colors.muted)
            },
            border: Border {
                color: if active {
                    to_color(theme.colors.accent)
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// List row in the palette; the selected row is highlighted.
pub fn list_row(
    theme: &Theme,
    selected: bool,
) -> impl Fn(&IcedTheme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = if selected || hovered {
            Some(Background::Color(to_color(theme.colors.selection)))
        } else {
            None
        };
        button::Style {
            background,
            text_color: to_color(theme.colors.foreground),
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}
