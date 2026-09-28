use std::path::PathBuf;

/// Typed platform errors. Messages are safe to show: they never include
/// environment values or terminal content.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("could not open a pseudo-terminal: {0}")]
    OpenPty(String),
    #[error("could not start shell `{program}`: {reason}")]
    Spawn { program: String, reason: String },
    #[error("shell `{0}` was not found or is not executable")]
    ShellNotFound(String),
    #[error("working directory {0} does not exist")]
    MissingCwd(PathBuf),
    #[error("invalid profile: {0}")]
    InvalidProfile(String),
    #[error("terminal resize failed: {0}")]
    Resize(String),
    #[error("waiting for the shell failed: {0}")]
    Wait(#[source] std::io::Error),
}
