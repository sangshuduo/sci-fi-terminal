//! Hover previews for the directory viewer: the first lines of a text file or
//! a thumbnail of an image.
//!
//! Loading happens on one background thread where only the newest request
//! matters: a burst of hovers is debounced and only the last file is read.
//! Every read is bounded. Files are opened non-blocking and must be regular
//! files, so a FIFO or device can never stall the loader. Nothing is executed,
//! and text is shown with control characters masked.

use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use iced::widget::image::Handle;

/// Wait for the pointer to settle before reading anything.
const DEBOUNCE: Duration = Duration::from_millis(120);
/// Bytes read for a text preview (and for sniffing an image's format).
const TEXT_BYTES: u64 = 16 * 1024;
/// Largest image file decoded.
const MAX_IMAGE_FILE: u64 = 32 * 1024 * 1024;
/// Decoder limits against decompression bombs.
const MAX_IMAGE_SIDE: u32 = 16_384;
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;
/// Thumbnail bounds in pixels.
pub const THUMB_WIDTH: u32 = 360;
pub const THUMB_HEIGHT: u32 = 270;
/// Text shown at most.
const MAX_LINES: usize = 30;
const MAX_LINE_CHARS: usize = 100;
const TAB: &str = "    ";

/// What the preview window shows for one file.
#[derive(Debug, Clone)]
pub enum FilePreview {
    Text {
        lines: Vec<String>,
        /// More text exists beyond what is shown.
        truncated: bool,
    },
    Image {
        handle: Handle,
        /// Original size, for the caption.
        width: u32,
        height: u32,
    },
    /// Nothing to show, with the reason (binary, too large, unreadable…).
    Note(String),
}

/// The file under the pointer and, once loaded, its preview.
#[derive(Debug, Clone)]
pub struct HoverPreview {
    pub path: PathBuf,
    pub content: Option<FilePreview>,
}

/// Build a preview. Blocking and bounded; call off the GUI thread.
pub fn load(path: &Path) -> FilePreview {
    match load_inner(path) {
        Ok(preview) => preview,
        Err(note) => FilePreview::Note(note),
    }
}

fn load_inner(path: &Path) -> Result<FilePreview, String> {
    let file = open_regular(path)?;
    let len = file
        .metadata()
        .map_err(|err| format!("Cannot read: {}", err.kind()))?
        .len();
    let mut head = Vec::new();
    file.take(TEXT_BYTES)
        .read_to_end(&mut head)
        .map_err(|err| format!("Cannot read: {}", err.kind()))?;
    if image::guess_format(&head).is_ok() {
        return load_image(path, len);
    }
    text_preview(&head, len > TEXT_BYTES)
}

/// Open without following into FIFOs/devices: non-blocking on Unix, then the
/// opened handle itself must be a regular file.
fn open_regular(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|err| format!("Cannot open: {}", err.kind()))?;
    let is_file = file.metadata().is_ok_and(|meta| meta.is_file());
    if !is_file {
        return Err("Not a regular file".into());
    }
    Ok(file)
}

fn load_image(path: &Path, len: u64) -> Result<FilePreview, String> {
    if len > MAX_IMAGE_FILE {
        return Err("Image too large to preview".into());
    }
    let mut bytes = Vec::new();
    open_regular(path)?
        .take(MAX_IMAGE_FILE)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("Cannot read: {}", err.kind()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| format!("Cannot read image: {err}"))?;
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|err| format!("Cannot preview image: {err}"))?;
    let (width, height) = (decoded.width(), decoded.height());
    let thumb = decoded.thumbnail(THUMB_WIDTH, THUMB_HEIGHT).into_rgba8();
    let (tw, th) = thumb.dimensions();
    Ok(FilePreview::Image {
        handle: Handle::from_rgba(tw, th, thumb.into_raw()),
        width,
        height,
    })
}

/// UTF-8 text only; a NUL byte or invalid sequence means binary.
fn text_preview(head: &[u8], more: bool) -> Result<FilePreview, String> {
    if head.is_empty() {
        return Err("Empty file".into());
    }
    if head.contains(&0) {
        return Err("Binary file".into());
    }
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        // Cut in the middle of a character at the read boundary: keep the rest.
        Err(err) if err.error_len().is_none() => {
            std::str::from_utf8(&head[..err.valid_up_to()]).map_err(|_| "Binary file")?
        }
        Err(_) => return Err("Binary or non-UTF-8 file".into()),
    };
    let mut lines: Vec<String> = text.lines().take(MAX_LINES).map(clean_line).collect();
    let truncated = more || text.lines().count() > MAX_LINES;
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    Ok(FilePreview::Text { lines, truncated })
}

