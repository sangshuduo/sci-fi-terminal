//! Configuration loading, layered precedence merge and atomic writes.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use toml::{Table, Value};

use super::schema::Config;
use super::validate::{Diagnostic, schema_version_diagnostic, validate};

/// Maximum size of any config or theme file.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Errors produced while loading or saving configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{path}: cannot read: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path}: file exceeds 1 MiB limit")]
    TooLarge { path: PathBuf },
    #[error("{path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("invalid configuration")]
    Invalid(Vec<Diagnostic>),
    #[error("{path}: cannot write: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Well-known configuration file locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub overrides_file: PathBuf,
    pub layout_file: PathBuf,
    pub themes_dir: PathBuf,
}

impl ConfigPaths {
    /// Derives all file paths from a configuration directory.
    pub fn from_dir(dir: PathBuf) -> Self {
        Self {
            config_file: dir.join("config.toml"),
            overrides_file: dir.join("ui-overrides.toml"),
            layout_file: dir.join("layout.toml"),
            themes_dir: dir.join("themes"),
            config_dir: dir,
        }
    }

    /// Platform configuration directory (XDG on Linux, Application Support on macOS,
    /// `%APPDATA%` on Windows).
    pub fn platform_default() -> Option<Self> {
        platform::paths::config_dir().map(Self::from_dir)
    }
}

/// Result of layered loading; always holds a usable, validated config.
#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub diagnostics: Vec<ConfigError>,
    /// Files that were successfully applied, in precedence order.
    pub sources: Vec<PathBuf>,
}

/// Converts a TOML deserialization error into a located [`ConfigError::Parse`].
pub(crate) fn parse_error(text: &str, path: &Path, err: &toml::de::Error) -> ConfigError {
    let message = match err.span() {
        Some(span) => {
            let (line, col) = line_col(text, span.start);
            format!("line {line}, column {col}: {}", err.message())
        }
        None => err.message().to_owned(),
    };
    ConfigError::Parse {
        path: path.to_path_buf(),
        message,
    }
}

fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let prefix = text.get(..offset).unwrap_or(text);
    let line = prefix.matches('\n').count() + 1;
    let col = prefix
        .rfind('\n')
        .map_or(prefix.len(), |nl| prefix.len() - nl - 1)
        + 1;
    (line, col)
}

fn attribute(diagnostics: Vec<Diagnostic>, path: &Path) -> ConfigError {
    ConfigError::Invalid(
        diagnostics
            .into_iter()
            .map(|d| d.with_file(path.to_path_buf()))
            .collect(),
    )
}

/// Rejects files declaring a newer schema before strict field checks run.
fn check_declared_version(table: &Table, path: &Path) -> Result<(), ConfigError> {
    let Some(version) = table.get("schema_version").and_then(Value::as_integer) else {
        return Ok(());
    };
    let version = u32::try_from(version).unwrap_or(u32::MAX);
    match schema_version_diagnostic(version) {
        Some(diag) => Err(attribute(vec![diag], path)),
        None => Ok(()),
    }
}

/// Parses and validates one config file's text.
pub fn parse_config(text: &str, path: &Path) -> Result<Config, ConfigError> {
    parse_config_with_table(text, path).map(|(config, _)| config)
}

fn parse_config_with_table(text: &str, path: &Path) -> Result<(Config, Table), ConfigError> {
    let table: Table = toml::from_str(text).map_err(|e| parse_error(text, path, &e))?;
    check_declared_version(&table, path)?;
    let config: Config = toml::from_str(text).map_err(|e| parse_error(text, path, &e))?;
    let diagnostics = validate(&config);
    if diagnostics.is_empty() {
        Ok((config, table))
    } else {
        Err(attribute(diagnostics, path))
    }
}

/// Reads a UTF-8 file up to [`MAX_CONFIG_BYTES`]; `None` if it does not exist.
pub(crate) fn read_limited(path: &Path) -> Result<Option<String>, ConfigError> {
    let io_err = |source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    };
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(e)),
    };
    if file.metadata().map_err(io_err)?.len() > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    let mut text = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(io_err)?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge {
            path: path.to_path_buf(),
        });
    }
    Ok(Some(text))
}

/// Loads and validates a single config file; `Ok(None)` if it is missing.
pub fn load_file(path: &Path) -> Result<Option<Config>, ConfigError> {
    read_limited(path)?
        .map(|text| parse_config(&text, path))
        .transpose()
}

fn load_layer(path: &Path) -> Result<Option<Table>, ConfigError> {
    let Some(text) = read_limited(path)? else {
        return Ok(None);
    };
    parse_config_with_table(&text, path).map(|(_, table)| Some(table))
}

