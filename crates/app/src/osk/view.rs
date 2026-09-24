//! On-screen keyboard view: rows of proportionally sized touch buttons.

use iced::widget::{Column, Row, button, text};
use iced::{Element, Length};

use super::layout::{KeyAction, KeySpec};
use super::state::{Keyboard, OskMessage};

/// Minimum key height in pixels, sized for touch.
const KEY_HEIGHT: f32 = 40.0;
const KEY_SPACING: f32 = 4.0;
const LABEL_SIZE: f32 = 14.0;
/// Width units are multiplied by this to get integer fill portions.
const PORTION_SCALE: f32 = 10.0;

fn portion(width: f32) -> u16 {
    // Width is validated to 0.5..=8.0, so this stays well inside u16.
    (width * PORTION_SCALE)
        .round()
        .clamp(1.0, f32::from(u16::MAX)) as u16
}

fn is_highlighted(keyboard: &Keyboard, key: &KeySpec) -> bool {
    match key.action {
        KeyAction::Modifier(m) => keyboard.is_active(m),
        KeyAction::ToggleCaps => keyboard.caps(),
        _ => false,
    }
}

fn key_button<'a, M: Clone + 'a>(
    keyboard: &'a Keyboard,
    key: &'a KeySpec,
    message: M,
) -> Element<'a, M> {
    let label = text(keyboard.label(key)).size(LABEL_SIZE).center();
    let style = if is_highlighted(keyboard, key) {
        button::primary
    } else {
        button::secondary
    };
    button(label)
        .width(Length::FillPortion(portion(key.width)))
        .height(Length::Fixed(KEY_HEIGHT))
        .style(style)
        .on_press(message)
        .into()
}

/// Rows of buttons sized by each key's `width`. Active modifiers and caps lock
/// are highlighted. Generic over the application message via `wrap`.
pub fn view<'a, M: Clone + 'a>(
    keyboard: &'a Keyboard,
    wrap: impl Fn(OskMessage) -> M + Copy + 'a,
) -> Element<'a, M> {
    let rows = keyboard.layout.rows.iter().enumerate().map(|(r, keys)| {
        let buttons = keys
            .iter()
            .enumerate()
            .map(|(c, key)| key_button(keyboard, key, wrap(OskMessage::Press(r, c))));
        Row::with_children(buttons)
            .spacing(KEY_SPACING)
            .width(Length::Fill)
            .into()
    });
    Column::with_children(rows)
        .spacing(KEY_SPACING)
        .padding(KEY_SPACING)
        .width(Length::Fill)
        .into()
}
