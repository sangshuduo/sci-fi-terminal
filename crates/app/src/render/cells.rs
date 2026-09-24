//! Cell metrics and colour resolution for the terminal surface.

use iced::Color;
use iced::Font;
use iced::advanced::graphics::text::{cosmic_text, font_system, to_attributes};
use terminal_core::{CellColor, CellFlags, CellSnapshot};

use crate::config::{Rgb, Theme};

/// Device-independent size of one terminal cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
}

impl CellMetrics {
    /// Measure the monospace advance with the renderer's own text system so
    /// grid positions match what is drawn. Falls back to a conventional ratio.
    pub fn measure(font: Font, font_size: f32, line_height: f32) -> Self {
        let height = (font_size * line_height).ceil();
        let width = measure_advance(font, font_size).unwrap_or(font_size * 0.6);
        Self {
            width: width.max(1.0),
            height: height.max(1.0),
            font_size,
        }
    }

    /// Columns and rows that fit into a pixel area (at least 2×1).
    pub fn grid_for(&self, width: f32, height: f32) -> (u16, u16) {
        let columns = (width / self.width).floor().clamp(2.0, f32::from(u16::MAX)) as u16;
        let rows = (height / self.height)
            .floor()
            .clamp(1.0, f32::from(u16::MAX)) as u16;
        (columns, rows)
    }
}

fn measure_advance(font: Font, font_size: f32) -> Option<f32> {
    const SAMPLE: &str = "MMMMMMMMMMMMMMMMMMMM";
    let mut system = font_system().write().ok()?;
    let raw = system.raw();
    let metrics = cosmic_text::Metrics::new(font_size, font_size * 1.2);
    let mut buffer = cosmic_text::Buffer::new(raw, metrics);
    buffer.set_size(raw, None, None);
    buffer.set_text(
        raw,
        SAMPLE,
        &to_attributes(font),
        cosmic_text::Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(raw, false);
    let width = buffer
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0_f32, f32::max);
    (width > 0.0).then(|| width / SAMPLE.len() as f32)
}

pub fn to_color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

/// Resolve a symbolic cell colour against the theme.
pub fn resolve(theme: &Theme, color: CellColor) -> Rgb {
    match color {
        CellColor::DefaultForeground => theme.terminal.foreground,
        CellColor::DefaultBackground => theme.terminal.background,
        CellColor::Cursor => theme.colors.cursor,
        CellColor::Indexed(index) => theme.indexed(index),
        CellColor::Rgb(r, g, b) => Rgb { r, g, b },
    }
}

/// Final foreground/background for a cell after inverse, bold-bright, dim and hidden.
pub fn cell_colors(theme: &Theme, cell: &CellSnapshot) -> (Color, Color) {
    let fg_symbol = match cell.fg {
        // Conventional bold-as-bright for the eight base colours.
        CellColor::Indexed(index) if index < 8 && cell.flags.contains(CellFlags::BOLD) => {
            CellColor::Indexed(index + 8)
        }
        other => other,
    };
    let mut fg = to_color(resolve(theme, fg_symbol));
    let mut bg = to_color(resolve(theme, cell.bg));
    if cell.flags.contains(CellFlags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.flags.contains(CellFlags::DIM) {
        fg = mix(fg, bg, 0.4);
    }
    if cell.flags.contains(CellFlags::HIDDEN) {
        fg = bg;
    }
    (fg, bg)
}

/// Linear blend from `a` towards `b` by `amount` (0..=1).
pub fn mix(a: Color, b: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::builtin_themes;
    use terminal_core::CellText;

    fn theme() -> Theme {
        builtin_themes().into_iter().next().expect("theme")
    }

    fn cell(fg: CellColor, bg: CellColor, flags: CellFlags) -> CellSnapshot {
        CellSnapshot {
            text: CellText::Char('x'),
            fg,
            bg,
            flags,
        }
    }

    #[test]
    fn grid_fits_area_with_minimums() {
        let metrics = CellMetrics {
            width: 8.0,
            height: 16.0,
            font_size: 13.0,
        };
        assert_eq!(metrics.grid_for(800.0, 480.0), (100, 30));
        assert_eq!(metrics.grid_for(0.0, 0.0), (2, 1));
    }

    #[test]
    fn measured_advance_is_positive() {
        let metrics = CellMetrics::measure(Font::MONOSPACE, 14.0, 1.2);
        assert!(metrics.width > 3.0 && metrics.width < 14.0, "{metrics:?}");
        assert_eq!(metrics.height, (14.0_f32 * 1.2).ceil());
    }

    #[test]
    fn inverse_swaps_and_hidden_matches_background() {
        let theme = theme();
        let plain = cell(
            CellColor::DefaultForeground,
            CellColor::DefaultBackground,
            CellFlags::empty(),
        );
        let (fg, bg) = cell_colors(&theme, &plain);
        let inverse = cell(
            CellColor::DefaultForeground,
            CellColor::DefaultBackground,
            CellFlags::INVERSE,
        );
        assert_eq!(cell_colors(&theme, &inverse), (bg, fg));
        let hidden = cell(
            CellColor::Indexed(1),
            CellColor::DefaultBackground,
            CellFlags::HIDDEN,
        );
        let (hfg, hbg) = cell_colors(&theme, &hidden);
        assert_eq!(hfg, hbg);
    }

    #[test]
    fn bold_brightens_base_colours_and_true_colour_passes_through() {
        let theme = theme();
        let bold = cell(
            CellColor::Indexed(1),
            CellColor::DefaultBackground,
            CellFlags::BOLD,
        );
        assert_eq!(cell_colors(&theme, &bold).0, to_color(theme.indexed(9)));
        let rgb = cell(
            CellColor::Rgb(1, 2, 3),
            CellColor::DefaultBackground,
            CellFlags::empty(),
        );
        assert_eq!(cell_colors(&theme, &rgb).0, Color::from_rgb8(1, 2, 3));
    }
}
