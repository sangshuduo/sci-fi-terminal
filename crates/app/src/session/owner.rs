//! The model owner: the only code that mutates a session's terminal model.
//!
//! It works in bounded batches (64 KiB or 2 ms), services commands and
//! resizes between batches, publishes only the newest snapshot, and runs the
//! explicit shutdown state machine without involving the GUI thread.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use platform::{ExitInfo, ProcessKiller, PtyControl, PtySize};
use terminal_core::{ModelEvent, TermSize, TerminalModel};

use super::spawn::{ReaderMsg, drain};
use super::write_queue::{QueueError, WriteQueue};
use super::{SessionCommand, SessionStatus, Shared};

const BATCH_BYTES: usize = 64 * 1024;
const BATCH_TIME: Duration = Duration::from_millis(2);
/// Output drain window after input is closed.
const DRAIN_GRACE: Duration = Duration::from_millis(500);
/// Time allowed for the child to exit after termination is requested.
const REAP_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct OwnerChannels {
    pub doorbell: Receiver<()>,
    pub output: Receiver<ReaderMsg>,
    pub exit: Receiver<Result<ExitInfo, String>>,
    pub commands: Receiver<crate::session::SessionCommand>,
    pub write_errors: Receiver<String>,
}

enum Phase {
    Running,
    /// Input closed; draining output until `drain_until`, then terminate.
    Closing {
        drain_until: Instant,
        terminated_at: Option<Instant>,
    },
    Done,
}

pub(crate) struct Owner {
    model: TerminalModel,
    control: Option<Box<dyn PtyControl>>,
    killer: Box<dyn ProcessKiller>,
    writes: Arc<WriteQueue>,
    shared: Arc<Shared>,
    channels: OwnerChannels,
    phase: Phase,
    exit: Option<ExitInfo>,
    reader_done: bool,
    dirty: bool,
}

impl Owner {
    pub(crate) fn new(
        model: TerminalModel,
        control: Box<dyn PtyControl>,
        killer: Box<dyn ProcessKiller>,
        writes: Arc<WriteQueue>,
        shared: Arc<Shared>,
        channels: OwnerChannels,
    ) -> Self {
        Self {
            model,
            control: Some(control),
            killer,
            writes,
            shared,
            channels,
            phase: Phase::Running,
            exit: None,
            reader_done: false,
            dirty: true,
        }
    }

    pub(crate) fn run(mut self) {
        self.publish();
        loop {
            let more_output = self.step();
            self.publish();
            if matches!(self.phase, Phase::Done) {
                break;
            }
            if !more_output && !self.wait_for_work() {
                break;
            }
        }
        self.finish();
    }

    /// One scheduling round. Returns whether output is still queued.
    fn step(&mut self) -> bool {
        if self.shared.close_requested() {
            self.begin_close();
        }
        self.handle_commands();
        let more = self.process_output_batch();
        self.handle_write_errors();
        self.handle_exit();
        self.advance_shutdown();
        more
    }

    /// Block until any producer rings, or until a shutdown deadline.
    fn wait_for_work(&mut self) -> bool {
        let timeout = match self.phase {
            Phase::Closing {
                drain_until,
                terminated_at,
            } => {
                // Wake for whichever shutdown deadline is next.
                let now = Instant::now();
                let reap = terminated_at.map(|t| t + REAP_GRACE);
                let deadline = match reap {
                    Some(reap) if drain_until <= now => reap,
                    Some(reap) => reap.min(drain_until),
                    None => drain_until,
                };
                Some(deadline.saturating_duration_since(now))
            }
            _ => None,
        };
        let result = match timeout {
            Some(limit) => match self.channels.doorbell.recv_timeout(limit) {
                Err(RecvTimeoutError::Disconnected) => Err(()),
                _ => Ok(()),
            },
            None => self.channels.doorbell.recv().map_err(|_| ()),
        };
        result.is_ok()
    }

    fn handle_commands(&mut self) {
        let mut resize = None;
        for command in drain(&self.channels.commands) {
            match command {
                // Coalesce not-yet-started resizes to the newest size.
                SessionCommand::Resize(size) => resize = Some(size),
                other => self.apply(other),
            }
        }
        if let Some(size) = resize {
            self.resize(size);
        }
    }

    fn apply(&mut self, command: SessionCommand) {
        match command {
            SessionCommand::Resize(size) => self.resize(size),
            SessionCommand::Scroll(lines) => self.model.scroll(lines),
            SessionCommand::ScrollToBottom => self.model.scroll_to_bottom(),
            SessionCommand::SelectStart(at, kind) => self.model.start_selection(at, kind),
            SessionCommand::SelectUpdate(at) => self.model.update_selection(at),
            SessionCommand::SelectClear => self.model.clear_selection(),
            SessionCommand::CopySelection => {
                let text = self.model.selection_text();
                self.shared.lock().copied = text;
                self.shared.wake(self.model.session());
                return;
            }
            SessionCommand::Search(query) => {
                let version = self.shared.lock().snapshot.as_ref().map(|s| s.version);
                let result = self
                    .model
                    .search_literal(&query, version.unwrap_or_default());
                self.shared.lock().search = Some(Arc::new(result));
                self.shared.wake(self.model.session());
            }
            SessionCommand::RevealLine(line) => self.model.reveal_line(line),
            SessionCommand::SetQueryPalette(palette) => {
                self.model.set_query_palette(*palette);
                return;
            }
        }
        self.dirty = true;
    }

