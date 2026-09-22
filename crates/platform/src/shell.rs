//! Shell discovery and launch specifications.
//!
//! A shell is always spawned directly from an argument vector; settings are
//! never concatenated into a command string.

use std::path::{Path, PathBuf};

use crate::error::PlatformError;

/// Everything needed to spawn one session's child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Explicit environment overrides applied on top of the inherited environment.
    pub env: Vec<(String, String)>,
    /// Human-readable profile label shown in the UI.
    pub label: String,
}

impl ShellSpec {
    fn new(program: PathBuf, args: Vec<String>, label: String) -> Self {
        Self {
            program,
            args,
            cwd: None,
            env: Vec::new(),
            label,
        }
    }
}

/// Discover the OS default shell.
///
/// Unix: account database, then a validated `SHELL`, then `/bin/sh`.
/// Windows: PowerShell if present, otherwise `ComSpec`/`cmd.exe`.
pub fn default_shell(login: bool) -> ShellSpec {
    let program = discover_default_program();
    let label = program_label(&program);
    let args = interactive_args(&program, login);
    ShellSpec::new(program, args, label)
}

/// Build a spec from a configured profile. An empty executable means discover the default.
pub fn resolve_profile(
    executable: &str,
    args: &[String],
    login: bool,
    cwd: Option<&Path>,
    env: &[(String, String)],
) -> Result<ShellSpec, PlatformError> {
    if executable.contains('\0') || args.iter().any(|a| a.contains('\0')) {
        return Err(PlatformError::InvalidProfile(
            "arguments may not contain NUL".into(),
        ));
    }
    let mut spec = if executable.is_empty() {
        default_shell(login)
    } else {
        let program = locate_program(executable)
            .ok_or_else(|| PlatformError::ShellNotFound(executable.to_owned()))?;
        let mut base = interactive_args(&program, login);
        base.extend(args.iter().cloned());
        ShellSpec::new(program.clone(), base, program_label(&program))
    };
    if executable.is_empty() {
        spec.args.extend(args.iter().cloned());
    }
    if let Some(dir) = cwd {
        if !dir.is_dir() {
            return Err(PlatformError::MissingCwd(dir.to_path_buf()));
        }
        spec.cwd = Some(dir.to_path_buf());
    }
    spec.env = env.to_vec();
    Ok(spec)
}

fn program_label(program: &Path) -> String {
    program
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "shell".to_owned())
}

/// Login-shell arguments are only added for shells known to accept `-l`.
fn interactive_args(program: &Path, login: bool) -> Vec<String> {
    let name = program_label(program).to_ascii_lowercase();
    let accepts_login = matches!(name.as_str(), "bash" | "zsh" | "fish" | "ksh" | "mksh");
    if cfg!(windows) && (name == "pwsh" || name == "powershell") {
        return vec!["-NoLogo".to_owned()];
    }
    if login && accepts_login {
        vec!["-l".to_owned()]
    } else {
        Vec::new()
    }
}

/// Resolve a bare program name through `PATH`, or accept an existing path.
fn locate_program(executable: &str) -> Option<PathBuf> {
    let candidate = Path::new(executable);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        executable_names(executable)
            .into_iter()
            .map(|name| dir.join(name))
            .find(|full| is_executable(full))
    })
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) && Path::new(name).extension().is_none() {
        vec![
            format!("{name}.exe"),
            format!("{name}.cmd"),
            name.to_owned(),
        ]
    } else {
        vec![name.to_owned()]
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn discover_default_program() -> PathBuf {
    // portable-pty consults the account database when SHELL is absent from the builder env.
    let mut builder = portable_pty::CommandBuilder::new_default_prog();
    builder.env_remove("SHELL");
    let from_account = PathBuf::from(builder.get_shell());
    if from_account != Path::new("/bin/sh") && is_executable(&from_account) {
        return from_account;
    }
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|shell| shell.is_absolute() && is_executable(shell))
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

#[cfg(windows)]
fn discover_default_program() -> PathBuf {
    ["pwsh", "powershell"]
        .iter()
        .find_map(|name| locate_program(name))
        .or_else(|| std::env::var_os("ComSpec").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("cmd.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_is_executable() {
        let spec = default_shell(false);
        assert!(spec.program.is_absolute() || cfg!(windows));
        assert!(!spec.label.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn login_flag_only_for_known_shells() {
        assert_eq!(interactive_args(Path::new("/bin/zsh"), true), vec!["-l"]);
        assert!(interactive_args(Path::new("/bin/dash"), true).is_empty());
        assert!(interactive_args(Path::new("/bin/zsh"), false).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn profile_resolves_through_path_and_keeps_argv() {
        let spec = resolve_profile("sh", &["-i".into()], false, None, &[]).expect("sh on PATH");
        assert!(spec.program.ends_with("sh"));
        assert_eq!(spec.args, vec!["-i"]);
    }

    #[test]
    fn missing_program_and_cwd_are_reported() {
        let missing = resolve_profile("definitely-not-a-shell-xyz", &[], false, None, &[]);
        assert!(matches!(missing, Err(PlatformError::ShellNotFound(_))));
        let cwd = Path::new("/definitely/not/here");
        let bad_cwd = resolve_profile("", &[], false, Some(cwd), &[]);
        assert!(matches!(bad_cwd, Err(PlatformError::MissingCwd(_))));
    }

    #[test]
    fn nul_bytes_are_rejected() {
        let result = resolve_profile("sh\0x", &[], false, None, &[]);
        assert!(matches!(result, Err(PlatformError::InvalidProfile(_))));
    }
}
