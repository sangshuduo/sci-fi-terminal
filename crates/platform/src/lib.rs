//! Platform services: PTY backend, shell discovery, paths and process lifecycle.
//!
//! Everything OS-specific lives behind the small traits in [`pty`] so the
//! session layer can be exercised headlessly with a fake transport.

mod error;
pub mod paths;
pub mod pty;
pub mod shell;

pub use error::PlatformError;
pub use pty::{
    ChildWaiter, ExitInfo, NativePty, ProcessKiller, PtyBackend, PtyControl, PtyProcess, PtySize,
};
pub use shell::{ShellSpec, default_shell, resolve_profile};