    /// Resize transaction: PTY first, then model; a failed PTY resize keeps the old epoch.
    fn resize(&mut self, size: TermSize) {
        if size == self.model.size() {
            return;
        }
        let Some(control) = self.control.as_ref() else {
            return;
        };
        let pty = PtySize {
            columns: size.columns,
            rows: size.rows,
            pixel_width: size.columns.saturating_mul(size.cell_width),
            pixel_height: size.rows.saturating_mul(size.cell_height),
        };
        match control.resize(pty) {
            Ok(()) => {
                self.model.resize(size);
                self.dirty = true;
            }
            Err(err) => {
                self.shared.lock().error = Some(format!("{err}; retry by resizing again"));
                self.shared.wake(self.model.session());
            }
        }
    }

    fn process_output_batch(&mut self) -> bool {
        let started = Instant::now();
        let mut consumed = 0;
        while consumed < BATCH_BYTES && started.elapsed() < BATCH_TIME {
            let chunk = match self.channels.output.try_recv() {
                Ok(ReaderMsg::Data(bytes)) => bytes,
                Ok(ReaderMsg::Eof) | Err(TryRecvError::Disconnected) => {
                    self.reader_done = true;
                    return false;
                }
                Ok(ReaderMsg::Error(kind)) => {
                    self.reader_done = true;
                    self.fail(format!("reading from the terminal failed: {kind}"));
                    return false;
                }
                Err(TryRecvError::Empty) => return false,
            };
            consumed += chunk.len();
            self.parse(&chunk);
        }
        true
    }

    fn parse(&mut self, bytes: &[u8]) {
        let (events, replies) = self.model.advance(bytes);
        self.dirty = true;
        for event in events {
            self.apply_event(event);
        }
        for reply in replies {
            match self.writes.push_reply(reply) {
                Ok(()) | Err(QueueError::Closed) => {}
                Err(QueueError::ReplyBackpressure | QueueError::InputBusy) => {
                    self.fail("protocol backpressure: the program is not reading replies".into());
                    break;
                }
            }
        }
    }

    fn apply_event(&mut self, event: ModelEvent) {
        let mut published = self.shared.lock();
        match event {
            ModelEvent::Title(title) => published.title = title,
            ModelEvent::Bell => published.bells = published.bells.wrapping_add(1),
            ModelEvent::Denied(_) => {}
        }
    }

    fn handle_write_errors(&mut self) {
        if let Some(kind) = drain(&self.channels.write_errors).into_iter().next()
            && matches!(self.phase, Phase::Running)
        {
            self.fail(format!("writing to the terminal failed: {kind}"));
        }
    }

    fn handle_exit(&mut self) {
        if self.exit.is_some() {
            return;
        }
        match self.channels.exit.try_recv() {
            Ok(Ok(info)) => {
                self.exit = Some(info);
                self.begin_close();
            }
            Ok(Err(message)) => {
                self.exit = Some(ExitInfo {
                    code: u32::MAX,
                    signal: None,
                });
                self.fail(message);
            }
            Err(_) => {}
        }
    }

    fn fail(&mut self, message: String) {
        self.shared.lock().status = SessionStatus::Failed(message);
        self.begin_close();
    }

    fn begin_close(&mut self) {
        if !matches!(self.phase, Phase::Running) {
            return;
        }
        self.writes.close();
        {
            let mut published = self.shared.lock();
            if !matches!(published.status, SessionStatus::Failed(_)) {
                published.status = SessionStatus::Closing;
            }
        }
        self.phase = Phase::Closing {
            drain_until: Instant::now() + DRAIN_GRACE,
            terminated_at: None,
        };
        self.dirty = true;
    }

    /// Closing: drain output for up to 500 ms, then request termination and
    /// wait up to 2 s to reap. Never blocks on a worker.
    fn advance_shutdown(&mut self) {
        let Phase::Closing {
            drain_until,
            terminated_at,
        } = self.phase
        else {
            return;
        };
        let now = Instant::now();
        let drained = self.reader_done || now >= drain_until;
        if self.exit.is_some() && drained {
            self.phase = Phase::Done;
            return;
        }
        match terminated_at {
            None if self.exit.is_none() && (drained || self.shared.close_requested()) => {
                let _ = self.killer.terminate();
                self.phase = Phase::Closing {
                    drain_until,
                    terminated_at: Some(now),
                };
            }
            Some(at) if now >= at + REAP_GRACE => self.phase = Phase::Done,
            _ => {}
        }
    }

    fn finish(&mut self) {
        // Closing the master releases the reader on platforms that need it.
        self.control = None;
        let mut published = self.shared.lock();
        if !matches!(published.status, SessionStatus::Failed(_)) {
            published.status = match self.exit.clone() {
                Some(info) => SessionStatus::Exited(info),
                None => SessionStatus::Failed("the shell did not exit after termination".into()),
            };
        }
        drop(published);
        self.shared.wake(self.model.session());
    }

    fn publish(&mut self) {
        if !std::mem::take(&mut self.dirty) {
            return;
        }
        let snapshot = Arc::new(self.model.snapshot());
        self.shared.lock().snapshot = Some(snapshot);
        self.shared.wake(self.model.session());
    }
}
