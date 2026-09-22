//! Terminal rendering: cell metrics, colour resolution and the GPU surface.

pub mod cells;
pub mod terminal_view;

pub use cells::CellMetrics;
pub use terminal_view::{PADDING, TerminalEvent, TerminalView, ViewState};
