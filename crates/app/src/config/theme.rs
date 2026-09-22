//! Theme types, built-in themes and user theme loading.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::load::{ConfigError, MAX_CONFIG_BYTES, parse_error, read_limited};
use super::validate::{Diagnostic, is_safe_id, schema_version_diagnostic};

/// Minimum contrast ratio for readable text (WCAG AA).
pub const MIN_CONTRAST: f32 = 4.5;

/// An sRGB color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Creates a color from components.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses `#RRGGBB` (exactly 7 characters).
    pub fn parse_hex(s: &str) -> Option<Rgb> {
        let hex = s.strip_prefix('#')?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Rgb::new(channel(0)?, channel(2)?, channel(4)?))
    }

    /// WCAG relative luminance in `0.0..=1.0`.
    pub fn relative_luminance(self) -> f32 {
        fn linear(c: u8) -> f32 {
            let c = f32::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }
}

/// WCAG contrast ratio between two colors (1.0..=21.0).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (a.relative_luminance(), b.relative_luminance());
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Semantic UI color tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct UiColors {
    pub background: Rgb,
    pub surface: Rgb,
    pub foreground: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub error: Rgb,
    pub selection: Rgb,
    pub cursor: Rgb,
}

/// Terminal default colors and ANSI16 palette.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalPalette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub ansi: [Rgb; 16],
}

/// A complete theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub colors: UiColors,
    pub terminal: TerminalPalette,
    pub builtin: bool,
}

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

impl Theme {
    /// Resolves a 256-color index: ANSI16, xterm 6x6x6 cube, then grayscale ramp.
    pub fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.terminal.ansi[usize::from(index)],
            16..=231 => {
                let i = usize::from(index - 16);
                Rgb::new(
                    CUBE_LEVELS[i / 36],
                    CUBE_LEVELS[(i / 6) % 6],
                    CUBE_LEVELS[i % 6],
                )
            }
            232..=255 => {
                let level = 8 + 10 * (index - 232);
                Rgb::new(level, level, level)
            }
        }
    }

    /// Human-readable warnings for text/background pairs below [`MIN_CONTRAST`].
    pub fn contrast_warnings(&self) -> Vec<String> {
        let pairs = [
            (
                "colors.foreground/background",
                self.colors.foreground,
                self.colors.background,
            ),
            (
                "terminal.foreground/background",
                self.terminal.foreground,
                self.terminal.background,
            ),
        ];
        pairs
            .into_iter()
            .filter_map(|(label, fg, bg)| {
                let ratio = contrast_ratio(fg, bg);
                (ratio < MIN_CONTRAST)
                    .then(|| format!("{label} contrast {ratio:.2}:1 is below {MIN_CONTRAST}:1"))
            })
            .collect()
    }
}

/// Finds a theme by id.
pub fn find_theme<'a>(themes: &'a [Theme], id: &str) -> Option<&'a Theme> {
    themes.iter().find(|t| t.id == id)
}

