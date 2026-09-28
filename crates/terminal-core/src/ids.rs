//! Typed identifiers shared by the session, UI and render layers.

use std::fmt;

/// Identifies one terminal session. The generation distinguishes a session
/// from an earlier one that reused the same slot, so late events from a
/// closed session can be recognised and discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId {
    pub slot: u32,
    pub generation: u32,
}

impl SessionId {
    pub const fn new(slot: u32, generation: u32) -> Self {
        Self { slot, generation }
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session {}#{}", self.slot, self.generation)
    }
}

/// Stable identifier of a pane in the layout tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(pub u64);

/// Monotonic version of published viewport snapshots for one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SnapshotVersion(pub u64);

impl SnapshotVersion {
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// Resize epoch. Snapshots and hit tests from an older epoch are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ResizeEpoch(pub u64);

impl ResizeEpoch {
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}
