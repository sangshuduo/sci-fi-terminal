//! Project-owned adapter around the `alacritty_terminal` engine.
//!
//! Only the session owner holds a [`TerminalModel`]. Engine events are
//! mediated here: protocol replies are returned to the caller for bounded
//! queueing, titles are sanitised, and clipboard/host effects are denied.

use std::sync::{Arc, Mutex, PoisonError};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, Osc52, Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{self, Color, NamedColor, Processor, StdSyncHandler};

use crate::ids::{ResizeEpoch, SessionId, SnapshotVersion};
use crate::policy::{Limits, sanitize_title};
use crate::snapshot::{
    CellColor, CellFlags, CellSnapshot, CellText, CursorShape, CursorSnapshot, Damage, ModeFlags,
    SelectionSpan, ViewportSnapshot,
};

/// Terminal size in cells plus the pixel size of one cell (for size queries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub columns: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl TermSize {
    /// Construct a size, clamping to at least 2 columns and 1 row as the engine requires.
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            columns: columns.max(2),
            rows: rows.max(1),
            cell_width: 8,
            cell_height: 16,
        }
    }

    #[must_use]
    pub fn with_cell_pixels(self, width: u16, height: u16) -> Self {
        Self {
            cell_width: width.max(1),
            cell_height: height.max(1),
            ..self
        }
    }
}

struct EngineSize {
    columns: usize,
    lines: usize,
}

impl Dimensions for EngineSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Mediated result of feeding output to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelEvent {
    /// Sanitised window/tab title; `None` resets to the default.
    Title(Option<String>),
    /// Visual bell; the UI rate-limits it.
    Bell,
    /// A host effect the policy refused (for diagnostics counters, no payload).
    Denied(&'static str),
}

/// Viewport coordinate used for selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPoint {
    pub row: u16,
    pub col: u16,
    /// Whether the pointer was on the right half of the cell.
    pub right_half: bool,
}

/// Selection gesture kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    Simple,
    Block,
    Word,
    Line,
}

/// Raw engine events collected during `advance`; drained immediately after.
#[derive(Clone, Default)]
pub(crate) struct Listener {
    queue: Arc<Mutex<Vec<Event>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if matches!(
            event,
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange
        ) {
            return;
        }
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        // Bounded: the owner drains after every batch, so this only guards pathological input.
        if queue.len() < 4096 {
            queue.push(event);
        }
    }
}

/// The terminal model owned by a single session owner thread.
pub struct TerminalModel {
    session: SessionId,
    term: Term<Listener>,
    parser: Processor<StdSyncHandler>,
    listener: Listener,
    size: TermSize,
    limits: Limits,
    version: SnapshotVersion,
    epoch: ResizeEpoch,
    full_damage_pending: bool,
    palette: Option<Arc<[(u8, u8, u8); 18]>>,
}

impl TerminalModel {
    pub fn new(session: SessionId, size: TermSize, limits: Limits) -> Self {
        let config = engine_config(&limits, size.columns);
        let listener = Listener::default();
        let dims = EngineSize {
            columns: size.columns.into(),
            lines: size.rows.into(),
        };
        let term = Term::new(config, &dims, listener.clone());
        Self {
            session,
            term,
            parser: Processor::new(),
            listener,
            size,
            limits,
            version: SnapshotVersion::default(),
            epoch: ResizeEpoch::default(),
            full_damage_pending: true,
            palette: None,
        }
    }

    pub fn session(&self) -> SessionId {
        self.session
    }

    pub fn size(&self) -> TermSize {
        self.size
    }

    pub fn epoch(&self) -> ResizeEpoch {
        self.epoch
    }

    pub fn mode(&self) -> ModeFlags {
        mode_flags(*self.term.mode())
    }

    /// Colours used to answer OSC colour queries: 16 ANSI, then fg, bg.
    pub fn set_query_palette(&mut self, palette: [(u8, u8, u8); 18]) {
        self.palette = Some(Arc::new(palette));
    }

