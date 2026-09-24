//! Terminal surface drawn on the shared wgpu device via an Iced canvas.
//!
//! Geometry comes from the snapshot grid (never proportional advances): each
//! glyph is placed at `column * cell_width`. The frame is cached and only
//! rebuilt when the owning pane clears the cache after a new snapshot, a
//! theme/font change or a resize, so idle terminals do not redraw.

use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::alignment;
use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Cache, Event, Frame, Geometry, Path, Stroke, Text};
use iced::widget::text::{LineHeight, Shaping};
use iced::{Color, Font, Point, Rectangle, Renderer, Size, keyboard, touch, window};
use terminal_core::{CellFlags, CellText, CursorShape, GridPoint, SearchResult, ViewportSnapshot};

use super::cells::{CellMetrics, cell_colors, mix, to_color};
use crate::config::{EffectsPreset, Theme};

/// Inner padding between the pane edge and the grid.
pub const PADDING: f32 = 4.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Pointer and geometry events produced by the terminal surface.
#[derive(Debug, Clone, PartialEq)]
pub enum TerminalEvent {
    /// The drawable area changed; the pane recomputes its grid size.
    Resized(Size),
    Pressed {
        at: GridPoint,
        button: mouse::Button,
        clicks: u8,
        shift: bool,
        alt: bool,
        ctrl: bool,
    },
    Dragged {
        at: GridPoint,
    },
    Released {
        at: GridPoint,
        button: mouse::Button,
    },
    Wheel {
        lines: f32,
        at: GridPoint,
    },
    /// A touch tap without movement: focus the pane and clear the selection.
    Tapped {
        at: GridPoint,
    },
}

/// Everything the surface needs to draw one pane.
pub struct TerminalView<'a, Message> {
    pub snapshot: Option<&'a Arc<ViewportSnapshot>>,
    pub theme: &'a Theme,
    pub metrics: CellMetrics,
    pub font: Font,
    pub focused: bool,
    pub cache: &'a Cache,
    pub search: Option<&'a SearchResult>,
    pub effects: EffectsPreset,
    pub overlay: Option<String>,
    pub on_event: Box<dyn Fn(TerminalEvent) -> Message + 'a>,
    /// One-finger drag scrolls; otherwise touches only focus.
    pub touch_scroll: bool,
    /// Edge glow strength from the theme style (0 disables).
    pub glow: f32,
}

#[derive(Default)]
pub struct ViewState {
    last_size: Option<Size>,
    dragging: bool,
    modifiers: keyboard::Modifiers,
    last_press: Option<(Instant, GridPoint, u8)>,
    touch: Option<TouchTrack>,
}

/// One tracked finger: drag scrolls, a tap without movement focuses.
struct TouchTrack {
    finger: touch::Finger,
    last: Point,
    moved: bool,
}

/// Movement (px) before a touch counts as a drag rather than a tap.
const TAP_SLOP: f32 = 8.0;

impl<Message> TerminalView<'_, Message> {
    fn cell_at(&self, bounds: Rectangle, position: Point) -> GridPoint {
        let x = (position.x - bounds.x - PADDING).max(0.0);
        let y = (position.y - bounds.y - PADDING).max(0.0);
        let col_f = x / self.metrics.width;
        let (cols, rows) = self
            .snapshot
            .map_or((u16::MAX, u16::MAX), |s| (s.columns, s.rows));
        GridPoint {
            row: ((y / self.metrics.height) as u16).min(rows.saturating_sub(1)),
            col: (col_f as u16).min(cols.saturating_sub(1)),
            right_half: col_f.fract() >= 0.5,
        }
    }

    fn press(
        &self,
        state: &mut ViewState,
        bounds: Rectangle,
        position: Point,
        button: mouse::Button,
    ) -> Message {
        let at = self.cell_at(bounds, position);
        let now = Instant::now();
        let clicks = match state.last_press {
            Some((when, last, count))
                if now - when < DOUBLE_CLICK && last.row == at.row && count < 3 =>
            {
                count + 1
            }
            _ => 1,
        };
        state.last_press = Some((now, at, clicks));
        state.dragging = button == mouse::Button::Left;
        let mods = state.modifiers;
        (self.on_event)(TerminalEvent::Pressed {
            at,
            button,
            clicks,
            shift: mods.shift(),
            alt: mods.alt(),
            ctrl: mods.control(),
        })
    }
}

