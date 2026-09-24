//! Headless session tests against an in-memory PTY transport.

use std::io::{self, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use platform::{
    ChildWaiter, ExitInfo, PlatformError, ProcessKiller, PtyBackend, PtyControl, PtyProcess,
    PtySize, ShellSpec,
};
use terminal_core::{Limits, SessionId, TermSize};

use super::*;

/// Test-side controls for a fake child.
struct FakeChild {
    output: Sender<Vec<u8>>,
    written: Arc<Mutex<Vec<u8>>>,
    exit: Sender<ExitInfo>,
    resizes: Arc<Mutex<Vec<PtySize>>>,
    killed: Arc<Mutex<bool>>,
}

struct FakeBackend {
    slot: Mutex<Option<FakeParts>>,
    fail_resize: bool,
    /// When false the child never reads input, so writes block.
    child_reads_input: bool,
}

struct FakeParts {
    output: Receiver<Vec<u8>>,
    written: Arc<Mutex<Vec<u8>>>,
    exit: Receiver<ExitInfo>,
    exit_tx: Sender<ExitInfo>,
    resizes: Arc<Mutex<Vec<PtySize>>>,
    killed: Arc<Mutex<bool>>,
}

impl FakeBackend {
    fn new(fail_resize: bool, child_reads_input: bool) -> (Self, FakeChild) {
        let (output_tx, output_rx) = channel();
        let (exit_tx, exit_rx) = channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let resizes = Arc::new(Mutex::new(Vec::new()));
        let killed = Arc::new(Mutex::new(false));
        let parts = FakeParts {
            output: output_rx,
            written: written.clone(),
            exit: exit_rx,
            exit_tx: exit_tx.clone(),
            resizes: resizes.clone(),
            killed: killed.clone(),
        };
        let child = FakeChild {
            output: output_tx,
            written,
            exit: exit_tx,
            resizes,
            killed,
        };
        let backend = Self {
            slot: Mutex::new(Some(parts)),
            fail_resize,
            child_reads_input,
        };
        (backend, child)
    }
}

impl PtyBackend for FakeBackend {
    fn spawn(&self, _spec: &ShellSpec, _size: PtySize) -> Result<PtyProcess, PlatformError> {
        let parts = self
            .slot
            .lock()
            .expect("lock")
            .take()
            .expect("single spawn");
        Ok(PtyProcess {
            reader: Box::new(FakeReader {
                rx: parts.output,
                pending: Vec::new(),
            }),
            writer: Box::new(FakeWriter {
                sink: parts.written,
                reads: self.child_reads_input,
            }),
            control: Box::new(FakeControl {
                resizes: parts.resizes,
                fail: self.fail_resize,
            }),
            child: Box::new(FakeWaiter { exit: parts.exit }),
            killer: Box::new(FakeKiller {
                exit: parts.exit_tx,
                killed: parts.killed,
            }),
        })
    }
}

struct FakeReader {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for FakeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv() {
                Ok(bytes) => self.pending = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

struct FakeWriter {
    sink: Arc<Mutex<Vec<u8>>>,
    reads: bool,
}

impl Write for FakeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.reads {
            // A child that never reads: the write blocks "forever".
            let gate = (Mutex::new(()), Condvar::new());
            let guard = gate.0.lock().expect("lock");
            let _unused = gate.1.wait_timeout(guard, Duration::from_secs(3600));
        }
        self.sink.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct FakeControl {
    resizes: Arc<Mutex<Vec<PtySize>>>,
    fail: bool,
}

impl PtyControl for FakeControl {
    fn resize(&self, size: PtySize) -> Result<(), PlatformError> {
        if self.fail {
            return Err(PlatformError::Resize("injected failure".into()));
        }
        self.resizes.lock().expect("lock").push(size);
        Ok(())
    }
}

struct FakeWaiter {
    exit: Receiver<ExitInfo>,
}

impl ChildWaiter for FakeWaiter {
    fn wait(&mut self) -> Result<ExitInfo, PlatformError> {
        self.exit
            .recv()
            .map_err(|_| PlatformError::Wait(io::ErrorKind::BrokenPipe.into()))
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
}

struct FakeKiller {
    exit: Sender<ExitInfo>,
    killed: Arc<Mutex<bool>>,
}

impl ProcessKiller for FakeKiller {
    fn terminate(&mut self) -> io::Result<()> {
        *self.killed.lock().expect("lock") = true;
        let _ = self.exit.send(ExitInfo {
            code: 1,
            signal: Some("SIGHUP".into()),
        });
        Ok(())
    }
}

#[derive(Default)]
struct CountingNotify(Mutex<u32>);

impl Notify for CountingNotify {
    fn wake(&self, _session: SessionId) {
        *self.0.lock().expect("lock") += 1;
    }
}

fn start(backend: &FakeBackend) -> SessionHandle {
    let config = SessionConfig {
        id: SessionId::new(1, 1),
        shell: platform::default_shell(false),
        size: TermSize::new(20, 4),
        limits: Limits::default(),
    };
    spawn_session(backend, config, Arc::new(CountingNotify::default())).expect("spawn")
}

fn wait_for(handle: &SessionHandle, what: &str, check: impl Fn(&Published) -> bool) -> Published {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let published = handle.published();
        if check(&published) {
            return published;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {:?}",
            published.status
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn screen(published: &Published) -> String {
    published
        .snapshot
        .as_ref()
        .map(|s| s.visible_text())
        .unwrap_or_default()
}

#[test]
fn split_utf8_and_escapes_across_reads_render_correctly() {
    let (backend, child) = FakeBackend::new(false, true);
    let handle = start(&backend);
    for piece in [&b"he\xe4\xb8"[..], b"\xad\x1b[3", b"1mX\x1b[0m"] {
        child.output.send(piece.to_vec()).expect("send");
    }
    let published = wait_for(&handle, "text", |p| screen(p).starts_with("he中X"));
    assert_eq!(published.status, SessionStatus::Running);
}

#[test]
fn input_and_replies_reach_the_writer_in_order() {
    let (backend, child) = FakeBackend::new(false, true);
    let handle = start(&backend);
    handle.write(b"ls\r", InputKind::Typed).expect("write");
    child.output.send(b"\x1b[6n".to_vec()).expect("send");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let written = child.written.lock().expect("lock").clone();
        if written.len() >= 3 + 6 {
            assert!(written.starts_with(b"ls\r") || written.ends_with(b"ls\r"));
            assert!(written.windows(6).any(|w| w == b"\x1b[1;1R"));
            break;
        }
        assert!(
            Instant::now() < deadline,
            "writer did not receive data: {written:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn child_exit_is_published_once_and_rejects_new_input() {
    let (backend, child) = FakeBackend::new(false, true);
    let handle = start(&backend);
    child.output.send(b"bye".to_vec()).expect("send");
    child
        .exit
        .send(ExitInfo {
            code: 0,
            signal: None,
        })
        .expect("exit");
    drop(child.output);
    let published = wait_for(
        &handle,
        "exit",
        |p| matches!(p.status, SessionStatus::Exited(ref e) if e.success()),
    );
    assert!(screen(&published).starts_with("bye"));
    assert_eq!(
        handle.write(b"x", InputKind::Typed),
        Err(QueueError::Closed)
    );
}

#[test]
fn close_terminates_child_without_blocking_caller() {
    let (backend, child) = FakeBackend::new(false, true);
    let handle = start(&backend);
    let started = Instant::now();
    handle.close();
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "close must not block"
    );
    wait_for(&handle, "exited", |p| {
        matches!(p.status, SessionStatus::Exited(_))
    });
    assert!(*child.killed.lock().expect("lock"));
}

#[test]
fn resize_resizes_pty_then_model_with_new_epoch() {
    let (backend, child) = FakeBackend::new(false, true);
    let handle = start(&backend);
    let before = wait_for(&handle, "first snapshot", |p| p.snapshot.is_some());
    let old_epoch = before.snapshot.as_ref().map(|s| s.epoch).expect("snapshot");
    for cols in [30, 40, 50] {
        handle
            .send(SessionCommand::Resize(TermSize::new(cols, 6)))
            .expect("send");
    }
    let after = wait_for(&handle, "resized", |p| {
        p.snapshot.as_ref().is_some_and(|s| s.columns == 50)
    });
    assert!(
        after
            .snapshot
            .as_ref()
            .is_some_and(|s| s.epoch > old_epoch && s.rows == 6)
    );
    let resizes = child.resizes.lock().expect("lock").clone();
    assert_eq!(resizes.last().map(|s| (s.columns, s.rows)), Some((50, 6)));
}

#[test]
fn failed_pty_resize_keeps_old_epoch_and_reports_error() {
    let (backend, _child) = FakeBackend::new(true, true);
    let handle = start(&backend);
    let before = wait_for(&handle, "snapshot", |p| p.snapshot.is_some());
    handle
        .send(SessionCommand::Resize(TermSize::new(60, 10)))
        .expect("send");
    let after = wait_for(&handle, "error", |p| p.error.is_some());
    let epoch = |p: &Published| p.snapshot.as_ref().map(|s| (s.epoch, s.columns));
    assert_eq!(epoch(&after), epoch(&before));
}

#[test]
fn child_that_never_reads_fails_visibly_instead_of_deadlocking() {
    let (backend, child) = FakeBackend::new(false, false);
    let handle = start(&backend);
    // Each DSR query produces a reply; the writer is stuck, so the reserve fills.
    let queries = b"\x1b[6n".repeat(8 * 1024);
    for chunk in queries.chunks(4096) {
        child.output.send(chunk.to_vec()).expect("send");
    }
    let published = wait_for(
        &handle,
        "failure",
        |p| matches!(&p.status, SessionStatus::Failed(m) if m.contains("backpressure")),
    );
    assert!(published.snapshot.is_some());
}

#[test]
fn copy_selection_and_search_publish_results() {
    let (backend, child) = FakeBackend::new(false, true);
    let handle = start(&backend);
    child.output.send(b"alpha beta".to_vec()).expect("send");
    wait_for(&handle, "text", |p| screen(p).starts_with("alpha"));
    let at = |col| terminal_core::GridPoint {
        row: 0,
        col,
        right_half: col == 4,
    };
    handle
        .send(SessionCommand::SelectStart(
            at(0),
            terminal_core::SelectionKind::Simple,
        ))
        .expect("s");
    handle.send(SessionCommand::SelectUpdate(at(4))).expect("u");
    handle.send(SessionCommand::CopySelection).expect("c");
    handle
        .send(SessionCommand::Search("BETA".into()))
        .expect("q");
    let published = wait_for(&handle, "results", |p| {
        p.copied.is_some() && p.search.is_some()
    });
    assert_eq!(published.copied.as_deref(), Some("alpha"));
    assert_eq!(published.search.as_ref().map(|r| r.matches.len()), Some(1));
}

#[test]
fn heavy_output_is_parsed_without_byte_loss() {
    let (backend, child) = FakeBackend::new(false, true);
    let handle = start(&backend);
    let producer = std::thread::spawn(move || {
        for i in 0..2000 {
            let line = format!("\x1b[32mline {i:05}\x1b[0m\r\n");
            child.output.send(line.into_bytes()).expect("send");
        }
        child.output.send(b"END-MARKER".to_vec()).expect("send");
        child
    });
    let published = wait_for(&handle, "marker", |p| screen(p).contains("END-MARKER"));
    assert!(screen(&published).contains("line 01999"));
    producer.join().expect("producer");
}