// ---------------------------------------------------------------------------
// Theme file format
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    schema_version: u32,
    id: String,
    name: String,
    colors: ColorsFile,
    terminal: TerminalFile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorsFile {
    background: String,
    surface: String,
    foreground: String,
    muted: String,
    accent: String,
    error: String,
    selection: String,
    cursor: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminalFile {
    foreground: String,
    background: String,
    ansi: Vec<String>,
}

/// Collects color parse failures as diagnostics.
struct ColorReader(Vec<Diagnostic>);

impl ColorReader {
    fn read(&mut self, field: &str, value: &str) -> Rgb {
        Rgb::parse_hex(value).unwrap_or_else(|| {
            self.0.push(Diagnostic::new(
                field,
                format!("'{value}' is not a #RRGGBB color"),
            ));
            Rgb::new(0, 0, 0)
        })
    }
}

fn convert_colors(c: &ColorsFile, rd: &mut ColorReader) -> UiColors {
    UiColors {
        background: rd.read("colors.background", &c.background),
        surface: rd.read("colors.surface", &c.surface),
        foreground: rd.read("colors.foreground", &c.foreground),
        muted: rd.read("colors.muted", &c.muted),
        accent: rd.read("colors.accent", &c.accent),
        error: rd.read("colors.error", &c.error),
        selection: rd.read("colors.selection", &c.selection),
        cursor: rd.read("colors.cursor", &c.cursor),
    }
}

fn convert_terminal(t: &TerminalFile, rd: &mut ColorReader) -> TerminalPalette {
    if t.ansi.len() != 16 {
        rd.0.push(Diagnostic::new(
            "terminal.ansi",
            format!("expected exactly 16 colors, found {}", t.ansi.len()),
        ));
    }
    let mut ansi = [Rgb::new(0, 0, 0); 16];
    for (i, (slot, value)) in ansi.iter_mut().zip(&t.ansi).enumerate() {
        *slot = rd.read(&format!("terminal.ansi[{i}]"), value);
    }
    TerminalPalette {
        foreground: rd.read("terminal.foreground", &t.foreground),
        background: rd.read("terminal.background", &t.background),
        ansi,
    }
}

fn convert_theme(file: ThemeFile) -> Result<Theme, Vec<Diagnostic>> {
    let mut rd = ColorReader(Vec::new());
    rd.0.extend(schema_version_diagnostic(file.schema_version));
    if !is_safe_id(&file.id) {
        rd.0.push(Diagnostic::new(
            "id",
            "theme id must be 1-64 characters of [a-z0-9_-]",
        ));
    }
    if file.name.trim().is_empty() {
        rd.0.push(Diagnostic::new("name", "must not be empty"));
    }
    let colors = convert_colors(&file.colors, &mut rd);
    let terminal = convert_terminal(&file.terminal, &mut rd);
    if !rd.0.is_empty() {
        return Err(rd.0);
    }
    Ok(Theme {
        id: file.id,
        name: file.name,
        colors,
        terminal,
        builtin: false,
    })
}

/// Parses a user theme file (see "Theme contract" in `docs/CONFIGURATION.md`).
pub fn parse_theme(text: &str, path: &Path) -> Result<Theme, ConfigError> {
    let file: ThemeFile = toml::from_str(text).map_err(|e| parse_error(text, path, &e))?;
    convert_theme(file).map_err(|diags| {
        ConfigError::Invalid(
            diags
                .into_iter()
                .map(|d| d.with_file(path.to_path_buf()))
                .collect(),
        )
    })
}

fn is_theme_candidate(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
}

/// Loads `*.toml` regular files directly inside `themes_dir`, skipping symlinks
/// and subdirectories. A missing directory yields no themes and no errors.
pub fn load_user_themes(themes_dir: &Path) -> (Vec<Theme>, Vec<ConfigError>) {
    let entries = match fs::read_dir(themes_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), Vec::new()),
        Err(source) => {
            let err = ConfigError::Io {
                path: themes_dir.to_path_buf(),
                source,
            };
            return (Vec::new(), vec![err]);
        }
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_theme_candidate(path))
        .collect();
    paths.sort();
    let mut themes = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match load_theme_file(&path) {
            Ok(Some(theme)) => themes.push(theme),
            Ok(None) => {}
            Err(err) => errors.push(err),
        }
    }
    (themes, errors)
}

