//! Session scheduling: one PTY child, one model owner, bounded channels.
//!
//! Per session there are four threads: a blocking PTY reader, a writer, a
//! lifecycle waiter and the model owner. The GUI thread never blocks on any
//! of them: it sends bounded commands, enqueues input without blocking and
//! reads the latest published state when woken.

mod owner;
mod spawn;
pub mod write_queue;

#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use platform::ExitInfo;
use terminal_core::{
    GridPoint, SearchResult, SelectionKind, SessionId, TermSize, ViewportSnapshot,
};

pub use spawn::{SessionConfig, spawn_session};
pub use write_queue::{InputKind, QueueError};

/// Lifecycle: Creating → Running → Closing → Exited, with Failed from startup or I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Creating,
    Running,
    Closing,
    Exited(ExitInfo),
    Failed(String),
}

impl SessionStatus {
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Creating | Self::Running)
    }
}

/// Commands from the UI to the model owner.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    Resize(TermSize),
    Scroll(i32),
    ScrollToBottom,
    SelectStart(GridPoint, SelectionKind),
    SelectUpdate(GridPoint),
    SelectClear,
    /// Put the selection text into the shared state for the UI to copy.
    CopySelection,
    Search(String),
    RevealLine(i32),
    SetQueryPalette(Box<[(u8, u8, u8); 18]>),
}

/// Wakes the UI. Implementations must not block.
pub trait Notify: Send + Sync {
    fn wake(&self, session: SessionId);
}

/// State published by the owner. The UI takes a short lock only to clone Arcs.
#[derive(Debug, Clone)]
pub struct Published {
    pub snapshot: Option<Arc<ViewportSnapshot>>,
    pub status: SessionStatus,
    pub title: Option<String>,
    pub bells: u32,
    pub copied: Option<String>,
    pub search: Option<Arc<SearchResult>>,
    pub error: Option<String>,
}

impl Default for Published {
    fn default() -> Self {
        Self {
            snapshot: None,
            status: SessionStatus::Creating,
            title: None,
            bells: 0,
            copied: None,
            search: None,
            error: None,
        }
    }
}

pub(crate) struct Shared {
    published: Mutex<Published>,
    wake_pending: AtomicBool,
    /// Set by the UI; checked by the owner on every wake so close is never lost.
    close_requested: AtomicBool,
    notify: Arc<dyn Notify>,
}

impl Shared {
    pub(crate) fn new(notify: Arc<dyn Notify>) -> Self {
        Self {
            published: Mutex::new(Published::default()),
            wake_pending: AtomicBool::new(false),
            close_requested: AtomicBool::new(false),
            notify,
        }
    }

    pub(crate) fn close_requested(&self) -> bool {
        self.close_requested.load(Ordering::Acquire)
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Published> {
        self.published
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Coalesced wakeup: at most one pending notification per session.
    pub(crate) fn wake(&self, id: SessionId) {
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            self.notify.wake(id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the session is busy; try again")]
pub struct SessionBusy;

/// UI-side handle to a running session.
pub struct SessionHandle {
    id: SessionId,
    commands: SyncSender<SessionCommand>,
    doorbell: SyncSender<()>,
    writes: Arc<write_queue::WriteQueue>,
    shared: Arc<Shared>,
    label: String,
    pid: Option<u32>,
}

impl SessionHandle {
    pub fn id(&self) -> SessionId {
        self.id
    }

    /// Profile label, e.g. `zsh` or `pwsh`.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// OS process id of the shell, when the platform reports one.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Send a command without blocking.
    pub fn send(&self, command: SessionCommand) -> Result<(), SessionBusy> {
        match self.commands.try_send(command) {
            Ok(()) => {
                let _ = self.doorbell.try_send(());
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(SessionBusy),
            // A finished owner simply ignores further commands.
            Err(TrySendError::Disconnected(_)) => Ok(()),
        }
    }

    /// Enqueue input bytes without blocking.
    pub fn write(&self, bytes: &[u8], kind: InputKind) -> Result<(), QueueError> {
        self.writes.push_input(bytes, kind)
    }

    pub fn input_busy(&self) -> bool {
        self.writes.input_busy()
    }

    /// Latest published state; clears the pending-wake flag.
    pub fn published(&self) -> Published {
        self.shared.wake_pending.store(false, Ordering::Release);
        self.shared.lock().clone()
    }

    /// Take (and clear) text produced by `CopySelection`.
    pub fn take_copied(&self) -> Option<String> {
        self.shared.lock().copied.take()
    }

    /// Request orderly teardown. Never blocks; teardown runs on session threads.
    pub fn close(&self) {
        self.shared.close_requested.store(true, Ordering::Release);
        let _ = self.doorbell.try_send(());
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.close();
    }
}