/// Merges `overlay` into `base`: tables by key (recursive), arrays replace,
/// except `keybindings` which merge keyed by `(action, context)`.
pub(crate) fn merge_tables(base: &Table, overlay: &Table) -> Table {
    let mut merged = base.clone();
    for (key, value) in overlay {
        let next = match (merged.get(key), value) {
            (Some(Value::Table(b)), Value::Table(o)) => Value::Table(merge_tables(b, o)),
            (Some(Value::Array(b)), Value::Array(o)) if key == "keybindings" => {
                Value::Array(merge_keybindings(b, o))
            }
            _ => value.clone(),
        };
        merged.insert(key.clone(), next);
    }
    merged
}

fn binding_key(value: &Value) -> (Option<&str>, Option<&str>) {
    let get = |k| value.get(k).and_then(Value::as_str);
    (get("action"), get("context"))
}

fn merge_keybindings(base: &[Value], overlay: &[Value]) -> Vec<Value> {
    let mut merged = base.to_vec();
    for entry in overlay {
        let key = binding_key(entry);
        match merged.iter().position(|b| binding_key(b) == key) {
            Some(i) => merged[i] = entry.clone(),
            None => merged.push(entry.clone()),
        }
    }
    merged
}

fn defaults_table() -> Table {
    Table::try_from(Config::default()).unwrap_or_default()
}

fn finalize(table: Table) -> Result<Config, ConfigError> {
    let config: Config = Value::Table(table)
        .try_into()
        .map_err(|e: toml::de::Error| {
            ConfigError::Invalid(vec![Diagnostic::new("", e.message())])
        })?;
    let diagnostics = validate(&config);
    if diagnostics.is_empty() {
        Ok(config)
    } else {
        Err(ConfigError::Invalid(diagnostics))
    }
}

/// Loads defaults → `config.toml` → `ui-overrides.toml` (skipped in safe mode).
/// Broken files are reported and skipped; never panics and never writes.
pub fn load_effective(paths: &ConfigPaths, safe_mode: bool) -> LoadedConfig {
    let mut layers = vec![paths.config_file.as_path()];
    if !safe_mode {
        layers.push(paths.overrides_file.as_path());
    }
    let mut merged = defaults_table();
    let mut diagnostics = Vec::new();
    let mut sources = Vec::new();
    for path in layers {
        match load_layer(path) {
            Ok(Some(table)) => {
                let candidate = merge_tables(&merged, &table);
                match finalize(candidate.clone()) {
                    Ok(_) => {
                        merged = candidate;
                        sources.push(path.to_path_buf());
                    }
                    Err(ConfigError::Invalid(diags)) => diagnostics.push(attribute(diags, path)),
                    Err(err) => diagnostics.push(err),
                }
            }
            Ok(None) => {}
            Err(err) => diagnostics.push(err),
        }
    }
    let config = finalize(merged).unwrap_or_else(|err| {
        diagnostics.push(err);
        Config::default()
    });
    LoadedConfig {
        config,
        diagnostics,
        sources,
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "config".into(), |n| n.to_string_lossy().into_owned());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let unique = format!(".{name}.tmp-{}-{nanos}-{seq}", std::process::id());
    path.with_file_name(unique)
}

fn backup_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

fn create_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn write_temp(temp: &Path, contents: &str) -> io::Result<()> {
    let mut file = create_private(temp)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()
}

fn replace_with(temp: &Path, path: &Path) -> io::Result<()> {
    if path.try_exists()? {
        fs::copy(path, backup_path_for(path))?;
    }
    fs::rename(temp, path)
}

/// Atomically replaces `path` with `contents` via a private same-directory temp file,
/// keeping the previous file as `<name>.bak`.
pub fn write_atomic(path: &Path, contents: &str) -> Result<(), ConfigError> {
    let write_err = |source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(write_err)?;
    }
    let temp = temp_path_for(path);
    let result = write_temp(&temp, contents).and_then(|()| replace_with(&temp, path));
    if result.is_err() {
        // Best effort cleanup; the original error is what matters.
        let _ = fs::remove_file(&temp);
    }
    result.map_err(write_err)
}

/// Validates `overrides` and writes it to the UI-managed overrides file.
pub fn save_overrides(paths: &ConfigPaths, overrides: &Config) -> Result<(), ConfigError> {
    let diagnostics = validate(overrides);
    if !diagnostics.is_empty() {
        return Err(ConfigError::Invalid(diagnostics));
    }
    let text = toml::to_string_pretty(overrides).map_err(|e| ConfigError::Parse {
        path: paths.overrides_file.clone(),
        message: format!("cannot serialize: {e}"),
    })?;
    write_atomic(&paths.overrides_file, &text)
}
