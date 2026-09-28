//! Terminal model adapter, immutable viewport snapshots and input encoding.
//!
//! This crate is the only place that touches the third-party terminal engine.
//! Nothing here depends on the GUI, GPU, clipboard or PTY APIs: callers feed
//! bytes in, and read typed snapshots, host effects and protocol replies out.

mod adapter;
mod ids;
pub mod input;
mod policy;
mod search;
mod snapshot;

pub use adapter::{GridPoint, ModelEvent, SelectionKind, TermSize, TerminalModel};
pub use ids::{PaneId, ResizeEpoch, SessionId, SnapshotVersion};
pub use policy::{Limits, sanitize_title};
pub use search::{SearchMatch, SearchResult};
pub use snapshot::{
    CellColor, CellFlags, CellSnapshot, CellText, CursorShape, CursorSnapshot, Damage, ModeFlags,
    SelectionSpan, ViewportSnapshot,
};