    /// Parse a chunk of PTY output. Returns mediated events and the protocol
    /// replies that must be written back to the child, in order.
    pub fn advance(&mut self, bytes: &[u8]) -> (Vec<ModelEvent>, Vec<Vec<u8>>) {
        self.parser.advance(&mut self.term, bytes);
        self.drain_events()
    }

    fn drain_events(&mut self) -> (Vec<ModelEvent>, Vec<Vec<u8>>) {
        let raw = {
            let mut queue = self
                .listener
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut *queue)
        };
        let mut events = Vec::new();
        let mut replies = Vec::new();
        for event in raw {
            self.mediate(event, &mut events, &mut replies);
        }
        (events, replies)
    }

    fn mediate(&self, event: Event, events: &mut Vec<ModelEvent>, replies: &mut Vec<Vec<u8>>) {
        match event {
            Event::Title(title) => {
                events.push(ModelEvent::Title(Some(sanitize_title(
                    &title,
                    self.limits.max_title_bytes,
                ))));
            }
            Event::ResetTitle => events.push(ModelEvent::Title(None)),
            Event::Bell => events.push(ModelEvent::Bell),
            Event::PtyWrite(text) => replies.push(text.into_bytes()),
            Event::TextAreaSizeRequest(format) => {
                replies.push(format(self.window_size()).into_bytes());
            }
            Event::ColorRequest(index, format) => match self.query_color(index) {
                Some(rgb) => replies.push(format(rgb).into_bytes()),
                None => events.push(ModelEvent::Denied("color-query")),
            },
            Event::ClipboardStore(..) => events.push(ModelEvent::Denied("clipboard-store")),
            Event::ClipboardLoad(..) => events.push(ModelEvent::Denied("clipboard-load")),
            _ => {}
        }
    }

    fn window_size(&self) -> WindowSize {
        WindowSize {
            num_lines: self.size.rows,
            num_cols: self.size.columns,
            cell_width: self.size.cell_width,
            cell_height: self.size.cell_height,
        }
    }

    fn query_color(&self, index: usize) -> Option<ansi::Rgb> {
        if let Some(rgb) = self.term.colors()[index] {
            return Some(rgb);
        }
        let palette = self.palette.as_ref()?;
        let slot = match index {
            0..=15 => index,
            i if i == NamedColor::Foreground as usize => 16,
            i if i == NamedColor::Background as usize => 17,
            _ => return None,
        };
        let (r, g, b) = palette[slot];
        Some(ansi::Rgb { r, g, b })
    }

    /// Resize the model and start a new epoch. Callers resize the PTY first.
    pub fn resize(&mut self, size: TermSize) -> ResizeEpoch {
        let dims = EngineSize {
            columns: size.columns.into(),
            lines: size.rows.into(),
        };
        self.term.resize(dims);
        let history = self.limits.effective_scrollback(size.columns.into());
        let mut config = engine_config(&self.limits, size.columns);
        config.scrolling_history = history;
        self.term.set_options(config);
        self.size = size;
        self.epoch = self.epoch.next();
        self.full_damage_pending = true;
        self.epoch
    }

    /// Scroll the viewport through history; positive values scroll up.
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
        self.full_damage_pending = true;
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.term.grid().display_offset() != 0 {
            self.term.scroll_display(Scroll::Bottom);
            self.full_damage_pending = true;
        }
    }

    pub fn start_selection(&mut self, at: GridPoint, kind: SelectionKind) {
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Block => SelectionType::Block,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
        };
        let (point, side) = self.to_engine_point(at);
        self.term.selection = Some(Selection::new(ty, point, side));
        self.full_damage_pending = true;
    }

    pub fn update_selection(&mut self, at: GridPoint) {
        let (point, side) = self.to_engine_point(at);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, side);
            self.full_damage_pending = true;
        }
    }

    pub fn clear_selection(&mut self) {
        if self.term.selection.take().is_some() {
            self.full_damage_pending = true;
        }
    }

    /// Selected text, skipping wide-cell spacers and honouring soft wraps.
    pub fn selection_text(&self) -> Option<String> {
        self.term
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    fn to_engine_point(&self, at: GridPoint) -> (Point, Side) {
        let offset = self.term.grid().display_offset() as i32;
        let row = at.row.min(self.size.rows.saturating_sub(1));
        let col = at.col.min(self.size.columns.saturating_sub(1));
        let point = Point::new(Line(i32::from(row) - offset), Column(col.into()));
        let side = if at.right_half {
            Side::Right
        } else {
            Side::Left
        };
        (point, side)
    }

    pub(crate) fn term(&self) -> &Term<Listener> {
        &self.term
    }

    pub(crate) fn limits_max_results(&self) -> usize {
        self.limits.max_search_results
    }

    pub(crate) fn scroll_to_line(&mut self, line: i32) {
        self.term.scroll_to_point(Point::new(Line(line), Column(0)));
        self.full_damage_pending = true;
    }

    /// Build the next immutable snapshot and clear accumulated damage.
    pub fn snapshot(&mut self) -> ViewportSnapshot {
        let damage = self.take_damage();
        self.version = self.version.next();
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        let columns = self.size.columns;
        let rows = self.size.rows;
        let mut cells = Vec::with_capacity(usize::from(columns) * usize::from(rows));
        for indexed in content.display_iter {
            cells.push(convert_cell(indexed.cell));
        }
        cells.resize(usize::from(columns) * usize::from(rows), blank_cell());
        let cursor = convert_cursor(content.cursor.shape, content.cursor.point, offset, rows);
        let selection = content.selection.and_then(|range| {
            clip_selection(range.start, range.end, range.is_block, offset, self.size)
        });
        ViewportSnapshot {
            session: self.session,
            version: self.version,
            epoch: self.epoch,
            columns,
            rows,
            cells: cells.into(),
            cursor,
            selection,
            mode: mode_flags(content.mode),
            display_offset: content.display_offset,
            history_lines: self.term.grid().history_size(),
            damage,
        }
    }

    fn take_damage(&mut self) -> Damage {
        let damage = if std::mem::take(&mut self.full_damage_pending) {
            Damage::Full
        } else {
            match self.term.damage() {
                TermDamage::Full => Damage::Full,
                TermDamage::Partial(lines) => {
                    Damage::Rows(lines.map(|bounds| bounds.line as u16).collect())
                }
            }
        };
        self.term.reset_damage();
        damage
    }
}

