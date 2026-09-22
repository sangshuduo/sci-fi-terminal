//! Read-only directory viewer that follows the focused shell's working directory.
//!
//! Listing happens on the monitor worker thread; the UI only renders the
//! result. Nothing here runs commands or opens files. The only
//! terminal-affecting action is typing a quoted `cd` without pressing Enter.

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

use iced::widget::{Space, button, column, row, text};
use iced::{Element, Length};

/// Entries listed at most; larger directories are summarised.
pub const MAX_ENTRIES: usize = 500;
/// Entries rendered in the panel.
const SHOWN_ENTRIES: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub path: PathBuf,
    pub entries: Vec<DirEntryInfo>,
    /// Entries beyond `MAX_ENTRIES` that were not listed.
    pub omitted: usize,
}

/// List a directory: directories first, then case-insensitive names.
/// Symlinks are reported, not followed, for metadata.
pub fn list_directory(path: &Path, show_hidden: bool) -> Result<Listing, String> {
    let reader = fs::read_dir(path)
        .map_err(|err| format!("cannot read {}: {}", path.display(), err.kind()))?;
    let mut entries = Vec::new();
    let mut omitted = 0;
    for entry in reader.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            omitted += 1;
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let is_symlink = meta.file_type().is_symlink();
        let is_dir = meta.is_dir() || (is_symlink && entry.path().is_dir());
        entries.push(DirEntryInfo {
            name: sanitize(&name),
            is_dir,
            is_symlink,
            size: meta.len(),
        });
    }
    entries.sort_by(order);
    Ok(Listing {
        path: path.to_path_buf(),
        entries,
        omitted,
    })
}

fn order(a: &DirEntryInfo, b: &DirEntryInfo) -> Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
}

/// File names can contain control characters; never render them raw.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}

/// POSIX single-quote a path for display in a `cd` command.
pub fn shell_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The text typed (without Enter) by the "Insert cd" action.
pub fn cd_command(path: &Path) -> String {
    if cfg!(windows) {
        format!("cd \"{}\"", path.to_string_lossy().replace('"', ""))
    } else {
        format!("cd -- {}", shell_quote(path))
    }
}

/// What the directory viewer is showing.
#[derive(Debug, Clone, PartialEq)]
pub enum FilesState {
    /// No focused shell, or its working directory is not exposed by the OS.
    Unknown,
    Error(String),
    Listed(Listing),
}

/// Messages the viewer emits; the app decides what they do.
#[derive(Debug, Clone, PartialEq)]
pub enum FilesMsg {
    /// Type `cd -- '<path>'` into the focused terminal without Enter.
    InsertCd(PathBuf),
}

pub fn view<'a, M: Clone + 'a>(
    state: &'a FilesState,
    wrap: impl Fn(FilesMsg) -> M + Copy + 'a,
) -> Element<'a, M> {
    let listing = match state {
        FilesState::Unknown => return text("Working directory unavailable").size(12).into(),
        FilesState::Error(message) => return text(message.as_str()).size(12).into(),
        FilesState::Listed(listing) => listing,
    };
    let header = text(abbreviate(&listing.path)).size(12);
    let mut list = column![].spacing(1);
    for entry in listing.entries.iter().take(SHOWN_ENTRIES) {
        let marker = match (entry.is_dir, entry.is_symlink) {
            (true, _) => "▸",
            (false, true) => "↪",
            (false, false) => " ",
        };
        let size = if entry.is_dir {
            String::new()
        } else {
            crate::panels::format_bytes(entry.size)
        };
        let label = row![
            text(marker).size(12).width(12),
            text(entry.name.as_str()).size(12).width(Length::Fill),
            text(size).size(11)
        ]
        .spacing(4);
        let item: Element<'a, M> = if entry.is_dir {
            let target = listing.path.join(&entry.name);
            button(label)
                .padding([1, 2])
                .style(button::text)
                .on_press(wrap(FilesMsg::InsertCd(target)))
                .into()
        } else {
            row![Space::new().width(2), label].into()
        };
        list = list.push(item);
    }
    let hidden = listing.entries.len().saturating_sub(SHOWN_ENTRIES) + listing.omitted;
    if hidden > 0 {
        list = list.push(text(format!("… {hidden} more")).size(11));
    }
    column![
        header,
        text("Click a folder to type its cd command (Enter not pressed)").size(10),
        list
    ]
    .spacing(4)
    .into()
}

/// Replace the home directory prefix with `~` for display only.
fn abbreviate(path: &Path) -> String {
    match platform::paths::home_dir()
        .and_then(|home| path.strip_prefix(&home).ok().map(Path::to_path_buf))
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sft-files-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn lists_directories_first_and_hides_dotfiles() {
        let dir = temp_dir("order");
        fs::write(dir.join("b.txt"), "hi").expect("write");
        fs::write(dir.join(".secret"), "x").expect("write");
        fs::create_dir(dir.join("Zeta")).expect("mkdir");
        fs::create_dir(dir.join("alpha")).expect("mkdir");
        let listing = list_directory(&dir, false).expect("list");
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Zeta", "b.txt"]);
        assert_eq!(listing.entries[2].size, 2);
        let with_hidden = list_directory(&dir, true).expect("list");
        assert!(with_hidden.entries.iter().any(|e| e.name == ".secret"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn caps_large_directories() {
        let dir = temp_dir("cap");
        for i in 0..(MAX_ENTRIES + 7) {
            fs::write(dir.join(format!("f{i}")), "").expect("write");
        }
        let listing = list_directory(&dir, false).expect("list");
        assert_eq!(listing.entries.len(), MAX_ENTRIES);
        assert_eq!(listing.omitted, 7);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_directory_is_an_error_not_a_panic() {
        assert!(list_directory(Path::new("/definitely/not/here"), false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cd_command_quotes_hostile_names() {
        let path = Path::new("/tmp/it's; rm -rf ~");
        assert_eq!(cd_command(path), r"cd -- '/tmp/it'\''s; rm -rf ~'");
        assert!(!cd_command(path).contains('\n'));
    }

    #[test]
    fn control_characters_in_names_are_masked() {
        assert_eq!(sanitize("a\x1b[31mb\n"), "a�[31mb�");
    }
}