/// Expand tabs, mask control characters, bound the width.
fn clean_line(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars() {
        if out.chars().count() >= MAX_LINE_CHARS {
            out.push('…');
            break;
        }
        match c {
            '\t' => out.push_str(TAB),
            '\r' => {}
            c if c.is_control() => out.push('�'),
            c => out.push(c),
        }
    }
    out
}

/// Background loader; dropping it stops the thread after its current file.
pub struct PreviewLoader {
    requests: Sender<PathBuf>,
}

impl PreviewLoader {
    pub fn start(sink: impl Fn(PathBuf, FilePreview) + Send + 'static) -> Option<Self> {
        let (requests, receiver) = channel();
        std::thread::Builder::new()
            .name("file-preview".into())
            .spawn(move || run(&receiver, &sink))
            .ok()?;
        Some(Self { requests })
    }

    pub fn request(&self, path: PathBuf) {
        let _ = self.requests.send(path);
    }
}

fn run(receiver: &Receiver<PathBuf>, sink: &dyn Fn(PathBuf, FilePreview)) {
    while let Ok(mut path) = receiver.recv() {
        loop {
            match receiver.recv_timeout(DEBOUNCE) {
                Ok(newer) => path = newer,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let preview = load(&path);
        sink(path, preview);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sft-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }

    fn note(preview: FilePreview) -> String {
        match preview {
            FilePreview::Note(note) => note,
            other => panic!("expected a note, got {other:?}"),
        }
    }

    #[test]
    fn text_files_show_clean_bounded_lines() {
        let body = "fn main() {\n\tprintln!(\"hi\x1b[31m\");\r\n}\n";
        let FilePreview::Text { lines, truncated } = load(&scratch("a.rs", body.as_bytes())) else {
            panic!("expected text");
        };
        assert_eq!(lines[1], "    println!(\"hi�[31m\");");
        assert_eq!(lines.len(), 3);
        assert!(!truncated);

        let long: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let FilePreview::Text { lines, truncated } = load(&scratch("long.txt", long.as_bytes()))
        else {
            panic!("expected text");
        };
        assert_eq!(lines.len(), MAX_LINES);
        assert!(truncated);
        assert!(clean_line(&"x".repeat(500)).chars().count() <= MAX_LINE_CHARS + 1);
    }

    #[test]
    fn binary_empty_and_missing_files_are_notes() {
        assert_eq!(
            note(load(&scratch("bin", b"\x7fELF\0\0\x01"))),
            "Binary file"
        );
        assert_eq!(
            note(load(&scratch("latin1", b"caf\xe9 au lait"))),
            "Binary or non-UTF-8 file"
        );
        assert_eq!(note(load(&scratch("empty", b""))), "Empty file");
        assert!(note(load(Path::new("/definitely/not/here"))).starts_with("Cannot open"));
        assert_eq!(note(load(&std::env::temp_dir())), "Not a regular file");
    }

    #[test]
    fn utf8_cut_at_the_read_boundary_is_still_text() {
        let mut bytes = vec![b'a'; TEXT_BYTES as usize - 1];
        bytes.extend("é and more".as_bytes());
        let path = scratch("boundary.txt", &bytes);
        assert!(matches!(
            load(&path),
            FilePreview::Text {
                truncated: true,
                ..
            }
        ));
    }

    #[test]
    fn images_become_bounded_thumbnails() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(800, 400, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        let FilePreview::Image { width, height, .. } = load(&scratch("red.png", &png)) else {
            panic!("expected image");
        };
        assert_eq!((width, height), (800, 400));
        // A PNG header with a corrupt body is reported, not a panic.
        let broken = &png[..40];
        assert!(note(load(&scratch("broken.png", broken))).starts_with("Cannot preview image"));
    }

    #[cfg(unix)]
    #[test]
    fn fifos_never_block_the_loader() {
        let path = std::env::temp_dir().join(format!("sft-preview-fifo-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let made = std::process::Command::new("mkfifo").arg(&path).status();
        if !made.is_ok_and(|status| status.success()) {
            eprintln!("mkfifo unavailable; skipping");
            return;
        }
        assert_eq!(note(load(&path)), "Not a regular file");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn loader_debounces_to_the_newest_request() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = Arc::clone(&seen);
        let loader = PreviewLoader::start(move |path, _| {
            sink_seen.lock().expect("lock").push(path);
        })
        .expect("thread");
        let a = scratch("first.txt", b"a");
        let b = scratch("second.txt", b"b");
        loader.request(a);
        loader.request(b.clone());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while seen.lock().expect("lock").is_empty() {
            assert!(std::time::Instant::now() < deadline, "no preview");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*seen.lock().expect("lock"), vec![b]);
    }
}
