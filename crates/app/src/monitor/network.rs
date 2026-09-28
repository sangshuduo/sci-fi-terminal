//! Per-interface network throughput sampling.

use std::time::{Duration, Instant};

use sysinfo::Networks;

/// Maximum number of interfaces returned by [`NetworkSampler::sample`].
const MAX_INTERFACES: usize = 8;

/// Throughput for one network interface.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceRate {
    /// Interface name (e.g. `en0`, `eth0`).
    pub name: String,
    /// Receive rate since the previous sample, in bytes per second.
    pub rx_bytes_per_sec: f64,
    /// Transmit rate since the previous sample, in bytes per second.
    pub tx_bytes_per_sec: f64,
    /// Total bytes received since boot (as reported by the OS).
    pub rx_total: u64,
    /// Total bytes transmitted since boot (as reported by the OS).
    pub tx_total: u64,
}

/// Samples interface counters and converts deltas to rates.
pub struct NetworkSampler {
    networks: Networks,
    last: Option<Instant>,
}

impl Default for NetworkSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkSampler {
    /// Creates a sampler. No OS data is read until [`NetworkSampler::sample`].
    pub fn new() -> Self {
        Self {
            networks: Networks::new(),
            last: None,
        }
    }

    /// Rates since the previous call (first call returns rates of 0.0).
    ///
    /// Skips loopback interfaces (`lo`, `lo0`, names starting with
    /// `Loopback`) and interfaces with zero totals. Sorted by rx+tx rate
    /// descending, capped at 8 entries.
    pub fn sample(&mut self) -> Vec<InterfaceRate> {
        self.networks.refresh(true);
        let now = Instant::now();
        let elapsed = self.last.map(|t| now.duration_since(t));
        self.last = Some(now);

        let mut rates: Vec<InterfaceRate> = self
            .networks
            .list()
            .iter()
            .filter(|(name, data)| {
                !is_loopback(name) && (data.total_received() > 0 || data.total_transmitted() > 0)
            })
            .map(|(name, data)| {
                let (rx, tx) = match elapsed {
                    Some(e) => (rate(data.received(), e), rate(data.transmitted(), e)),
                    None => (0.0, 0.0),
                };
                InterfaceRate {
                    name: name.clone(),
                    rx_bytes_per_sec: rx,
                    tx_bytes_per_sec: tx,
                    rx_total: data.total_received(),
                    tx_total: data.total_transmitted(),
                }
            })
            .collect();
        sort_and_cap(&mut rates);
        rates
    }
}

/// Converts a byte delta over `elapsed` into bytes per second.
/// Returns 0.0 when `elapsed` is zero.
pub fn rate(delta_bytes: u64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        0.0
    } else {
        delta_bytes as f64 / secs
    }
}

/// True for loopback interface names on Linux, macOS and Windows.
fn is_loopback(name: &str) -> bool {
    name == "lo" || name == "lo0" || name.starts_with("Loopback")
}

/// Sorts by combined rate descending (name ascending as a stable tiebreak)
/// and truncates to [`MAX_INTERFACES`].
fn sort_and_cap(rates: &mut Vec<InterfaceRate>) {
    rates.sort_by(|a, b| {
        let ta = a.rx_bytes_per_sec + a.tx_bytes_per_sec;
        let tb = b.rx_bytes_per_sec + b.tx_bytes_per_sec;
        tb.total_cmp(&ta).then_with(|| a.name.cmp(&b.name))
    });
    rates.truncate(MAX_INTERFACES);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_zero_elapsed_is_zero() {
        assert_eq!(rate(1000, Duration::ZERO), 0.0);
    }

    #[test]
    fn rate_computes_bytes_per_second() {
        assert_eq!(rate(1000, Duration::from_secs(2)), 500.0);
        assert_eq!(rate(0, Duration::from_secs(1)), 0.0);
        assert!((rate(100, Duration::from_millis(500)) - 200.0).abs() < 1e-9);
    }

    #[test]
    fn loopback_names_detected() {
        assert!(is_loopback("lo"));
        assert!(is_loopback("lo0"));
        assert!(is_loopback("Loopback Pseudo-Interface 1"));
        assert!(!is_loopback("en0"));
        assert!(!is_loopback("eth0"));
    }

    #[test]
    fn sort_and_cap_orders_and_limits() {
        let mut v: Vec<InterfaceRate> = (0..12)
            .map(|i| InterfaceRate {
                name: format!("if{i:02}"),
                rx_bytes_per_sec: i as f64,
                tx_bytes_per_sec: 1.0,
                rx_total: 1,
                tx_total: 1,
            })
            .collect();
        sort_and_cap(&mut v);
        assert_eq!(v.len(), MAX_INTERFACES);
        assert_eq!(v[0].name, "if11");
    }
}
