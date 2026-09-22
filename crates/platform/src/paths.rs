//! Platform directories resolved through OS conventions, never hardcoded users.

use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};

const APP_NAME: &str = "sci-fi-terminal";

/// Configuration directory, exactly as documented in CONFIGURATION.md:
/// `$XDG_CONFIG_HOME/sci-fi-terminal` (Linux), `~/Library/Application Support/sci-fi-terminal`
/// (macOS) and `%APPDATA%\sci-fi-terminal` (Windows).
pub fn config_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.config_dir().join(APP_NAME))
}

/// Runtime cache directory; never used for configuration.
pub fn cache_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", APP_NAME).map(|dirs| dirs.cache_dir().to_path_buf())
}

/// The user's home directory, used as the visible fallback working directory.
pub fn home_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_is_named_after_the_app_without_extra_levels() {
        if let Some(dir) = config_dir() {
            assert!(dir.ends_with(APP_NAME), "{}", dir.display());
        }
    }
}
