//! CPU, memory and swap samples.

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

/// CPU and memory only; no process table.
pub(crate) struct SystemSampler {
    system: System,
    refresh: RefreshKind,
}

impl SystemSampler {
    pub(crate) fn new() -> Self {
        let refresh = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(MemoryRefreshKind::everything());
        Self {
            system: System::new_with_specifics(refresh),
            refresh,
        }
    }

    pub(crate) fn sample(&mut self) -> MetricsSample {
        self.system.refresh_specifics(self.refresh);
        let system = &self.system;
        MetricsSample {
            cpu_percent: system.global_cpu_usage(),
            per_core: system.cpus().iter().map(sysinfo::Cpu::cpu_usage).collect(),
            memory_used: system.used_memory(),
            memory_total: system.total_memory(),
            swap_used: system.used_swap(),
            swap_total: system.total_swap(),
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn system_sample_reports_memory() {
        assert!(SystemSampler::new().sample().memory_total > 0);
    }
}
