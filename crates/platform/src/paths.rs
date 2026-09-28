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

/// Locate a data file shipped with the application (e.g. `dbip-city-lite.mmdb`).
///
/// Searched in order: next to the executable (Windows zip, Linux tarball),
/// `../Resources` (macOS `.app`), `../share/sci-fi-terminal` (Linux install),
/// and in debug builds the repository's `assets/geo` directory. Only plain
/// file names are accepted, so a caller cannot escape these directories.
pub fn bundled_resource(name: &str) -> Option<PathBuf> {
    let plain = !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..";
    if !plain {
        return None;
    }
    resource_dirs()
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

fn resource_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
    {
        dirs.push(exe_dir.clone());
        dirs.push(exe_dir.join("../Resources"));
        dirs.push(exe_dir.join("../share").join(APP_NAME));
    }
    if cfg!(debug_assertions) {
        dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/geo"));
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_resource_rejects_paths_and_finds_repo_assets() {
        assert_eq!(bundled_resource("../etc/passwd"), None);
        assert_eq!(bundled_resource("a/b"), None);
        assert_eq!(bundled_resource(""), None);
        // The coastline asset is always present in the repository.
        assert!(bundled_resource("coastline-110m.bin").is_some());
        assert_eq!(bundled_resource("definitely-missing.mmdb"), None);
    }

    #[test]
    fn config_dir_is_named_after_the_app_without_extra_levels() {
        if let Some(dir) = config_dir() {
            assert!(dir.ends_with(APP_NAME), "{}", dir.display());
        }
    }
}
