//! Session startup: spawn the child, then start reader, writer, waiter and owner.

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;

use platform::{ChildWaiter, ExitInfo, PtyBackend, PtySize, ShellSpec};
use terminal_core::{Limits, SessionId, TermSize, TerminalModel};

use super::owner::{Owner, OwnerChannels};
use super::write_queue::WriteQueue;
use super::{Notify, SessionHandle, SessionStatus, Shared};

/// Reader chunk size; the bounded channel holds at most four chunks (256 KiB).
pub(crate) const READ_CHUNK_BYTES: usize = 64 * 1024;
const READ_QUEUE_CHUNKS: usize = 4;
const COMMAND_QUEUE: usize = 64;

/// Messages from the blocking reader to the owner.
pub(crate) enum ReaderMsg {
    Data(Vec<u8>),
    Eof,
    Error(String),
}

/// Per-session launch parameters.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub id: SessionId,
    pub shell: ShellSpec,
    pub size: TermSize,
    pub limits: Limits,
}

/// Spawn a session. Startup failures are returned before any thread starts,
/// with every acquired handle already released by the backend.
pub fn spawn_session(
    backend: &dyn PtyBackend,
    config: SessionConfig,
    notify: Arc<dyn Notify>,
) -> Result<SessionHandle, platform::PlatformError> {
    let pty_size = PtySize {
        columns: config.size.columns,
        rows: config.size.rows,
        pixel_width: config.size.columns * config.size.cell_width,
        pixel_height: config.size.rows * config.size.cell_height,
    };
    let process = backend.spawn(&config.shell, pty_size)?;
    let shared = Arc::new(Shared::new(notify));
    let writes = Arc::new(WriteQueue::default());
    let (doorbell_tx, doorbell_rx) = sync_channel::<()>(1);
    let (output_tx, output_rx) = sync_channel::<ReaderMsg>(READ_QUEUE_CHUNKS);
    let (exit_tx, exit_rx) = sync_channel::<Result<ExitInfo, String>>(1);
    let (command_tx, command_rx) = sync_channel(COMMAND_QUEUE);
    let (write_err_tx, write_err_rx) = sync_channel::<String>(1);

    spawn_reader(config.id, process.reader, output_tx, doorbell_tx.clone());
    spawn_writer(
        config.id,
        process.writer,
        writes.clone(),
        write_err_tx,
        doorbell_tx.clone(),
    );
    let pid = process.child.process_id();
    spawn_waiter(config.id, process.child, exit_tx, doorbell_tx.clone());

    let model = TerminalModel::new(config.id, config.size, config.limits);
    let owner = Owner::new(
        model,
        process.control,
        process.killer,
        writes.clone(),
        shared.clone(),
        OwnerChannels {
            doorbell: doorbell_rx,
            output: output_rx,
            exit: exit_rx,
            commands: command_rx,
            write_errors: write_err_rx,
        },
    );
    shared.lock().status = SessionStatus::Running;
    spawn_named(config.id, "owner", move || owner.run());
    Ok(SessionHandle {
        id: config.id,
        commands: command_tx,
        doorbell: doorbell_tx,
        writes,
        shared,
        label: config.shell.label,
        pid,
    })
}

fn spawn_named(id: SessionId, role: &str, body: impl FnOnce() + Send + 'static) {
    let name = format!("s{}-{}-{role}", id.slot, id.generation);
    // Thread creation failure leaves the session without that worker; the
    // owner observes a disconnected channel and fails the session.
    let _ = thread::Builder::new()
        .name(name)
        .stack_size(256 * 1024)
        .spawn(body);
}

fn spawn_reader(
    id: SessionId,
    mut reader: Box<dyn Read + Send>,
    output: SyncSender<ReaderMsg>,
    doorbell: SyncSender<()>,
) {
    spawn_named(id, "read", move || {
        let mut buf = vec![0u8; READ_CHUNK_BYTES];
        loop {
            let msg = match reader.read(&mut buf) {
                Ok(0) => ReaderMsg::Eof,
                Ok(n) => ReaderMsg::Data(buf[..n].to_vec()),
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                // Linux reports EIO once the child side is gone; treat as EOF.
                Err(err) if err.raw_os_error() == Some(5) => ReaderMsg::Eof,
                Err(err) => ReaderMsg::Error(err.kind().to_string()),
            };
            let done = !matches!(msg, ReaderMsg::Data(_));
            // Blocking send is the backpressure: bytes are never dropped.
            if output.send(msg).is_err() {
                return;
            }
            let _ = doorbell.try_send(());
            if done {
                return;
            }
        }
    });
}

fn spawn_writer(
    id: SessionId,
    mut writer: Box<dyn Write + Send>,
    queue: Arc<WriteQueue>,
    errors: SyncSender<String>,
    doorbell: SyncSender<()>,
) {
    spawn_named(id, "write", move || {
        while let Some(bytes) = queue.next() {
            if let Err(err) = writer.write_all(&bytes).and_then(|()| writer.flush()) {
                let _ = errors.try_send(err.kind().to_string());
                let _ = doorbell.try_send(());
                return;
            }
        }
    });
}

fn spawn_waiter(
    id: SessionId,
    mut child: Box<dyn ChildWaiter>,
    exit: SyncSender<Result<ExitInfo, String>>,
    doorbell: SyncSender<()>,
) {
    spawn_named(id, "wait", move || {
        let result = child.wait().map_err(|err| err.to_string());
        let _ = exit.send(result);
        let _ = doorbell.try_send(());
    });
}

/// Receive everything currently queued without blocking.
pub(crate) fn drain<T>(rx: &Receiver<T>) -> Vec<T> {
    rx.try_iter().collect()
}
