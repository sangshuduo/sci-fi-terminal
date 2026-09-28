//! Optional `[style]` section of a theme: data-only visual tokens.
//!
//! This replaces stylesheet injection. Themes can shape surfaces (corner
//! radius, border width), request edge glow and name a UI font — but never
//! supply code, shaders, URLs or file imports.

use serde::{Deserialize, Serialize};

use super::validate::Diagnostic;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeStyle {
    /// Corner radius of panels, dialogs and tabs in logical pixels (0–16).
    pub corner_radius: f32,
    /// Border width of panels and dialogs in logical pixels (0–4).
    pub border_width: f32,
    /// Strength of the static edge glow around the focused pane (0–1).
    /// Only drawn when effects are enabled.
    pub glow: f32,
    /// Font family for UI chrome; empty uses the system UI font.
    pub ui_font: String,
}

impl Default for ThemeStyle {
    fn default() -> Self {
        Self {
            corner_radius: 4.0,
            border_width: 1.0,
            glow: 0.0,
            ui_font: String::new(),
        }
    }
}

impl ThemeStyle {
    /// Styling chosen for each built-in theme.
    pub fn for_builtin(id: &str) -> Self {
        match id {
            "signal" => Self {
                corner_radius: 2.0,
                glow: 0.35,
                ..Self::default()
            },
            "graphite" => Self {
                glow: 0.2,
                ..Self::default()
            },
            "high-contrast" => Self {
                corner_radius: 0.0,
                border_width: 2.0,
                ..Self::default()
            },
            _ => Self::default(),
        }
    }

    /// Range and content checks; field names are prefixed with `style.`.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let ranges = [
            ("style.corner_radius", self.corner_radius, 0.0, 16.0),
            ("style.border_width", self.border_width, 0.0, 4.0),
            ("style.glow", self.glow, 0.0, 1.0),
        ];
        for (field, value, min, max) in ranges {
            if !value.is_finite() || value < min || value > max {
                out.push(Diagnostic::new(
                    field,
                    format!("{value} is out of range {min}..={max}"),
                ));
            }
        }
        let font_ok = self.ui_font.len() <= 128
            && !self
                .ui_font
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\'));
        if !font_ok {
            out.push(Diagnostic::new(
                "style.ui_font",
                "must be a font family name (at most 128 characters, no paths)",
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_builtins_are_valid() {
        for id in ["graphite", "daylight", "signal", "high-contrast"] {
            assert!(ThemeStyle::for_builtin(id).diagnostics().is_empty(), "{id}");
        }
    }

    #[test]
    fn rejects_out_of_range_and_paths() {
        let style = ThemeStyle {
            corner_radius: 99.0,
            glow: f32::NAN,
            ui_font: "../fonts/evil.ttf".into(),
            ..ThemeStyle::default()
        };
        let fields: Vec<String> = style.diagnostics().into_iter().map(|d| d.field).collect();
        assert_eq!(
            fields,
            vec!["style.corner_radius", "style.glow", "style.ui_font"]
        );
    }

    #[test]
    fn unknown_style_fields_are_rejected() {
        assert!(toml::from_str::<ThemeStyle>("css = \"body { }\"").is_err());
        let parsed: ThemeStyle = toml::from_str("glow = 0.5").expect("parse");
        assert_eq!(parsed.glow, 0.5);
        assert_eq!(parsed.corner_radius, 4.0);
    }
}
