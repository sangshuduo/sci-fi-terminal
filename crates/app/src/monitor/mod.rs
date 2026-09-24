//! Local, read-only, bounded system monitoring.
//!
//! Everything in this module reads OS state through library APIs (sysinfo,
//! netstat2, an optional user-supplied offline GeoIP database). Nothing here
//! shells out to `ps`/`netstat`/`lsof` or collects process command lines or
//! environment variables. The single exception to "no network I/O" is the
//! opt-in, off-by-default [`public_ip`] lookup (ADR-007).
//!
//! The functions are designed to be driven from a background worker thread
//! owned by the caller; each call is bounded in time and output size.

pub mod connections;
pub mod geoip;
pub mod network;
pub mod processes;
pub mod public_ip;

pub use connections::{Connection, ConnectionState, MAX_CONNECTIONS, Protocol, list_connections};
pub use geoip::{GeoIp, GeoIpError, GeoLocation};
pub use network::{InterfaceRate, NetworkSampler};
pub use processes::{MAX_PROCESSES, ProcessInfo, ProcessSampler};

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn process_sampler_returns_bounded_real_processes() {
        let mut sampler = ProcessSampler::new();
        let procs = sampler.sample(5);
        assert!(procs.len() <= 5);
        assert!(
            procs.iter().any(|p| p.pid > 0),
            "expected at least one pid > 0"
        );
    }

    #[test]
    fn network_sampler_can_sample_twice() {
        let mut sampler = NetworkSampler::new();
        let first = sampler.sample();
        assert!(first.len() <= 8);
        assert!(
            first
                .iter()
                .all(|r| r.rx_bytes_per_sec == 0.0 && r.tx_bytes_per_sec == 0.0)
        );
        let second = sampler.sample();
        assert!(second.len() <= 8);
    }

    #[test]
    fn list_connections_is_ok_or_descriptive() {
        match list_connections() {
            Ok(conns) => assert!(conns.len() <= MAX_CONNECTIONS),
            Err(msg) => assert!(!msg.is_empty()),
        }
    }
}
