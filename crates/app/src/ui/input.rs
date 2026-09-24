//! Translate toolkit keyboard events into chords and terminal input.
//!
//! Routing order (enforced by the caller): active modal → application
//! shortcuts in scope → focused widget → terminal encoding.

use iced::keyboard::{Key, Modifiers, key::Named};
use terminal_core::ModeFlags;
use terminal_core::input::{self, Key as TermKey, Modifiers as TermMods};

use super::actions::Chord;

/// A key press as delivered by the toolkit, reduced to what routing needs.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyPress {
    /// Logical key without modifiers applied.
    pub key: Key,
    pub modifiers: Modifiers,
    /// Committed text (includes dead-key compositions), if any.
    pub text: Option<String>,
}

/// Chord for shortcut lookup, or `None` for keys that cannot be bound.
pub fn chord(press: &KeyPress) -> Option<Chord> {
    let key = match press.key.as_ref() {
        Key::Character(c) => c.to_lowercase(),
        Key::Named(named) => named_key(&named)?.to_owned(),
        Key::Unidentified => return None,
    };
    Some(Chord {
        ctrl: press.modifiers.control(),
        shift: press.modifiers.shift(),
        alt: press.modifiers.alt(),
        logo: press.modifiers.logo(),
        key,
    })
}

fn named_key(named: &Named) -> Option<&'static str> {
    Some(match named {
        Named::Enter => "enter",
        Named::Tab => "tab",
        Named::Space => "space",
        Named::Backspace => "backspace",
        Named::Escape => "escape",
        Named::Delete => "delete",
        Named::Insert => "insert",
        Named::Home => "home",
        Named::End => "end",
        Named::PageUp => "pageup",
        Named::PageDown => "pagedown",
        Named::ArrowUp => "up",
        Named::ArrowDown => "down",
        Named::ArrowLeft => "left",
        Named::ArrowRight => "right",
        Named::F1 => "f1",
        Named::F2 => "f2",
        Named::F3 => "f3",
        Named::F4 => "f4",
        Named::F5 => "f5",
        Named::F6 => "f6",
        Named::F7 => "f7",
        Named::F8 => "f8",
        Named::F9 => "f9",
        Named::F10 => "f10",
        Named::F11 => "f11",
        Named::F12 => "f12",
        _ => return None,
    })
}

fn terminal_named(named: &Named) -> Option<TermKey> {
    Some(match named {
        Named::Enter => TermKey::Enter,
        Named::Tab => TermKey::Tab,
        Named::Backspace => TermKey::Backspace,
        Named::Escape => TermKey::Escape,
        Named::Delete => TermKey::Delete,
        Named::Insert => TermKey::Insert,
        Named::Home => TermKey::Home,
        Named::End => TermKey::End,
        Named::PageUp => TermKey::PageUp,
        Named::PageDown => TermKey::PageDown,
        Named::ArrowUp => TermKey::Up,
        Named::ArrowDown => TermKey::Down,
        Named::ArrowLeft => TermKey::Left,
        Named::ArrowRight => TermKey::Right,
        Named::Space => TermKey::Text(" ".into()),
        Named::F1 => TermKey::F(1),
        Named::F2 => TermKey::F(2),
        Named::F3 => TermKey::F(3),
        Named::F4 => TermKey::F(4),
        Named::F5 => TermKey::F(5),
        Named::F6 => TermKey::F(6),
        Named::F7 => TermKey::F(7),
        Named::F8 => TermKey::F(8),
        Named::F9 => TermKey::F(9),
        Named::F10 => TermKey::F(10),
        Named::F11 => TermKey::F(11),
        Named::F12 => TermKey::F(12),
        _ => return None,
    })
}

/// Encode a key press for the terminal. `option_as_alt` controls macOS Option.
pub fn encode(press: &KeyPress, mode: ModeFlags, option_as_alt: bool) -> Option<Vec<u8>> {
    let mods = press.modifiers;
    // Command/Super combinations are application shortcuts, never terminal input.
    if mods.logo() {
        return None;
    }
    let committed = press
        .text
        .as_deref()
        .filter(|t| !t.is_empty() && !t.chars().all(char::is_control));
    let altgr = mods.control() && mods.alt() && committed.is_some();
    let alt_is_meta = mods.alt() && (option_as_alt || !cfg!(target_os = "macos"));
    let term_mods = TermMods {
        shift: mods.shift(),
        ctrl: mods.control() && !altgr,
        alt: alt_is_meta && !altgr,
    };
    let key = match press.key.as_ref() {
        Key::Named(named) => terminal_named(&named)?,
        Key::Character(c) if term_mods.ctrl || term_mods.alt => TermKey::Text(c.to_string()),
        Key::Character(c) => TermKey::Text(committed.unwrap_or(c).to_owned()),
        Key::Unidentified => TermKey::Text(committed?.to_owned()),
    };
    // Text keys already carry shift in their characters.
    let term_mods = match key {
        TermKey::Text(_) => TermMods {
            shift: false,
            ..term_mods
        },
        _ => term_mods,
    };
    input::encode_key(&key, term_mods, mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: Key, modifiers: Modifiers, text: Option<&str>) -> KeyPress {
        KeyPress {
            key,
            modifiers,
            text: text.map(str::to_owned),
        }
    }

    fn character(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn chords_use_unmodified_lowercase_keys() {
        let p = press(
            character("T"),
            Modifiers::CTRL | Modifiers::SHIFT,
            Some("T"),
        );
        assert_eq!(chord(&p), Chord::parse("Ctrl+Shift+T").ok());
        let named = press(Key::Named(Named::PageUp), Modifiers::SHIFT, None);
        assert_eq!(chord(&named), Chord::parse("Shift+PageUp").ok());
    }

    #[test]
    fn plain_text_is_sent_as_committed() {
        let p = press(character("a"), Modifiers::SHIFT, Some("A"));
        assert_eq!(encode(&p, ModeFlags::default(), false), Some(b"A".to_vec()));
    }

    #[test]
    fn ctrl_letters_become_control_codes() {
        let p = press(character("c"), Modifiers::CTRL, Some("\u{3}"));
        assert_eq!(encode(&p, ModeFlags::default(), false), Some(vec![3]));
    }

    #[test]
    fn logo_combinations_are_not_sent() {
        let p = press(character("c"), Modifiers::LOGO, Some("c"));
        assert_eq!(encode(&p, ModeFlags::default(), false), None);
    }

    #[test]
    fn altgr_text_is_sent_literally() {
        let p = press(character("q"), Modifiers::CTRL | Modifiers::ALT, Some("@"));
        assert_eq!(encode(&p, ModeFlags::default(), false), Some(b"@".to_vec()));
    }

    #[test]
    fn alt_prefixes_escape_when_meta() {
        let p = press(character("b"), Modifiers::ALT, Some("∫"));
        assert_eq!(
            encode(&p, ModeFlags::default(), true),
            Some(b"\x1bb".to_vec())
        );
    }

    #[test]
    fn arrows_respect_application_cursor_mode() {
        let p = press(Key::Named(Named::ArrowUp), Modifiers::empty(), None);
        let app = ModeFlags {
            app_cursor: true,
            ..ModeFlags::default()
        };
        assert_eq!(encode(&p, app, false), Some(b"\x1bOA".to_vec()));
    }

    #[test]
    fn dead_key_composition_sends_text_once() {
        let p = press(Key::Unidentified, Modifiers::empty(), Some("é"));
        assert_eq!(
            encode(&p, ModeFlags::default(), false),
            Some("é".as_bytes().to_vec())
        );
    }
}