impl<Message> canvas::Program<Message> for TerminalView<'_, Message> {
    type State = ViewState;

    fn update(
        &self,
        state: &mut ViewState,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                let size = bounds.size();
                if state.last_size != Some(size) {
                    state.last_size = Some(size);
                    return Some(canvas::Action::publish((self.on_event)(
                        TerminalEvent::Resized(size),
                    )));
                }
                None
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(mods)) => {
                state.modifiers = *mods;
                None
            }
            Event::Mouse(mouse_event) => self.mouse(state, mouse_event, bounds, cursor),
            Event::Touch(touch_event) => self.touch(state, touch_event, bounds),
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &ViewState,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let geometry = self
            .cache
            .draw(renderer, bounds.size(), |frame| self.paint(frame));
        vec![geometry]
    }

    fn mouse_interaction(
        &self,
        _state: &ViewState,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<Message> TerminalView<'_, Message> {
    fn touch(
        &self,
        state: &mut ViewState,
        event: &touch::Event,
        bounds: Rectangle,
    ) -> Option<canvas::Action<Message>> {
        match *event {
            touch::Event::FingerPressed { id, position }
                if bounds.contains(position) && state.touch.is_none() =>
            {
                state.touch = Some(TouchTrack {
                    finger: id,
                    last: position,
                    moved: false,
                });
                Some(canvas::Action::capture())
            }
            touch::Event::FingerMoved { id, position } => {
                let track = state.touch.as_mut().filter(|t| t.finger == id)?;
                let dy = position.y - track.last.y;
                if !track.moved && dy.abs() < TAP_SLOP {
                    return Some(canvas::Action::capture());
                }
                track.moved = true;
                track.last = position;
                if !self.touch_scroll {
                    return Some(canvas::Action::capture());
                }
                // Dragging down reveals earlier output, like scrolling a document.
                let lines = dy / self.metrics.height;
                let at = self.cell_at(bounds, position);
                Some(
                    canvas::Action::publish((self.on_event)(TerminalEvent::Wheel { lines, at }))
                        .and_capture(),
                )
            }
            touch::Event::FingerLifted { id, position }
            | touch::Event::FingerLost { id, position } => {
                let track = state.touch.take().filter(|t| t.finger == id)?;
                if track.moved {
                    return Some(canvas::Action::capture());
                }
                let at = self.cell_at(bounds, position);
                Some(
                    canvas::Action::publish((self.on_event)(TerminalEvent::Tapped { at }))
                        .and_capture(),
                )
            }
            touch::Event::FingerPressed { .. } => None,
        }
    }

    fn mouse(
        &self,
        state: &mut ViewState,
        event: &mouse::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            mouse::Event::ButtonPressed(button) => {
                let position = cursor.position_over(bounds)?;
                Some(
                    canvas::Action::publish(self.press(state, bounds, position, *button))
                        .and_capture(),
                )
            }
            mouse::Event::CursorMoved { position } if state.dragging => {
                let at = self.cell_at(bounds, *position);
                Some(canvas::Action::publish((self.on_event)(
                    TerminalEvent::Dragged { at },
                )))
            }
            mouse::Event::ButtonReleased(button) if state.dragging || cursor.is_over(bounds) => {
                state.dragging = false;
                let position = cursor.position().unwrap_or(bounds.center());
                let at = self.cell_at(bounds, position);
                Some(canvas::Action::publish((self.on_event)(
                    TerminalEvent::Released {
                        at,
                        button: *button,
                    },
                )))
            }
            mouse::Event::WheelScrolled { delta } => {
                let position = cursor.position_over(bounds)?;
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / self.metrics.height,
                };
                let at = self.cell_at(bounds, position);
                Some(
                    canvas::Action::publish((self.on_event)(TerminalEvent::Wheel { lines, at }))
                        .and_capture(),
                )
            }
            _ => None,
        }
    }

    fn paint(&self, frame: &mut Frame) {
        let background = to_color(self.theme.terminal.background);
        frame.fill_rectangle(Point::ORIGIN, frame.size(), background);
        if let Some(snapshot) = self.snapshot {
            self.paint_backgrounds(frame, snapshot, background);
            self.paint_text(frame, snapshot);
            self.paint_cursor(frame, snapshot);
        }
        self.paint_edges(frame);
        if let Some(message) = &self.overlay {
            self.paint_overlay(frame, message);
        }
    }

    fn origin(&self, row: u16, col: u16) -> Point {
        Point::new(
            PADDING + f32::from(col) * self.metrics.width,
            PADDING + f32::from(row) * self.metrics.height,
        )
    }

    fn cell_size(&self, wide: bool) -> Size {
        let width = if wide {
            self.metrics.width * 2.0
        } else {
            self.metrics.width
        };
        Size::new(width, self.metrics.height)
    }

    fn paint_backgrounds(&self, frame: &mut Frame, snapshot: &ViewportSnapshot, default_bg: Color) {
        let selection = to_color(self.theme.colors.selection);
        let highlight = Color {
            a: 0.35,
            ..to_color(self.theme.colors.accent)
        };
        for row in 0..snapshot.rows {
            for (col, cell) in snapshot.row(row).iter().enumerate() {
                let col = col as u16;
                let (_, bg) = cell_colors(self.theme, cell);
                let selected = snapshot
                    .selection
                    .is_some_and(|span| span.contains(row, col));
                let fill = if selected { selection } else { bg };
                if fill != default_bg {
                    frame.fill_rectangle(self.origin(row, col), self.cell_size(false), fill);
                }
                if self.is_search_hit(snapshot, row, col) {
                    frame.fill_rectangle(self.origin(row, col), self.cell_size(false), highlight);
                }
            }
        }
    }

    fn is_search_hit(&self, snapshot: &ViewportSnapshot, row: u16, col: u16) -> bool {
        let Some(result) = self.search else {
            return false;
        };
        let line = i32::from(row) - snapshot.display_offset as i32;
        result
            .matches
            .iter()
            .any(|m| m.line == line && col >= m.start_col && col <= m.end_col)
    }

    fn paint_text(&self, frame: &mut Frame, snapshot: &ViewportSnapshot) {
        for row in 0..snapshot.rows {
            for (col, cell) in snapshot.row(row).iter().enumerate() {
                if cell.text.is_blank() || cell.flags.contains(CellFlags::WIDE_SPACER) {
                    self.paint_decorations(frame, cell, row, col as u16);
                    continue;
                }
                let (fg, _) = cell_colors(self.theme, cell);
                let fg = self.selection_text_color(snapshot, row, col as u16, fg);
                self.paint_glyph(
                    frame,
                    &cell.text,
                    cell.flags,
                    self.origin(row, col as u16),
                    fg,
                );
                self.paint_decorations(frame, cell, row, col as u16);
            }
        }
    }

    fn selection_text_color(
        &self,
        snapshot: &ViewportSnapshot,
        row: u16,
        col: u16,
        fg: Color,
    ) -> Color {
        let selected = snapshot
            .selection
            .is_some_and(|span| span.contains(row, col));
        if selected {
            to_color(self.theme.colors.foreground)
        } else {
            fg
        }
    }

    fn paint_glyph(
        &self,
        frame: &mut Frame,
        text: &CellText,
        flags: CellFlags,
        at: Point,
        color: Color,
    ) {
        let content = match text {
            CellText::Char(c) => c.to_string(),
            CellText::Cluster(s) => s.to_string(),
        };
        let ascii = content.is_ascii();
        let font = Font {
            weight: if flags.contains(CellFlags::BOLD) {
                iced::font::Weight::Bold
            } else {
                self.font.weight
            },
            style: if flags.contains(CellFlags::ITALIC) {
                iced::font::Style::Italic
            } else {
                self.font.style
            },
            ..self.font
        };
        frame.fill_text(Text {
            content,
            position: at,
            max_width: f32::INFINITY,
            color,
            size: self.metrics.font_size.into(),
            line_height: LineHeight::Absolute(self.metrics.height.into()),
            font,
            align_x: iced::widget::text::Alignment::Left,
            align_y: alignment::Vertical::Top,
            shaping: if ascii {
                Shaping::Basic
            } else {
                Shaping::Advanced
            },
        });
    }

    fn paint_decorations(
        &self,
        frame: &mut Frame,
        cell: &terminal_core::CellSnapshot,
        row: u16,
        col: u16,
    ) {
        let underline = cell.flags.contains(CellFlags::UNDERLINE);
        let strike = cell.flags.contains(CellFlags::STRIKEOUT);
        if !underline && !strike {
            return;
        }
        let (fg, _) = cell_colors(self.theme, cell);
        let origin = self.origin(row, col);
        let width = self.metrics.width;
        if underline {
            let y = origin.y + self.metrics.height - 2.0;
            frame.fill_rectangle(Point::new(origin.x, y), Size::new(width, 1.0), fg);
        }
        if strike {
            let y = origin.y + self.metrics.height / 2.0;
            frame.fill_rectangle(Point::new(origin.x, y), Size::new(width, 1.0), fg);
        }
    }

    fn paint_cursor(&self, frame: &mut Frame, snapshot: &ViewportSnapshot) {
        let Some(cursor) = snapshot.cursor else {
            return;
        };
        let Some(cell) = snapshot.cell(cursor.row, cursor.col) else {
            return;
        };
        let origin = self.origin(cursor.row, cursor.col);
        let size = self.cell_size(cell.flags.contains(CellFlags::WIDE));
        let color = to_color(self.theme.colors.cursor);
        if !self.focused {
            frame.stroke(
                &Path::rectangle(origin, size),
                Stroke::default().with_color(color).with_width(1.0),
            );
            return;
        }
        match cursor.shape {
            CursorShape::Block => {
                frame.fill_rectangle(origin, size, color);
                if !cell.text.is_blank() {
                    let text_color = to_color(self.theme.terminal.background);
                    self.paint_glyph(frame, &cell.text, cell.flags, origin, text_color);
                }
            }
            CursorShape::HollowBlock => {
                frame.stroke(
                    &Path::rectangle(origin, size),
                    Stroke::default().with_color(color).with_width(1.0),
                );
            }
            CursorShape::Beam => frame.fill_rectangle(origin, Size::new(2.0, size.height), color),
            CursorShape::Underline => {
                let y = origin.y + size.height - 2.0;
                frame.fill_rectangle(Point::new(origin.x, y), Size::new(size.width, 2.0), color);
            }
        }
    }

    /// Focus ring, plus optional static edge lighting. No animation, no blur of glyphs.
    fn paint_edges(&self, frame: &mut Frame) {
        if !self.focused {
            return;
        }
        let accent = to_color(self.theme.colors.accent);
        let size = frame.size();
        let rect = Path::rectangle(
            Point::new(0.5, 0.5),
            Size::new(size.width - 1.0, size.height - 1.0),
        );
        if self.effects == EffectsPreset::Subtle {
            // Static glow: the theme's [style] glow sets its strength (a floor keeps
            // the preset visible for themes without one). Never animated, never over glyphs.
            let strength = self.glow.max(0.18).clamp(0.0, 1.0);
            let outer = Color {
                a: 0.25 * strength,
                ..accent
            };
            frame.stroke(
                &rect,
                Stroke::default()
                    .with_color(outer)
                    .with_width(4.0 + 8.0 * strength),
            );
            let inner = Color {
                a: 0.5 * strength,
                ..accent
            };
            frame.stroke(&rect, Stroke::default().with_color(inner).with_width(2.0));
        }
        frame.stroke(
            &rect,
            Stroke::default()
                .with_color(Color { a: 0.7, ..accent })
                .with_width(1.0),
        );
    }

    fn paint_overlay(&self, frame: &mut Frame, message: &str) {
        let size = frame.size();
        let height = self.metrics.height + 8.0;
        let top = (size.height - height).max(0.0);
        let surface = to_color(self.theme.colors.surface);
        frame.fill_rectangle(
            Point::new(0.0, top),
            Size::new(size.width, height),
            mix(surface, Color::BLACK, 0.1),
        );
        frame.fill_text(Text {
            content: message.to_owned(),
            position: Point::new(PADDING * 2.0, top + 4.0),
            color: to_color(self.theme.colors.foreground),
            size: (self.metrics.font_size * 0.95).into(),
            font: Font::DEFAULT,
            ..Text::default()
        });
    }
}
