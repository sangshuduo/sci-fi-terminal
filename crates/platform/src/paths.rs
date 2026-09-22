//! Platform directories resolved through OS conventions, never hardcoded users.

use std::path::PathBuf;

use directories::{BaseDirs, ProjectDirs};

const APP_NAME: &str = "sci-fi-terminal";

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", APP_NAME)
}

/// Configuration directory (`$XDG_CONFIG_HOME/sci-fi-terminal`, Application Support, `%APPDATA%`).
pub fn config_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().to_path_buf())
}

/// Runtime cache directory; never used for configuration.
pub fn cache_dir() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.cache_dir().to_path_buf())
}

/// The user's home directory, used as the visible fallback working directory.
pub fn home_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}
