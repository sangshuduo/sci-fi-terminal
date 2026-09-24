//! Renderer-facing immutable viewport snapshots.
//!
//! Snapshots never expose engine types. Colours stay symbolic (default,
//! indexed, or true colour) so the renderer can resolve them against the
//! active theme without asking the model again.

use std::sync::Arc;

use crate::ids::{ResizeEpoch, SessionId, SnapshotVersion};

/// Symbolic cell colour; resolved against the theme at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellColor {
    DefaultForeground,
    DefaultBackground,
    Cursor,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attributes of one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellFlags(u16);

impl CellFlags {
    pub const BOLD: Self = Self(1);
    pub const ITALIC: Self = Self(1 << 1);
    pub const UNDERLINE: Self = Self(1 << 2);
    pub const STRIKEOUT: Self = Self(1 << 3);
    pub const INVERSE: Self = Self(1 << 4);
    pub const DIM: Self = Self(1 << 5);
    pub const HIDDEN: Self = Self(1 << 6);
    /// First half of a double-width character.
    pub const WIDE: Self = Self(1 << 7);
    /// Continuation cell of a double-width character; draw nothing.
    pub const WIDE_SPACER: Self = Self(1 << 8);
    /// Row soft-wraps into the next row (set on the last cell).
    pub const WRAPLINE: Self = Self(1 << 9);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn with_if(self, other: Self, condition: bool) -> Self {
        if condition { self.with(other) } else { self }
    }
}

/// One visible cell. `text` holds the base character plus any combining marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellSnapshot {
    pub text: CellText,
    pub fg: CellColor,
    pub bg: CellColor,
    pub flags: CellFlags,
}

/// Cell text without a heap allocation for the common single-char case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellText {
    Char(char),
    Cluster(Arc<str>),
}

impl CellText {
    pub fn is_blank(&self) -> bool {
        matches!(self, Self::Char(' ') | Self::Char('\0'))
    }

    pub fn push_to(&self, out: &mut String) {
        match self {
            Self::Char(c) => out.push(*c),
            Self::Cluster(s) => out.push_str(s),
        }
    }
}

/// Cursor geometry as the terminal model reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSnapshot {
    pub row: u16,
    pub col: u16,
    pub shape: CursorShape,
}

/// Terminal modes the UI needs for input encoding and mouse routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModeFlags {
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    pub alt_screen: bool,
    pub mouse_report: bool,
    pub mouse_motion: bool,
    pub mouse_drag: bool,
    pub sgr_mouse: bool,
    pub focus_events: bool,
    pub line_feed_new_line: bool,
    pub alternate_scroll: bool,
}

impl ModeFlags {
    pub fn mouse_active(&self) -> bool {
        self.mouse_report || self.mouse_motion || self.mouse_drag
    }
}

/// Selection in viewport coordinates, inclusive on both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionSpan {
    pub start: (u16, u16),
    pub end: (u16, u16),
    pub block: bool,
}

impl SelectionSpan {
    /// Whether the viewport cell `(row, col)` is selected.
    pub fn contains(&self, row: u16, col: u16) -> bool {
        if row < self.start.0 || row > self.end.0 {
            return false;
        }
        if self.block {
            let (lo, hi) = order(self.start.1, self.end.1);
            return col >= lo && col <= hi;
        }
        let after_start = row > self.start.0 || col >= self.start.1;
        let before_end = row < self.end.0 || col <= self.end.1;
        after_start && before_end
    }
}

fn order(a: u16, b: u16) -> (u16, u16) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Rows changed since the previous snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    Full,
    Rows(Vec<u16>),
}

/// Latest immutable view of a session, published by the session owner.
#[derive(Debug, Clone)]
pub struct ViewportSnapshot {
    pub session: SessionId,
    pub version: SnapshotVersion,
    pub epoch: ResizeEpoch,
    pub columns: u16,
    pub rows: u16,
    /// Row-major, `rows * columns` cells.
    pub cells: Arc<[CellSnapshot]>,
    pub cursor: Option<CursorSnapshot>,
    pub selection: Option<SelectionSpan>,
    pub mode: ModeFlags,
    /// Lines scrolled back from the live bottom.
    pub display_offset: usize,
    /// Lines currently retained in history.
    pub history_lines: usize,
    pub damage: Damage,
}

impl ViewportSnapshot {
    pub fn cell(&self, row: u16, col: u16) -> Option<&CellSnapshot> {
        if row >= self.rows || col >= self.columns {
            return None;
        }
        self.cells
            .get(usize::from(row) * usize::from(self.columns) + usize::from(col))
    }

    pub fn row(&self, row: u16) -> &[CellSnapshot] {
        let width = usize::from(self.columns);
        let start = usize::from(row) * width;
        self.cells.get(start..start + width).unwrap_or(&[])
    }

    /// Plain text of the visible viewport, one line per row, trailing blanks trimmed.
    /// Used for the bounded accessible text representation and tests.
    pub fn visible_text(&self) -> String {
        let mut out = String::new();
        for row in 0..self.rows {
            let mut line = String::new();
            for cell in self.row(row) {
                if !cell.flags.contains(CellFlags::WIDE_SPACER) {
                    cell.text.push_to(&mut line);
                }
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_selection_contains() {
        let span = SelectionSpan {
            start: (1, 5),
            end: (3, 2),
            block: false,
        };
        assert!(!span.contains(1, 4));
        assert!(span.contains(1, 5));
        assert!(span.contains(2, 0));
        assert!(span.contains(3, 2));
        assert!(!span.contains(3, 3));
        assert!(!span.contains(0, 9));
    }

    #[test]
    fn block_selection_contains() {
        let span = SelectionSpan {
            start: (0, 6),
            end: (2, 2),
            block: true,
        };
        assert!(span.contains(1, 4));
        assert!(!span.contains(1, 7));
    }

    #[test]
    fn flags_compose() {
        let flags = CellFlags::empty()
            .with(CellFlags::BOLD)
            .with_if(CellFlags::DIM, false);
        assert!(flags.contains(CellFlags::BOLD));
        assert!(!flags.contains(CellFlags::DIM));
    }
}