fn engine_config(limits: &Limits, columns: u16) -> Config {
    Config {
        scrolling_history: limits.effective_scrollback(columns.into()),
        kitty_keyboard: false,
        osc52: Osc52::Disabled,
        ..Config::default()
    }
}

/// Clip an engine selection range to the visible viewport.
fn clip_selection(
    start: Point,
    end: Point,
    block: bool,
    offset: i32,
    size: TermSize,
) -> Option<SelectionSpan> {
    let rows = i32::from(size.rows);
    let last_col = size.columns.saturating_sub(1);
    let start_row = start.line.0 + offset;
    let end_row = end.line.0 + offset;
    if end_row < 0 || start_row >= rows {
        return None;
    }
    let start = if start_row < 0 {
        (0, if block { start.column.0 as u16 } else { 0 })
    } else {
        (start_row as u16, start.column.0 as u16)
    };
    let end = if end_row >= rows {
        (
            (rows - 1) as u16,
            if block { end.column.0 as u16 } else { last_col },
        )
    } else {
        (end_row as u16, end.column.0 as u16)
    };
    Some(SelectionSpan { start, end, block })
}

fn convert_cursor(
    shape: ansi::CursorShape,
    point: Point,
    offset: i32,
    rows: u16,
) -> Option<CursorSnapshot> {
    let shape = match shape {
        ansi::CursorShape::Hidden => return None,
        ansi::CursorShape::Block => CursorShape::Block,
        ansi::CursorShape::Underline => CursorShape::Underline,
        ansi::CursorShape::Beam => CursorShape::Beam,
        ansi::CursorShape::HollowBlock => CursorShape::HollowBlock,
    };
    let row = point.line.0 + offset;
    if row < 0 || row >= i32::from(rows) {
        return None;
    }
    Some(CursorSnapshot {
        row: row as u16,
        col: point.column.0 as u16,
        shape,
    })
}

