//! Data-only on-screen keyboard layouts: the built-in layout, the TOML format
//! for user layouts, and validation into typed [`Layout`] values.
//!
//! File format (original to this project):
//!
//! ```toml
//! schema_version = 1
//! id = "my-layout"
//! name = "My layout"
//!
//! [[rows]]
//! keys = [
//!   { text = "q", shifted = "Q" },
//!   { named = "backspace", label = "⌫", width = 1.5 },
//!   { modifier = "shift", width = 2.0 },
//!   { action = "caps" },
//! ]
//! ```
//!
//! Each key carries exactly one of `text`, `named`, `modifier` or `action`.
//! Keys only ever produce single key/text events; they cannot run commands.

use std::fs;
use std::path::Path;

use iced::keyboard::key::Named;
use serde::Deserialize;

use crate::config::ConfigError;
use crate::config::MAX_CONFIG_BYTES;
use crate::config::load::{parse_error, read_limited};
use crate::config::validate::is_safe_id;

/// Maximum number of keys in one layout.
pub const MAX_KEYS: usize = 120;
/// Maximum number of rows in one layout.
const MAX_ROWS: usize = 8;
/// Maximum characters in a text key's output.
const MAX_TEXT_CHARS: usize = 4;
/// Maximum characters in a key label or layout name.
const MAX_LABEL_CHARS: usize = 16;
const MAX_NAME_CHARS: usize = 64;
const MIN_WIDTH: f32 = 0.5;
const MAX_WIDTH: f32 = 8.0;
/// Layout file format version understood by this build.
const LAYOUT_SCHEMA_VERSION: u32 = 1;

/// A sticky modifier key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
}

/// What a key does when pressed.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    /// Types text; `shifted` is used while shift is active.
    Text {
        base: String,
        shifted: Option<String>,
    },
    /// A named (non-text) key such as Enter or an arrow.
    Named(Named),
    /// Toggles a one-shot sticky modifier.
    Modifier(Modifier),
    /// Toggles caps lock (letters only).
    ToggleCaps,
}

/// One key: display label, action and relative width.
#[derive(Debug, Clone, PartialEq)]
pub struct KeySpec {
    pub label: String,
    pub action: KeyAction,
    /// Relative width in key units (0.5..=8.0).
    pub width: f32,
}

/// A validated keyboard layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub id: String,
    pub name: String,
    pub rows: Vec<Vec<KeySpec>>,
}

// ---------------------------------------------------------------------------
// Intermediate file representation
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLayout {
    schema_version: u32,
    id: String,
    name: String,
    #[serde(default)]
    rows: Vec<RawRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRow {
    keys: Vec<RawKey>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKey {
    text: Option<String>,
    shifted: Option<String>,
    named: Option<String>,
    modifier: Option<Modifier>,
    action: Option<String>,
    label: Option<String>,
    width: Option<f32>,
}

// ---------------------------------------------------------------------------
// Named keys
// ---------------------------------------------------------------------------

/// Allowed named keys: (file name, key, default label).
const NAMED_KEYS: &[(&str, Named, &str)] = &[
    ("enter", Named::Enter, "Enter"),
    ("tab", Named::Tab, "Tab"),
    ("escape", Named::Escape, "Esc"),
    ("backspace", Named::Backspace, "⌫"),
    ("delete", Named::Delete, "Del"),
    ("space", Named::Space, "Space"),
    ("up", Named::ArrowUp, "↑"),
    ("down", Named::ArrowDown, "↓"),
    ("left", Named::ArrowLeft, "←"),
    ("right", Named::ArrowRight, "→"),
    ("home", Named::Home, "Home"),
    ("end", Named::End, "End"),
    ("pageup", Named::PageUp, "PgUp"),
    ("pagedown", Named::PageDown, "PgDn"),
    ("f1", Named::F1, "F1"),
    ("f2", Named::F2, "F2"),
    ("f3", Named::F3, "F3"),
    ("f4", Named::F4, "F4"),
    ("f5", Named::F5, "F5"),
    ("f6", Named::F6, "F6"),
    ("f7", Named::F7, "F7"),
    ("f8", Named::F8, "F8"),
    ("f9", Named::F9, "F9"),
    ("f10", Named::F10, "F10"),
    ("f11", Named::F11, "F11"),
    ("f12", Named::F12, "F12"),
];

fn named_from_str(name: &str) -> Option<Named> {
    NAMED_KEYS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, key, _)| *key)
}

fn named_label(named: Named) -> &'static str {
    NAMED_KEYS
        .iter()
        .find(|(_, key, _)| *key == named)
        .map_or("?", |(_, _, label)| label)
}

