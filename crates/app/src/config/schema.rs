//! Typed configuration schema (see `docs/CONFIGURATION.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Current configuration schema version understood by this build.
pub const SCHEMA_VERSION: u32 = 1;

/// Name of the profile created by default.
pub const DEFAULT_PROFILE_ID: &str = "default";

/// Complete application configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Required in user-written files; no serde default.
    pub schema_version: u32,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub terminal: TerminalSettings,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    pub layout: LayoutSettings,
    #[serde(default)]
    pub panels: Panels,
    #[serde(default)]
    pub effects: Effects,
    #[serde(default)]
    pub keybindings: Vec<KeyBinding>,
}

impl Default for Config {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert(DEFAULT_PROFILE_ID.to_owned(), Profile::default());
        Self {
            schema_version: SCHEMA_VERSION,
            appearance: Appearance::default(),
            terminal: TerminalSettings::default(),
            profiles,
            layout: LayoutSettings::default(),
            panels: Panels::default(),
            effects: Effects::default(),
            keybindings: Vec::new(),
        }
    }
}

/// Visual appearance settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Appearance {
    pub theme: String,
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    pub ligatures: bool,
    pub ui_scale: f32,
    pub reduced_motion: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "graphite".to_owned(),
            font_family: "monospace".to_owned(),
            font_size: 14.0,
            line_height: 1.15,
            ligatures: false,
            ui_scale: 1.0,
            reduced_motion: true,
        }
    }
}

/// Terminal emulation behavior settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalSettings {
    pub scrollback_lines: u32,
    pub scrollback_max_mib: u32,
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
    pub confirm_multiline_paste: bool,
    pub copy_on_select: bool,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            scrollback_lines: 10_000,
            scrollback_max_mib: 32,
            cursor_shape: CursorShape::Block,
            cursor_blink: false,
            confirm_multiline_paste: true,
            copy_on_select: false,
        }
    }
}

/// Terminal cursor shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorShape {
    #[default]
    Block,
    Beam,
    Underline,
}

/// Shell profile. Values are used verbatim; environment variables are never expanded.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    /// Empty means discover the OS default shell.
    pub executable: String,
    pub args: Vec<String>,
    pub login_shell: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Session layout settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutSettings {
    pub preset: LayoutPreset,
    pub restore: bool,
    pub max_sessions: u8,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            preset: LayoutPreset::Focus,
            restore: true,
            max_sessions: 8,
        }
    }
}

/// Layout preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutPreset {
    #[default]
    Focus,
    Split,
}

/// Auxiliary panel settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Panels {
    pub metrics: MetricsPanelSettings,
}

/// System metrics panel settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetricsPanelSettings {
    pub enabled: bool,
    pub interval_ms: u64,
}

impl Default for MetricsPanelSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_ms: 1000,
        }
    }
}

/// Visual effects settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Effects {
    pub preset: EffectsPreset,
    pub intensity: f32,
    pub max_fps: u32,
}

impl Default for Effects {
    fn default() -> Self {
        Self {
            preset: EffectsPreset::Off,
            intensity: 0.15,
            max_fps: 30,
        }
    }
}

/// Supported effects presets; anything else is rejected at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectsPreset {
    #[default]
    Off,
    Subtle,
}

/// Explicit keybinding override, keyed by `(action, context)`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeyBinding {
    pub action: String,
    pub context: String,
    pub keys: String,
}
