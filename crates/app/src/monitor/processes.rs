//! Top-N process sampling (CPU and memory only).
//!
//! Privacy: only pid, name, CPU and memory are ever refreshed. Command lines,
//! environment variables, users and executable paths are never collected.
//! The working directory is fetched only on explicit request for one pid.

use std::cmp::Ordering;
use std::path::PathBuf;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Hard upper bound on the number of processes returned by [`ProcessSampler::sample`].
pub const MAX_PROCESSES: usize = 10;

/// Maximum number of characters kept from a process name.
const MAX_NAME_CHARS: usize = 64;

/// A single process snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessInfo {
    /// OS process id.
    pub pid: u32,
    /// Process name with control characters stripped, at most 64 characters.
    pub name: String,
    /// CPU usage in percent (may exceed 100 on multi-core systems).
    pub cpu_percent: f32,
    /// Resident memory in bytes.
    pub memory_bytes: u64,
}

/// Samples processes using a long-lived `sysinfo::System` so CPU deltas work.
pub struct ProcessSampler {
    system: System,
}

impl Default for ProcessSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSampler {
    /// Creates a sampler. No OS data is read until [`ProcessSampler::sample`].
    pub fn new() -> Self {
        Self {
            system: System::new(),
        }
    }

    /// Refresh process CPU+memory only (no command lines, no environment, no
    /// cwd) and return the top `limit.min(MAX_PROCESSES)` by CPU desc, ties by
    /// memory desc.
    ///
    /// CPU usage is computed relative to the previous call, so the first call
    /// typically reports 0% for every process.
    pub fn sample(&mut self, limit: usize) -> Vec<ProcessInfo> {
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        let all: Vec<ProcessInfo> = self
            .system
            .processes()
            .values()
            .map(|p| ProcessInfo {
                pid: p.pid().as_u32(),
                name: sanitize_name(&p.name().to_string_lossy()),
                cpu_percent: p.cpu_usage(),
                memory_bytes: p.memory(),
            })
            .collect();
        top_n(all, limit)
    }

    /// Current working directory of a pid, if the OS exposes it (used by the
    /// CWD directory viewer). Refreshes only that pid with cwd enabled.
    pub fn cwd_of(&mut self, pid: u32) -> Option<PathBuf> {
        let pid = Pid::from_u32(pid);
        let kind = ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always);
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
        self.system
            .process(pid)
            .and_then(|p| p.cwd())
            .map(|p| p.to_path_buf())
    }
}

/// Sorts by CPU descending (ties broken by memory descending) and keeps the
/// first `limit.min(MAX_PROCESSES)` entries.
pub(crate) fn top_n(mut v: Vec<ProcessInfo>, limit: usize) -> Vec<ProcessInfo> {
    v.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.memory_bytes.cmp(&a.memory_bytes))
    });
    v.truncate(limit.min(MAX_PROCESSES));
    v
}

/// Strips control characters and truncates to [`MAX_NAME_CHARS`] characters.
fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, cpu: f32, mem: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: format!("p{pid}"),
            cpu_percent: cpu,
            memory_bytes: mem,
        }
    }

    #[test]
    fn top_n_sorts_by_cpu_then_memory() {
        let v = vec![p(1, 1.0, 10), p(2, 5.0, 1), p(3, 1.0, 50), p(4, 0.0, 999)];
        let pids: Vec<u32> = top_n(v, 10).iter().map(|x| x.pid).collect();
        assert_eq!(pids, vec![2, 3, 1, 4]);
    }

    #[test]
    fn top_n_respects_limit_and_cap() {
        let v: Vec<_> = (0..30).map(|i| p(i, i as f32, 0)).collect();
        assert_eq!(top_n(v.clone(), 3).len(), 3);
        assert_eq!(top_n(v.clone(), 100).len(), MAX_PROCESSES);
        assert!(top_n(v, 0).is_empty());
    }

    #[test]
    fn top_n_handles_nan_and_empty() {
        assert!(top_n(Vec::new(), 5).is_empty());
        let v = vec![p(1, f32::NAN, 1), p(2, 2.0, 1)];
        assert_eq!(top_n(v, 5).len(), 2);
    }

    #[test]
    fn sanitize_strips_control_and_truncates() {
        assert_eq!(sanitize_name("a\x1b[31mb\n"), "a[31mb");
        let long = "x".repeat(200);
        assert_eq!(sanitize_name(&long).chars().count(), MAX_NAME_CHARS);
    }
}
