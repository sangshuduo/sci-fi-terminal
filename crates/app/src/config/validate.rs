//! Range validation and diagnostics for [`Config`].

use std::fmt;
use std::path::PathBuf;

use super::schema::{Config, KeyBinding, SCHEMA_VERSION};

/// A single validation problem tied to a dotted field path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: Option<PathBuf>,
    /// Dotted path, e.g. `appearance.font_size`.
    pub field: String,
    pub message: String,
}

impl Diagnostic {
    /// Creates a diagnostic without a file.
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            file: None,
            field: field.into(),
            message: message.into(),
        }
    }

    /// Returns a copy attributed to `file`.
    pub fn with_file(self, file: PathBuf) -> Self {
        Self {
            file: Some(file),
            ..self
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.file {
            Some(file) => write!(f, "{}: {}: {}", file.display(), self.field, self.message),
            None => write!(f, "{}: {}", self.field, self.message),
        }
    }
}

/// Maximum length of a safe identifier.
pub const MAX_ID_LEN: usize = 64;

/// Returns true if `id` matches `[a-z0-9_-]{1,64}`.
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Builds the diagnostic for a schema version this build cannot apply.
pub fn schema_version_diagnostic(version: u32) -> Option<Diagnostic> {
    use std::cmp::Ordering;
    match version.cmp(&SCHEMA_VERSION) {
        Ordering::Equal => None,
        Ordering::Greater => Some(Diagnostic::new(
            "schema_version",
            format!(
                "newer unsupported schema version {version} (this build supports {SCHEMA_VERSION}); \
                 file opened read-only and not applied"
            ),
        )),
        Ordering::Less => Some(Diagnostic::new(
            "schema_version",
            format!("unsupported schema version {version}; expected {SCHEMA_VERSION}"),
        )),
    }
}

/// Validates every range and identifier rule, returning all problems found.
pub fn validate(config: &Config) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    out.extend(schema_version_diagnostic(config.schema_version));
    validate_appearance(config, &mut out);
    validate_terminal(config, &mut out);
    validate_layout_panels_effects(config, &mut out);
    validate_profiles(config, &mut out);
    for (i, binding) in config.keybindings.iter().enumerate() {
        validate_keybinding(i, binding, &mut out);
    }
    out
}

fn check_f32(out: &mut Vec<Diagnostic>, field: &str, value: f32, min: f32, max: f32) {
    if !value.is_finite() {
        out.push(Diagnostic::new(field, "must be a finite number"));
    } else if value < min || value > max {
        out.push(Diagnostic::new(
            field,
            format!("{value} is out of range {min}..={max}"),
        ));
    }
}

fn check_int(out: &mut Vec<Diagnostic>, field: &str, value: u64, min: u64, max: u64) {
    if value < min || value > max {
        out.push(Diagnostic::new(
            field,
            format!("{value} is out of range {min}..={max}"),
        ));
    }
}

fn validate_appearance(config: &Config, out: &mut Vec<Diagnostic>) {
    let a = &config.appearance;
    if !is_safe_id(&a.theme) {
        out.push(Diagnostic::new(
            "appearance.theme",
            "theme id must be 1-64 characters of [a-z0-9_-]",
        ));
    }
    if a.font_family.trim().is_empty() || a.font_family.contains('\0') {
        out.push(Diagnostic::new(
            "appearance.font_family",
            "must be a non-empty name without NUL",
        ));
    }
    check_f32(out, "appearance.font_size", a.font_size, 8.0, 48.0);
    check_f32(out, "appearance.line_height", a.line_height, 1.0, 2.0);
    check_f32(out, "appearance.ui_scale", a.ui_scale, 0.75, 3.0);
}

fn validate_terminal(config: &Config, out: &mut Vec<Diagnostic>) {
    let t = &config.terminal;
    check_int(
        out,
        "terminal.scrollback_lines",
        u64::from(t.scrollback_lines),
        0,
        100_000,
    );
    check_int(
        out,
        "terminal.scrollback_max_mib",
        u64::from(t.scrollback_max_mib),
        1,
        256,
    );
}

fn validate_layout_panels_effects(config: &Config, out: &mut Vec<Diagnostic>) {
    check_int(
        out,
        "layout.max_sessions",
        u64::from(config.layout.max_sessions),
        1,
        8,
    );
    let interval = config.panels.metrics.interval_ms;
    if interval < 1000 {
        out.push(Diagnostic::new(
            "panels.metrics.interval_ms",
            format!("{interval} must be at least 1000"),
        ));
    }
    check_f32(out, "effects.intensity", config.effects.intensity, 0.0, 1.0);
    check_int(
        out,
        "effects.max_fps",
        u64::from(config.effects.max_fps),
        1,
        60,
    );
}

fn validate_profiles(config: &Config, out: &mut Vec<Diagnostic>) {
    for (id, profile) in &config.profiles {
        let base = format!("profiles.{id}");
        if !is_safe_id(id) {
            out.push(Diagnostic::new(
                base.clone(),
                "profile id must be 1-64 characters of [a-z0-9_-]",
            ));
        }
        if profile.executable.contains('\0') {
            out.push(Diagnostic::new(
                format!("{base}.executable"),
                "must not contain NUL",
            ));
        }
        if profile.args.iter().any(|a| a.contains('\0')) {
            out.push(Diagnostic::new(
                format!("{base}.args"),
                "must not contain NUL",
            ));
        }
        if profile.cwd.as_deref().is_some_and(|c| c.contains('\0')) {
            out.push(Diagnostic::new(
                format!("{base}.cwd"),
                "must not contain NUL",
            ));
        }
        let bad_env = profile
            .env
            .iter()
            .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'));
        if bad_env {
            // Never echo environment values: they may be secrets.
            out.push(Diagnostic::new(
                format!("{base}.env"),
                "names must be non-empty without '=' or NUL; values must not contain NUL",
            ));
        }
    }
}

/// Returns true if `keys` is `+`-separated, non-empty tokens (e.g. `Ctrl+Shift+T`).
pub fn keys_are_valid(keys: &str) -> bool {
    !keys.trim().is_empty() && keys.split('+').all(|token| !token.trim().is_empty())
}

fn validate_keybinding(index: usize, binding: &KeyBinding, out: &mut Vec<Diagnostic>) {
    let base = format!("keybindings[{index}]");
    if binding.action.trim().is_empty() {
        out.push(Diagnostic::new(
            format!("{base}.action"),
            "must not be empty",
        ));
    }
    if !keys_are_valid(&binding.keys) {
        out.push(Diagnostic::new(
            format!("{base}.keys"),
            format!(
                "cannot parse '{}': expected tokens separated by '+'",
                binding.keys
            ),
        ));
    }
}
