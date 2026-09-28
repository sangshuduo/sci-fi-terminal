//! Tabs, panes and the sessions they own.

use iced::Size;
use iced::widget::canvas::Cache;
use iced::widget::pane_grid::{self, Node, Pane};
use terminal_core::{SnapshotVersion, TermSize};

use crate::render::{CellMetrics, PADDING};
use crate::session::{Published, SessionCommand, SessionHandle, SessionStatus};

/// One pane: a session (or the error that prevented it) plus render caches.
pub struct PaneState {
    pub profile: String,
    pub session: Option<SessionHandle>,
    pub published: Published,
    pub spawn_error: Option<String>,
    pub cache: Cache,
    pub pixel_size: Option<Size>,
    pub scroll_accum: f32,
    last_version: Option<SnapshotVersion>,
}

impl PaneState {
    pub fn new(profile: String, session: Result<SessionHandle, String>) -> Self {
        let (session, spawn_error) = match session {
            Ok(handle) => (Some(handle), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            profile,
            session,
            published: Published::default(),
            spawn_error,
            cache: Cache::new(),
            pixel_size: None,
            scroll_accum: 0.0,
            last_version: None,
        }
    }

    /// Pull the latest published state; invalidate the frame only when it changed.
    pub fn refresh(&mut self) -> Option<String> {
        let session = self.session.as_ref()?;
        self.published = session.published();
        let version = self.published.snapshot.as_ref().map(|s| s.version);
        if version != self.last_version {
            self.last_version = version;
            self.cache.clear();
        }
        session.take_copied()
    }

    pub fn is_live(&self) -> bool {
        self.session.is_some() && self.published.status.is_live()
    }

    pub fn send(&self, command: SessionCommand) {
        if let Some(session) = &self.session {
            let _ = session.send(command);
        }
    }

    /// Title for tabs and pane headers.
    pub fn title(&self) -> String {
        let label = self.session.as_ref().map_or("shell", |s| s.label());
        match &self.published.title {
            Some(title) if !title.is_empty() => title.clone(),
            _ => label.to_owned(),
        }
    }

    /// Visible exit/error state drawn over the terminal.
    pub fn overlay(&self) -> Option<String> {
        if let Some(error) = &self.spawn_error {
            return Some(format!("Could not start shell: {error}"));
        }
        match &self.published.status {
            SessionStatus::Exited(info) if info.success() => {
                Some("Process exited. Close this pane to dismiss.".into())
            }
            SessionStatus::Exited(info) => Some(match &info.signal {
                Some(signal) => format!("Process ended by {signal}. Close this pane to dismiss."),
                None => format!(
                    "Process exited with code {}. Close this pane to dismiss.",
                    info.code
                ),
            }),
            SessionStatus::Failed(message) => Some(format!("Session failed: {message}")),
            SessionStatus::Closing => Some("Closing…".into()),
            _ => self
                .published
                .error
                .as_ref()
                .map(|e| format!("Warning: {e}")),
        }
    }

    /// Recompute the grid for the current pixel size and request a resize if it changed.
    pub fn apply_size(&mut self, metrics: CellMetrics) {
        let Some(size) = self.pixel_size else { return };
        let (columns, rows) =
            metrics.grid_for(size.width - 2.0 * PADDING, size.height - 2.0 * PADDING);
        let term = TermSize::new(columns, rows)
            .with_cell_pixels(metrics.width.round() as u16, metrics.height.round() as u16);
        self.cache.clear();
        self.send(SessionCommand::Resize(term));
    }
}

pub struct Tab {
    pub panes: pane_grid::State<PaneState>,
    pub focus: Pane,
}

impl Tab {
    pub fn new(first: PaneState) -> Self {
        let (panes, focus) = pane_grid::State::new(first);
        Self { panes, focus }
    }

    /// Wrap a restored grid; `None` only for an (impossible) empty grid.
    pub fn from_state(panes: pane_grid::State<PaneState>) -> Option<Self> {
        let focus = ordered_panes(&panes).into_iter().next()?;
        Some(Self { panes, focus })
    }

    pub fn focused(&self) -> Option<&PaneState> {
        self.panes.get(self.focus)
    }

    pub fn focused_mut(&mut self) -> Option<&mut PaneState> {
        self.panes.get_mut(self.focus)
    }

    /// Move focus forwards or backwards in layout order.
    pub fn cycle_focus(&mut self, forward: bool) {
        let order = ordered_panes(&self.panes);
        let Some(index) = order.iter().position(|pane| *pane == self.focus) else {
            return;
        };
        let len = order.len();
        let next = if forward {
            (index + 1) % len
        } else {
            (index + len - 1) % len
        };
        self.focus = order[next];
    }

    pub fn title(&self) -> String {
        self.focused()
            .map_or_else(|| "shell".into(), PaneState::title)
    }
}

/// Panes in depth-first layout order (left/top before right/bottom).
pub fn ordered_panes<T>(state: &pane_grid::State<T>) -> Vec<Pane> {
    fn walk(node: &Node, out: &mut Vec<Pane>) {
        match node {
            Node::Pane(pane) => out.push(*pane),
            Node::Split { a, b, .. } => {
                walk(a, out);
                walk(b, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(state.layout(), &mut out);
    out
}

/// Split handles in a layout, used to equalise ratios on reset.
pub fn splits<T>(state: &pane_grid::State<T>) -> Vec<pane_grid::Split> {
    fn walk(node: &Node, out: &mut Vec<pane_grid::Split>) {
        if let Node::Split { id, a, b, .. } = node {
            out.push(*id);
            walk(a, out);
            walk(b, out);
        }
    }
    let mut out = Vec::new();
    walk(state.layout(), &mut out);
    out
}