/// Loads one theme; `Ok(None)` for symlinks and non-regular files.
fn load_theme_file(path: &Path) -> Result<Option<Theme>, ConfigError> {
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
        Some(text) => parse_theme(&text, path).map(Some),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Built-in themes (original palettes designed for this project)
// ---------------------------------------------------------------------------

const fn hex(v: u32) -> Rgb {
    Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

const fn ansi(v: [u32; 16]) -> [Rgb; 16] {
    let mut out = [Rgb::new(0, 0, 0); 16];
    let mut i = 0;
    while i < 16 {
        out[i] = hex(v[i]);
        i += 1;
    }
    out
}

/// `[background, surface, foreground, muted, accent, error, selection, cursor]`.
fn ui(c: [u32; 8]) -> UiColors {
    UiColors {
        background: hex(c[0]),
        surface: hex(c[1]),
        foreground: hex(c[2]),
        muted: hex(c[3]),
        accent: hex(c[4]),
        error: hex(c[5]),
        selection: hex(c[6]),
        cursor: hex(c[7]),
    }
}

fn builtin(id: &str, name: &str, colors: [u32; 8], term: (u32, u32, [u32; 16])) -> Theme {
    Theme {
        id: id.to_owned(),
        name: name.to_owned(),
        colors: ui(colors),
        terminal: TerminalPalette {
            foreground: hex(term.0),
            background: hex(term.1),
            ansi: ansi(term.2),
        },
        builtin: true,
    }
}

/// The four built-in themes: graphite, daylight, signal, high-contrast.
pub fn builtin_themes() -> Vec<Theme> {
    vec![
        builtin(
            "graphite",
            "Graphite",
            [
                0x101820, 0x1B2630, 0xE5EDF3, 0xA6B4BF, 0x77C7E8, 0xFF8C8C, 0x345568, 0xE5EDF3,
            ],
            (
                0xE5EDF3,
                0x101820,
                [
                    0x14212B, 0xD96F78, 0x83B98A, 0xD9BC76, 0x7DA5D8, 0xBB91CE, 0x76BFC5, 0xC7D0D8,
                    0x697B89, 0xFF939C, 0xA2D7AA, 0xF5D998, 0xA1C4F0, 0xD8B0E8, 0x9DDFE4, 0xF3F7FA,
                ],
            ),
        ),
        builtin(
            "daylight",
            "Daylight",
            [
                0xF7F5F0, 0xECE8DF, 0x1E2328, 0x4F5962, 0x1F5F8B, 0xA8262E, 0xC9DCEB, 0x1E2328,
            ],
            (
                0x1E2328,
                0xF7F5F0,
                [
                    0x1E2328, 0xA8262E, 0x2E6B34, 0x7A5A00, 0x1F5F8B, 0x7B3F8C, 0x1C6C73, 0xD5D0C4,
                    0x5E666E, 0xC4383F, 0x3C8443, 0x946F0A, 0x2F76A8, 0x9651A8, 0x278590, 0xFFFFFF,
                ],
            ),
        ),
        builtin(
            "signal",
            "Signal",
            [
                0x14110D, 0x211C16, 0xEDE3D2, 0xB3A48C, 0xF2A93B, 0xFF7B6B, 0x4A3A22, 0xF2A93B,
            ],
            (
                0xEDE3D2,
                0x14110D,
                [
                    0x1C1812, 0xE0705E, 0x9DBA6A, 0xF2A93B, 0x7FA3C4, 0xC58FB4, 0x7DBFAF, 0xD6CCBB,
                    0x6F6556, 0xFF8E7A, 0xB8D486, 0xFFC766, 0x9ABFE0, 0xDDA8CC, 0x98D8C8, 0xFAF4EA,
                ],
            ),
        ),
        builtin(
            "high-contrast",
            "High Contrast",
            [
                0x000000, 0x0F0F0F, 0xFFFFFF, 0xD0D0D0, 0xFFD500, 0xFF9A9A, 0x1F3F7F, 0xFFFFFF,
            ],
            (
                0xFFFFFF,
                0x000000,
                [
                    0x000000, 0xFF7A7A, 0x6BFF8A, 0xFFE14D, 0x8AB4FF, 0xFF8AF0, 0x5CF0FF, 0xE6E6E6,
                    0xA0A0A0, 0xFFA3A3, 0xA3FFB8, 0xFFF08A, 0xB8D0FF, 0xFFB8F5, 0xA3F7FF, 0xFFFFFF,
                ],
            ),
        ),
    ]
}