fn modifier_label(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::Shift => "Shift",
        Modifier::Ctrl => "Ctrl",
        Modifier::Alt => "Alt",
    }
}

/// Default label for an action when the file gives none.
fn default_label(action: &KeyAction) -> String {
    match action {
        KeyAction::Text { base, .. } => base.clone(),
        KeyAction::Named(named) => named_label(*named).to_owned(),
        KeyAction::Modifier(m) => modifier_label(*m).to_owned(),
        KeyAction::ToggleCaps => "Caps".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_text(value: &str, what: &str) -> Result<(), String> {
    let count = value.chars().count();
    if count == 0 || count > MAX_TEXT_CHARS {
        return Err(format!("{what} must be 1-{MAX_TEXT_CHARS} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{what} must not contain control characters"));
    }
    Ok(())
}

fn validate_label(value: &str) -> Result<(), String> {
    let count = value.chars().count();
    if count == 0 || count > MAX_LABEL_CHARS {
        return Err(format!("label must be 1-{MAX_LABEL_CHARS} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err("label must not contain control characters".to_owned());
    }
    Ok(())
}

fn validate_width(width: f32) -> Result<f32, String> {
    if width.is_finite() && (MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        Ok(width)
    } else {
        Err(format!("width must be between {MIN_WIDTH} and {MAX_WIDTH}"))
    }
}

/// Resolves the single action a raw key declares.
fn key_action(raw: &RawKey) -> Result<KeyAction, String> {
    let declared = [
        raw.text.is_some(),
        raw.named.is_some(),
        raw.modifier.is_some(),
        raw.action.is_some(),
    ];
    if declared.iter().filter(|d| **d).count() != 1 {
        return Err("exactly one of text, named, modifier or action is required".to_owned());
    }
    if raw.shifted.is_some() && raw.text.is_none() {
        return Err("shifted is only allowed on text keys".to_owned());
    }
    if let Some(base) = &raw.text {
        validate_text(base, "text")?;
        if let Some(shifted) = &raw.shifted {
            validate_text(shifted, "shifted")?;
        }
        return Ok(KeyAction::Text {
            base: base.clone(),
            shifted: raw.shifted.clone(),
        });
    }
    if let Some(name) = &raw.named {
        return named_from_str(name)
            .map(KeyAction::Named)
            .ok_or_else(|| format!("unknown named key {name:?}"));
    }
    if let Some(modifier) = raw.modifier {
        return Ok(KeyAction::Modifier(modifier));
    }
    match raw.action.as_deref() {
        Some("caps") => Ok(KeyAction::ToggleCaps),
        other => Err(format!("unknown action {:?}", other.unwrap_or_default())),
    }
}

fn key_spec(raw: &RawKey) -> Result<KeySpec, String> {
    let action = key_action(raw)?;
    let label = match &raw.label {
        Some(label) => {
            validate_label(label)?;
            label.clone()
        }
        None => default_label(&action),
    };
    let width = validate_width(raw.width.unwrap_or(1.0))?;
    Ok(KeySpec {
        label,
        action,
        width,
    })
}

fn validate_header(raw: &RawLayout) -> Result<(), String> {
    if raw.schema_version != LAYOUT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema_version {} (expected {LAYOUT_SCHEMA_VERSION})",
            raw.schema_version
        ));
    }
    if !is_safe_id(&raw.id) {
        return Err("id must be 1-64 characters of [a-z0-9_-]".to_owned());
    }
    let name_len = raw.name.chars().count();
    if raw.name.trim().is_empty() || name_len > MAX_NAME_CHARS {
        return Err(format!("name must be 1-{MAX_NAME_CHARS} characters"));
    }
    if raw.name.chars().any(char::is_control) {
        return Err("name must not contain control characters".to_owned());
    }
    Ok(())
}

fn validate_shape(raw: &RawLayout) -> Result<(), String> {
    if raw.rows.is_empty() || raw.rows.len() > MAX_ROWS {
        return Err(format!("layout must have 1-{MAX_ROWS} rows"));
    }
    if let Some(index) = raw.rows.iter().position(|row| row.keys.is_empty()) {
        return Err(format!("row {index}: must contain at least one key"));
    }
    let total: usize = raw.rows.iter().map(|row| row.keys.len()).sum();
    if total > MAX_KEYS {
        return Err(format!("layout has {total} keys (maximum {MAX_KEYS})"));
    }
    Ok(())
}

fn build_layout(raw: RawLayout) -> Result<Layout, String> {
    validate_header(&raw)?;
    validate_shape(&raw)?;
    let mut rows = Vec::with_capacity(raw.rows.len());
    for (r, row) in raw.rows.iter().enumerate() {
        let keys = row
            .keys
            .iter()
            .enumerate()
            .map(|(k, key)| key_spec(key).map_err(|e| format!("row {r}, key {k}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(keys);
    }
    Ok(Layout {
        id: raw.id,
        name: raw.name,
        rows,
    })
}

/// Parses and validates a layout file's text. Errors name the row/key index.
pub fn parse_layout(text: &str, path: &Path) -> Result<Layout, ConfigError> {
    let raw: RawLayout = toml::from_str(text).map_err(|e| parse_error(text, path, &e))?;
    build_layout(raw).map_err(|message| ConfigError::Parse {
        path: path.to_path_buf(),
        message,
    })
}

// ---------------------------------------------------------------------------
// User layouts on disk
// ---------------------------------------------------------------------------

/// Loads `*.toml` regular files directly inside `dir`, skipping symlinks and
/// subdirectories. A missing directory yields no layouts and no errors.
pub fn load_user_layouts(dir: &Path) -> (Vec<Layout>, Vec<ConfigError>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(source) => {
            let err = ConfigError::Io {
                path: dir.to_path_buf(),
                source,
            };
            return (Vec::new(), vec![err]);
        }
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut layouts = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match load_layout_file(&path) {
            Ok(Some(layout)) => layouts.push(layout),
            Ok(None) => {}
            Err(err) => errors.push(err),
        }
    }
    (layouts, errors)
}

/// Loads one layout; `Ok(None)` for symlinks and non-regular files.
fn load_layout_file(path: &Path) -> Result<Option<Layout>, ConfigError> {
    let meta = fs::symlink_metadata(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    match read_limited(path)? {
        Some(text) => parse_layout(&text, path).map(Some),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Built-in layout (standard QWERTY ordering, composed for this project)
// ---------------------------------------------------------------------------

fn text_key(base: &str, shifted: &str) -> KeySpec {
    let action = KeyAction::Text {
        base: base.to_owned(),
        shifted: Some(shifted.to_owned()),
    };
    KeySpec {
        label: base.to_owned(),
        action,
        width: 1.0,
    }
}

fn special(action: KeyAction, width: f32) -> KeySpec {
    KeySpec {
        label: default_label(&action),
        action,
        width,
    }
}

/// Text keys from pairs of `base`/`shifted` characters.
fn text_keys(bases: &str, shifted: &str) -> Vec<KeySpec> {
    bases
        .chars()
        .zip(shifted.chars())
        .map(|(b, s)| text_key(&b.to_string(), &s.to_string()))
        .collect()
}

fn with_edges(first: KeySpec, middle: Vec<KeySpec>, last: KeySpec) -> Vec<KeySpec> {
    let mut row = Vec::with_capacity(middle.len() + 2);
    row.push(first);
    row.extend(middle);
    row.push(last);
    row
}

fn bottom_row() -> Vec<KeySpec> {
    vec![
        special(KeyAction::Modifier(Modifier::Ctrl), 1.5),
        special(KeyAction::Modifier(Modifier::Alt), 1.5),
        special(KeyAction::Named(Named::Space), 6.0),
        special(KeyAction::Named(Named::ArrowLeft), 1.0),
        special(KeyAction::Named(Named::ArrowUp), 1.0),
        special(KeyAction::Named(Named::ArrowDown), 1.0),
        special(KeyAction::Named(Named::ArrowRight), 1.0),
    ]
}

/// The built-in US English layout (`id = "en-us"`), five rows.
pub fn builtin_layout() -> Layout {
    let rows = vec![
        with_edges(
            special(KeyAction::Named(Named::Escape), 1.0),
            text_keys("`1234567890-=", "~!@#$%^&*()_+"),
            special(KeyAction::Named(Named::Backspace), 1.5),
        ),
        with_edges(
            special(KeyAction::Named(Named::Tab), 1.5),
            text_keys("qwertyuiop[]\\", "QWERTYUIOP{}|"),
            special(KeyAction::Named(Named::Delete), 1.0),
        ),
        with_edges(
            special(KeyAction::ToggleCaps, 1.75),
            text_keys("asdfghjkl;'", "ASDFGHJKL:\""),
            special(KeyAction::Named(Named::Enter), 2.0),
        ),
        with_edges(
            special(KeyAction::Modifier(Modifier::Shift), 2.25),
            text_keys("zxcvbnm,./", "ZXCVBNM<>?"),
            special(KeyAction::Modifier(Modifier::Shift), 2.75),
        ),
        bottom_row(),
    ];
    Layout {
        id: "en-us".to_owned(),
        name: "English (US)".to_owned(),
        rows,
    }
}
