//! Keyboard, paste and mouse encoding for the terminal input stream.
//!
//! Encodings follow the public xterm control-sequence reference for the
//! supported subset (legacy keys, modifier parameters, application cursor
//! and keypad modes, bracketed paste, X10/normal and SGR mouse reports).

use thiserror::Error;

use crate::snapshot::ModeFlags;

/// Logical key, independent of any GUI toolkit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Committed text (possibly from IME or dead keys); sent once, as-is.
    Text(String),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// Function key 1..=12.
    F(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    fn any(self) -> bool {
        self.shift || self.ctrl || self.alt
    }

    /// xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// Encode a key press for the child, or `None` if the key produces no bytes.
pub fn encode_key(key: &Key, mods: Modifiers, mode: ModeFlags) -> Option<Vec<u8>> {
    let bytes = match key {
        Key::Text(text) => return encode_text(text, mods),
        Key::Enter if mode.line_feed_new_line => b"\r\n".to_vec(),
        Key::Enter => b"\r".to_vec(),
        Key::Backspace if mods.ctrl => vec![0x08],
        Key::Backspace => vec![0x7f],
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => vec![0x1b],
        Key::Up => cursor_key(b'A', mods, mode),
        Key::Down => cursor_key(b'B', mods, mode),
        Key::Right => cursor_key(b'C', mods, mode),
        Key::Left => cursor_key(b'D', mods, mode),
        Key::Home => cursor_key(b'H', mods, mode),
        Key::End => cursor_key(b'F', mods, mode),
        Key::Insert => tilde_key(2, mods),
        Key::Delete => tilde_key(3, mods),
        Key::PageUp => tilde_key(5, mods),
        Key::PageDown => tilde_key(6, mods),
        Key::F(n) => return function_key(*n, mods),
    };
    let needs_alt_prefix = mods.alt && matches!(key, Key::Enter | Key::Backspace | Key::Escape);
    Some(if needs_alt_prefix {
        prefixed_escape(bytes)
    } else {
        bytes
    })
}

fn prefixed_escape(bytes: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(0x1b);
    out.extend(bytes);
    out
}

fn encode_text(text: &str, mods: Modifiers) -> Option<Vec<u8>> {
    if text.is_empty() {
        return None;
    }
    let mut chars = text.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    let bytes = match single {
        Some(c) if mods.ctrl => ctrl_char(c).map(|b| vec![b]).unwrap_or_else(|| text.into()),
        _ => text.as_bytes().to_vec(),
    };
    Some(if mods.alt {
        prefixed_escape(bytes)
    } else {
        bytes
    })
}

/// C0 control for Ctrl+<char>, per the conventional ASCII mapping.
fn ctrl_char(c: char) -> Option<u8> {
    let lower = c.to_ascii_lowercase();
    match lower {
        'a'..='z' => Some(lower as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '-' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

fn cursor_key(final_byte: u8, mods: Modifiers, mode: ModeFlags) -> Vec<u8> {
    if mods.any() {
        return format!("\x1b[1;{}{}", mods.param(), final_byte as char).into_bytes();
    }
    let intro = if mode.app_cursor { b'O' } else { b'[' };
    vec![0x1b, intro, final_byte]
}

fn tilde_key(code: u8, mods: Modifiers) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[{code};{}~", mods.param()).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn function_key(n: u8, mods: Modifiers) -> Option<Vec<u8>> {
    let ss3_final = match n {
        1 => Some(b'P'),
        2 => Some(b'Q'),
        3 => Some(b'R'),
        4 => Some(b'S'),
        _ => None,
    };
    if let Some(final_byte) = ss3_final {
        return Some(if mods.any() {
            format!("\x1b[1;{}{}", mods.param(), final_byte as char).into_bytes()
        } else {
            vec![0x1b, b'O', final_byte]
        });
    }
    let code = match n {
        5 => 15,
        6 => 17,
        7 => 18,
        8 => 19,
        9 => 20,
        10 => 21,
        11 => 23,
        12 => 24,
        _ => return None,
    };
    Some(tilde_key(code, mods))
}

/// Maximum explicit paste size.
pub const MAX_PASTE_BYTES: usize = 1024 * 1024;

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PasteError {
    #[error("paste of {size} bytes exceeds the {limit} byte limit")]
    TooLarge { size: usize, limit: usize },
    #[error("paste contains a bracketed-paste terminator and was not sent")]
    ContainsTerminator,
}

/// Why a paste deserves confirmation before sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PasteRisk {
    pub multiline: bool,
    pub control_chars: bool,
}

impl PasteRisk {
    pub fn needs_confirmation(self) -> bool {
        self.multiline || self.control_chars
    }
}

/// Inspect a paste without modifying it.
pub fn paste_risk(text: &str) -> PasteRisk {
    let body = text.trim_end_matches(['\n', '\r']);
    PasteRisk {
        multiline: body.contains(['\n', '\r']),
        control_chars: text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
    }
}

/// Encode a paste. Content is never silently rewritten: anything that would
/// have to be altered to be safe is rejected with an explanation instead.
pub fn encode_paste(text: &str, mode: ModeFlags) -> Result<Vec<u8>, PasteError> {
    if text.len() > MAX_PASTE_BYTES {
        return Err(PasteError::TooLarge {
            size: text.len(),
            limit: MAX_PASTE_BYTES,
        });
    }
    if !mode.bracketed_paste {
        return Ok(text.as_bytes().to_vec());
    }
    if contains(text.as_bytes(), PASTE_END) {
        return Err(PasteError::ContainsTerminator);
    }
    let mut out = Vec::with_capacity(text.len() + PASTE_START.len() + PASTE_END.len());
    out.extend_from_slice(PASTE_START);
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(PASTE_END);
    Ok(out)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Mouse buttons the encoder supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Drag,
}

/// Encode a mouse report for the current mode, or `None` if reporting is off
/// or the event is not requested by the application. `col`/`row` are 0-based.
pub fn encode_mouse(
    button: MouseButton,
    action: MouseAction,
    col: u16,
    row: u16,
    mods: Modifiers,
    mode: ModeFlags,
) -> Option<Vec<u8>> {
    if !mode.mouse_active()
        || (action == MouseAction::Drag && !mode.mouse_drag && !mode.mouse_motion)
    {
        return None;
    }
    let base = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };
    let modifier = 4 * u8::from(mods.shift) + 8 * u8::from(mods.alt) + 16 * u8::from(mods.ctrl);
    let motion = if action == MouseAction::Drag { 32 } else { 0 };
    let code = base + modifier + motion;
    if mode.sgr_mouse {
        let final_byte = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{code};{};{}{final_byte}", col + 1, row + 1).into_bytes());
    }
    // Legacy encoding: release has no button identity and coordinates cap at 223.
    let code = if action == MouseAction::Release {
        3 + modifier
    } else {
        code
    };
    let encode = |v: u16| (v.min(222) as u8).saturating_add(33);
    Some(vec![0x1b, b'[', b'M', 32 + code, encode(col), encode(row)])
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