fn blank_cell() -> CellSnapshot {
    CellSnapshot {
        text: CellText::Char(' '),
        fg: CellColor::DefaultForeground,
        bg: CellColor::DefaultBackground,
        flags: CellFlags::empty(),
    }
}

fn convert_cell(cell: &Cell) -> CellSnapshot {
    let text = match cell.zerowidth() {
        Some(marks) if !marks.is_empty() => {
            let mut cluster = String::with_capacity(4 + marks.len() * 3);
            cluster.push(cell.c);
            cluster.extend(marks.iter());
            CellText::Cluster(cluster.into())
        }
        _ => CellText::Char(if cell.c == '\0' { ' ' } else { cell.c }),
    };
    let (fg, fg_dim) = convert_color(cell.fg);
    let (bg, _) = convert_color(cell.bg);
    CellSnapshot {
        text,
        fg,
        bg,
        flags: convert_flags(cell.flags, fg_dim),
    }
}

fn convert_flags(flags: Flags, dim: bool) -> CellFlags {
    CellFlags::empty()
        .with_if(CellFlags::BOLD, flags.contains(Flags::BOLD))
        .with_if(CellFlags::ITALIC, flags.contains(Flags::ITALIC))
        .with_if(
            CellFlags::UNDERLINE,
            flags.intersects(Flags::ALL_UNDERLINES),
        )
        .with_if(CellFlags::STRIKEOUT, flags.contains(Flags::STRIKEOUT))
        .with_if(CellFlags::INVERSE, flags.contains(Flags::INVERSE))
        .with_if(CellFlags::DIM, dim || flags.contains(Flags::DIM))
        .with_if(CellFlags::HIDDEN, flags.contains(Flags::HIDDEN))
        .with_if(CellFlags::WIDE, flags.contains(Flags::WIDE_CHAR))
        .with_if(
            CellFlags::WIDE_SPACER,
            flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
        )
        .with_if(CellFlags::WRAPLINE, flags.contains(Flags::WRAPLINE))
}

/// Map an engine colour to a symbolic colour plus a "dim variant" hint.
fn convert_color(color: Color) -> (CellColor, bool) {
    match color {
        Color::Spec(rgb) => (CellColor::Rgb(rgb.r, rgb.g, rgb.b), false),
        Color::Indexed(index) => (CellColor::Indexed(index), false),
        Color::Named(named) => convert_named(named),
    }
}

fn convert_named(named: NamedColor) -> (CellColor, bool) {
    let index = named as usize;
    match named {
        NamedColor::Foreground | NamedColor::BrightForeground => {
            (CellColor::DefaultForeground, false)
        }
        NamedColor::DimForeground => (CellColor::DefaultForeground, true),
        NamedColor::Background => (CellColor::DefaultBackground, false),
        NamedColor::Cursor => (CellColor::Cursor, false),
        _ if index < 16 => (CellColor::Indexed(index as u8), false),
        // Dim variants follow the foreground/background/cursor entries and map to 0..8.
        _ => {
            let dim_base = NamedColor::DimBlack as usize;
            let normal = index.saturating_sub(dim_base).min(7);
            (CellColor::Indexed(normal as u8), true)
        }
    }
}

fn mode_flags(mode: TermMode) -> ModeFlags {
    ModeFlags {
        app_cursor: mode.contains(TermMode::APP_CURSOR),
        app_keypad: mode.contains(TermMode::APP_KEYPAD),
        bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        alt_screen: mode.contains(TermMode::ALT_SCREEN),
        mouse_report: mode.contains(TermMode::MOUSE_REPORT_CLICK),
        mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
        mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
        sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
        focus_events: mode.contains(TermMode::FOCUS_IN_OUT),
        line_feed_new_line: mode.contains(TermMode::LINE_FEED_NEW_LINE),
        alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
    }
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
