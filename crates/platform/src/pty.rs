//! PTY transport abstraction and the native `portable-pty` implementation.
//!
//! The session layer only sees these traits, so tests can substitute an
//! in-memory transport. Only one PTY implementation is ever connected to a
//! model owner; the engine's own PTY event loop is never used.

use std::io::{Read, Write};

use portable_pty::{CommandBuilder, MasterPty, PtySize as NativeSize, native_pty_system};

use crate::error::PlatformError;
use crate::shell::ShellSpec;

/// Terminal size in cells and pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub columns: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtySize {
    fn native(self) -> NativeSize {
        NativeSize {
            rows: self.rows.max(1),
            cols: self.columns.max(1),
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
        }
    }
}

/// How a child ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: u32,
    pub signal: Option<String>,
}

impl ExitInfo {
    pub fn success(&self) -> bool {
        self.code == 0 && self.signal.is_none()
    }
}

/// Resize control for the PTY master.
pub trait PtyControl: Send {
    fn resize(&self, size: PtySize) -> Result<(), PlatformError>;
}

/// Blocking wait for the child; run on a dedicated lifecycle thread.
pub trait ChildWaiter: Send {
    fn wait(&mut self) -> Result<ExitInfo, PlatformError>;
    fn process_id(&self) -> Option<u32>;
}

/// Terminates the owned child (hangup on Unix, terminate on Windows).
pub trait ProcessKiller: Send {
    fn terminate(&mut self) -> std::io::Result<()>;
}

/// All handles for one running child. The unused slave side has already been released.
pub struct PtyProcess {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub control: Box<dyn PtyControl>,
    pub child: Box<dyn ChildWaiter>,
    pub killer: Box<dyn ProcessKiller>,
}

/// Spawns children attached to a PTY.
pub trait PtyBackend: Send + Sync {
    fn spawn(&self, spec: &ShellSpec, size: PtySize) -> Result<PtyProcess, PlatformError>;
}

/// The OS pseudo-terminal: Unix PTY or Windows ConPTY via `portable-pty`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativePty;

impl PtyBackend for NativePty {
    fn spawn(&self, spec: &ShellSpec, size: PtySize) -> Result<PtyProcess, PlatformError> {
        let pair = native_pty_system()
            .openpty(size.native())
            .map_err(|err| PlatformError::OpenPty(err.to_string()))?;
        let command = build_command(spec);
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| PlatformError::Spawn {
                program: spec.program.display().to_string(),
                reason: err.to_string(),
            })?;
        // Release the slave so EOF is observed once the child exits.
        drop(pair.slave);
        let killer = child.clone_killer();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| PlatformError::OpenPty(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| PlatformError::OpenPty(err.to_string()))?;
        Ok(PtyProcess {
            reader,
            writer,
            control: Box::new(NativeControl {
                master: pair.master,
            }),
            child: Box::new(NativeChild { child }),
            killer: Box::new(NativeKiller { killer }),
        })
    }
}

fn build_command(spec: &ShellSpec) -> CommandBuilder {
    let mut command = CommandBuilder::new(&spec.program);
    command.args(&spec.args);
    // A missing profile cwd is rejected earlier; otherwise start in the home directory.
    if let Some(cwd) = spec.cwd.clone().or_else(crate::paths::home_dir) {
        command.cwd(cwd);
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "sci-fi-terminal");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    for (key, value) in &spec.env {
        command.env(key, value);
    }
    command
}

struct NativeControl {
    master: Box<dyn MasterPty + Send>,
}

impl PtyControl for NativeControl {
    fn resize(&self, size: PtySize) -> Result<(), PlatformError> {
        self.master
            .resize(size.native())
            .map_err(|err| PlatformError::Resize(err.to_string()))
    }
}

struct NativeChild {
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl ChildWaiter for NativeChild {
    fn wait(&mut self) -> Result<ExitInfo, PlatformError> {
        let status = self.child.wait().map_err(PlatformError::Wait)?;
        Ok(ExitInfo {
            code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
        })
    }

    fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }
}

struct NativeKiller {
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
}

impl ProcessKiller for NativeKiller {
    fn terminate(&mut self) -> std::io::Result<()> {
        self.killer.kill()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::shell::resolve_profile;

    fn size() -> PtySize {
        PtySize {
            columns: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// Read concurrently: on macOS a child's final tty close waits for output to drain.
    fn spawn_reader(mut reader: Box<dyn Read + Send>) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            // EIO after the child exits ends the stream on Linux; EOF on macOS.
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    }

    #[test]
    fn spawns_child_reads_output_and_reaps() {
        let spec = resolve_profile(
            "sh",
            &["-c".into(), "printf ready; exit 3".into()],
            false,
            None,
            &[],
        )
        .expect("spec");
        let mut process = NativePty.spawn(&spec, size()).expect("spawn");
        let output = spawn_reader(process.reader);
        let exit = process.child.wait().expect("wait");
        drop(process.writer);
        drop(process.control);
        assert_eq!(exit.code, 3);
        assert!(output.join().expect("reader").contains("ready"));
    }

    #[test]
    fn child_sees_terminal_environment_and_size() {
        let script = "printf '%s %s ' \"$TERM\" \"$COLORTERM\"; stty size";
        let spec =
            resolve_profile("sh", &["-c".into(), script.into()], false, None, &[]).expect("spec");
        let mut process = NativePty.spawn(&spec, size()).expect("spawn");
        let output = spawn_reader(process.reader);
        process.child.wait().expect("wait");
        drop(process.control);
        let text = output.join().expect("reader");
        assert!(text.contains("xterm-256color truecolor 24 80"), "{text:?}");
    }

    #[test]
    fn terminate_ends_interactive_child() {
        let spec = resolve_profile("sh", &["-c".into(), "sleep 30".into()], false, None, &[])
            .expect("spec");
        let mut process = NativePty.spawn(&spec, size()).expect("spawn");
        process.killer.terminate().expect("terminate");
        let exit = process.child.wait().expect("wait");
        assert!(!exit.success());
    }

    #[test]
    fn repeated_spawn_close_cycles_settle() {
        for _ in 0..20 {
            let spec = resolve_profile("sh", &["-c".into(), "exit 0".into()], false, None, &[])
                .expect("spec");
            let mut process = NativePty.spawn(&spec, size()).expect("spawn");
            assert!(process.child.wait().expect("wait").success());
        }
    }
}
