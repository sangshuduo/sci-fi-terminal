//! Offline GeoIP lookups against a user-supplied MaxMind-format `.mmdb` file.
//!
//! This module never downloads anything and performs no network I/O. The
//! database is read fully into memory (bounded to 512 MiB) when opened.
//!
//! Testing note: no `.mmdb` fixture ships with the repository, so
//! database-backed lookups are not unit-tested. The open error paths and the
//! public/private gate ([`should_lookup`]) are.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use maxminddb::{Reader, geoip2};

use super::connections::is_public;

/// Maximum accepted database size (512 MiB).
const MAX_DB_BYTES: u64 = 512 * 1024 * 1024;

/// Maximum number of cached lookups before the cache is cleared.
const MAX_CACHE_ENTRIES: usize = 1024;

/// Location information for an IP address. Every field is optional because
/// Country databases and sparse City records omit data.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoLocation {
    /// ISO 3166-1 alpha-2 country code.
    pub country_code: Option<String>,
    /// English country name.
    pub country: Option<String>,
    /// English city name.
    pub city: Option<String>,
    /// Approximate latitude.
    pub latitude: Option<f64>,
    /// Approximate longitude.
    pub longitude: Option<f64>,
}

/// Errors opening a GeoIP database.
#[derive(Debug, thiserror::Error)]
pub enum GeoIpError {
    /// The file could not be read or is not a valid MaxMind database.
    #[error("GeoIP database {0} could not be opened: {1}")]
    Open(PathBuf, String),
    /// The file exceeds the size limit.
    #[error("GeoIP database {0} is larger than 512 MiB")]
    TooLarge(PathBuf),
}

/// An in-memory offline GeoIP database with a small bounded lookup cache.
pub struct GeoIp {
    reader: Reader<Vec<u8>>,
    cache: HashMap<IpAddr, Option<GeoLocation>>,
}

impl std::fmt::Debug for GeoIp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoIp")
            .field("cache_entries", &self.cache.len())
            .finish_non_exhaustive()
    }
}

impl GeoIp {
    /// Open a local `.mmdb` (City or Country database). The file size is
    /// checked against 512 MiB before reading, and the read itself is capped
    /// so a file growing concurrently cannot exceed the limit.
    pub fn open(path: &Path) -> Result<Self, GeoIpError> {
        let open_err =
            |e: &dyn std::fmt::Display| GeoIpError::Open(path.to_path_buf(), e.to_string());
        let file = File::open(path).map_err(|e| open_err(&e))?;
        let len = file.metadata().map_err(|e| open_err(&e))?.len();
        if len > MAX_DB_BYTES {
            return Err(GeoIpError::TooLarge(path.to_path_buf()));
        }
        let mut buf = Vec::new();
        file.take(MAX_DB_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| open_err(&e))?;
        if buf.len() as u64 > MAX_DB_BYTES {
            return Err(GeoIpError::TooLarge(path.to_path_buf()));
        }
        let reader = Reader::from_source(buf).map_err(|e| open_err(&e))?;
        Ok(Self {
            reader,
            cache: HashMap::new(),
        })
    }

    /// Look up only public addresses; private/loopback/reserved addresses
    /// return `None` without touching the database. Uses English names.
    /// Never panics on missing or malformed fields (they become `None`).
    pub fn lookup(&mut self, ip: IpAddr) -> Option<GeoLocation> {
        if !should_lookup(ip) {
            return None;
        }
        if let Some(hit) = self.cache.get(&ip) {
            return hit.clone();
        }
        let result = self.query(ip);
        if self.cache.len() >= MAX_CACHE_ENTRIES {
            self.cache.clear();
        }
        self.cache.insert(ip, result.clone());
        result
    }

    /// Performs the uncached database lookup.
    fn query(&self, ip: IpAddr) -> Option<GeoLocation> {
        let found = self.reader.lookup(ip).ok()?;
        let city: geoip2::City<'_> = found.decode().ok()??;
        let location = GeoLocation {
            country_code: city.country.iso_code.map(str::to_owned),
            country: city.country.names.english.map(str::to_owned),
            city: city.city.names.english.map(str::to_owned),
            latitude: city.location.latitude.filter(|v| v.is_finite()),
            longitude: city.location.longitude.filter(|v| v.is_finite()),
        };
        let empty = location.country_code.is_none()
            && location.country.is_none()
            && location.city.is_none()
            && location.latitude.is_none()
            && location.longitude.is_none();
        (!empty).then_some(location)
    }
}

/// The public/private gate: only globally routable addresses are looked up.
pub(crate) fn should_lookup(ip: IpAddr) -> bool {
    is_public(ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch_file(name: &str, contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sft-geoip-{}-{name}", std::process::id()));
        let mut f = File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        path
    }

    #[test]
    fn open_missing_file_is_open_error() {
        let path = std::env::temp_dir().join("sft-geoip-definitely-missing.mmdb");
        let err = GeoIp::open(&path).unwrap_err();
        assert!(matches!(err, GeoIpError::Open(p, _) if p == path));
    }

    #[test]
    fn open_non_mmdb_file_is_open_error() {
        let path = scratch_file("bogus.mmdb", b"this is not a maxmind database");
        let result = GeoIp::open(&path);
        let _ = std::fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(matches!(err, GeoIpError::Open(..)));
        assert!(err.to_string().contains("could not be opened"));
    }

    #[test]
    fn too_large_error_message() {
        let err = GeoIpError::TooLarge(PathBuf::from("/x.mmdb"));
        assert_eq!(
            err.to_string(),
            "GeoIP database /x.mmdb is larger than 512 MiB"
        );
    }

    #[test]
    fn gate_skips_private_and_allows_public() {
        for s in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "::1",
            "fe80::1",
            "100.64.0.1",
        ] {
            assert!(!should_lookup(s.parse().unwrap()), "{s}");
        }
        for s in ["8.8.8.8", "2606:4700:4700::1111"] {
            assert!(should_lookup(s.parse().unwrap()), "{s}");
        }
    }
}
