//! Local CPU/memory sampling on a low-rate worker thread.
//!
//! Sampling runs only while the panel is enabled and visible: once per
//! interval (≥ 1 s) in the foreground and at most every 5 s in the
//! background. No process scans, no shelling out, no network.

use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

pub const MIN_INTERVAL: Duration = Duration::from_secs(1);
pub const BACKGROUND_INTERVAL: Duration = Duration::from_secs(5);
/// Per-core bars shown at most; larger machines are summarised.
pub const MAX_CORES_SHOWN: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct MetricsSample {
    pub cpu_percent: f32,
    pub per_core: Vec<f32>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
}

impl MetricsSample {
    pub fn memory_fraction(&self) -> f32 {
        fraction(self.memory_used, self.memory_total)
    }

    pub fn swap_fraction(&self) -> f32 {
        fraction(self.swap_used, self.swap_total)
    }
}

fn fraction(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) as f32
    }
}

/// Render a byte count as a short human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

enum Control {
    Interval(Duration),
    Stop,
}

/// Owns the sampling thread; dropping it stops sampling promptly.
pub struct MetricsWorker {
    control: Sender<Control>,
    thread: Option<JoinHandle<()>>,
}

impl MetricsWorker {
    pub fn start(
        interval: Duration,
        sink: Arc<dyn Fn(MetricsSample) + Send + Sync>,
    ) -> Option<Self> {
        let (control, rx) = channel();
        let thread = std::thread::Builder::new()
            .name("metrics".into())
            .stack_size(128 * 1024)
            .spawn(move || run(interval.max(MIN_INTERVAL), &rx, sink.as_ref()))
            .ok()?;
        Some(Self {
            control,
            thread: Some(thread),
        })
    }

    /// Change the sampling interval (e.g. slower while the window is unfocused).
    pub fn set_interval(&self, interval: Duration) {
        let _ = self
            .control
            .send(Control::Interval(interval.max(MIN_INTERVAL)));
    }
}

impl Drop for MetricsWorker {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Stop);
        // The worker wakes immediately on Stop, so this join is bounded.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    mut interval: Duration,
    control: &std::sync::mpsc::Receiver<Control>,
    sink: &dyn Fn(MetricsSample),
) {
    let refresh = RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
        .with_memory(MemoryRefreshKind::everything());
    let mut system = System::new_with_specifics(refresh);
    loop {
        match control.recv_timeout(interval) {
            Ok(Control::Interval(next)) => {
                interval = next;
                continue;
            }
            Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        system.refresh_specifics(refresh);
        sink(sample(&system));
    }
}

fn sample(system: &System) -> MetricsSample {
    MetricsSample {
        cpu_percent: system.global_cpu_usage(),
        per_core: system.cpus().iter().map(sysinfo::Cpu::cpu_usage).collect(),
        memory_used: system.used_memory(),
        memory_total: system.total_memory(),
        swap_used: system.used_swap(),
        swap_total: system.total_swap(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Instant;

    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn fractions_handle_zero_totals() {
        let sample = MetricsSample {
            cpu_percent: 0.0,
            per_core: Vec::new(),
            memory_used: 1,
            memory_total: 0,
            swap_used: 0,
            swap_total: 0,
        };
        assert_eq!(sample.memory_fraction(), 0.0);
    }

    #[test]
    fn worker_samples_and_stops_promptly() {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let samples = samples.clone();
            Arc::new(move |s: MetricsSample| samples.lock().expect("lock").push(s))
        };
        let worker = MetricsWorker::start(MIN_INTERVAL, sink).expect("start");
        let deadline = Instant::now() + Duration::from_secs(5);
        while samples.lock().expect("lock").is_empty() {
            assert!(Instant::now() < deadline, "no sample produced");
            std::thread::sleep(Duration::from_millis(50));
        }
        let stopping = Instant::now();
        drop(worker);
        assert!(stopping.elapsed() < Duration::from_millis(500));
        let sample = samples.lock().expect("lock")[0].clone();
        assert!(sample.memory_total > 0);
    }
}
