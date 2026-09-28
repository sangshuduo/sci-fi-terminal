//! On-screen keyboard state: sticky modifiers, caps lock and key-press
//! translation into the same [`KeyPress`] events a physical keyboard produces.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use super::layout::{KeyAction, KeySpec, Layout, Modifier};
use crate::ui::input::KeyPress;

/// Messages emitted by the on-screen keyboard view.
#[derive(Debug, Clone)]
pub enum OskMessage {
    /// A key at (row, column) was pressed.
    Press(usize, usize),
}

/// On-screen keyboard: a layout plus one-shot sticky modifiers and caps lock.
#[derive(Debug, Clone)]
pub struct Keyboard {
    pub layout: Layout,
    shift: bool,
    ctrl: bool,
    alt: bool,
    caps: bool,
}

/// True when `text` has distinct upper/lower case forms (so caps applies).
fn is_cased(text: &str) -> bool {
    text.chars().any(char::is_alphabetic) && text.to_uppercase() != text.to_lowercase()
}

impl Keyboard {
    /// Creates a keyboard with no modifiers active.
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            shift: false,
            ctrl: false,
            alt: false,
            caps: false,
        }
    }

    /// Applies a press. Modifier keys toggle a one-shot sticky modifier that is
    /// cleared after the next non-modifier key; caps toggles caps lock (letters
    /// only). Returns `None` for modifier/caps presses or out-of-range indices.
    pub fn press(&mut self, row: usize, col: usize) -> Option<KeyPress> {
        let action = self.layout.rows.get(row)?.get(col)?.action.clone();
        let press = match action {
            KeyAction::Modifier(m) => {
                self.toggle(m);
                return None;
            }
            KeyAction::ToggleCaps => {
                self.caps = !self.caps;
                return None;
            }
            KeyAction::Text { base, shifted } => self.text_press(&base, shifted.as_deref()),
            KeyAction::Named(named) => self.named_press(named),
        };
        self.clear_sticky();
        Some(press)
    }

    /// Whether a sticky modifier is currently active.
    pub fn is_active(&self, m: Modifier) -> bool {
        match m {
            Modifier::Shift => self.shift,
            Modifier::Ctrl => self.ctrl,
            Modifier::Alt => self.alt,
        }
    }

    /// Whether caps lock is on.
    pub fn caps(&self) -> bool {
        self.caps
    }

    /// Label to display for a key given current shift/caps state. Custom
    /// labels are shown as-is; default text labels follow the typed output.
    pub fn label(&self, key: &KeySpec) -> String {
        match &key.action {
            KeyAction::Text { base, shifted } if key.label == *base => {
                self.output_text(base, shifted.as_deref())
            }
            _ => key.label.clone(),
        }
    }

    fn toggle(&mut self, m: Modifier) {
        match m {
            Modifier::Shift => self.shift = !self.shift,
            Modifier::Ctrl => self.ctrl = !self.ctrl,
            Modifier::Alt => self.alt = !self.alt,
        }
    }

    fn clear_sticky(&mut self) {
        self.shift = false;
        self.ctrl = false;
        self.alt = false;
    }

    fn modifiers(&self) -> Modifiers {
        let mut mods = Modifiers::empty();
        mods.set(Modifiers::SHIFT, self.shift);
        mods.set(Modifiers::CTRL, self.ctrl);
        mods.set(Modifiers::ALT, self.alt);
        mods
    }

    /// Text a key types now: shift (xor caps for letters) selects the
    /// shifted form, falling back to the uppercase of a cased base.
    fn output_text(&self, base: &str, shifted: Option<&str>) -> String {
        let cased = is_cased(base);
        let upper = if cased {
            self.shift != self.caps
        } else {
            self.shift
        };
        match (upper, shifted) {
            (false, _) => base.to_owned(),
            (true, Some(s)) => s.to_owned(),
            (true, None) if cased => base.to_uppercase(),
            (true, None) => base.to_owned(),
        }
    }

    fn text_press(&self, base: &str, shifted: Option<&str>) -> KeyPress {
        let text = self.output_text(base, shifted);
        // Ctrl+Alt with committed text reads as AltGr to the encoder; send
        // the bare key so both modifiers reach the terminal.
        let text = (!(self.ctrl && self.alt)).then_some(text);
        KeyPress {
            key: Key::Character(base.to_lowercase().into()),
            modifiers: self.modifiers(),
            text,
        }
    }

    fn named_press(&self, named: Named) -> KeyPress {
        let text = (named == Named::Space).then(|| " ".to_owned());
        KeyPress {
            key: Key::Named(named),
            modifiers: self.modifiers(),
            text,
        }
    }
}
