# Third-party notices

sci-fi-terminal is MIT-licensed (see `LICENSE`). It bundles or uses the
following third-party data. Provenance, versions and hashes are recorded in
`docs/provenance.csv`.

## DB-IP IP to City Lite database

- **File:** `dbip-city-lite.mmdb` (bundled with release archives; fetched and
  verified by `scripts/fetch-geoip.sh` from `assets/geo/dbip-city-lite.toml`)
- **Source:** https://db-ip.com/db/download/ip-to-city-lite
- **Licence:** Creative Commons Attribution 4.0 International (CC BY 4.0),
  https://creativecommons.org/licenses/by/4.0/
- **Attribution:** IP Geolocation by DB-IP — https://db-ip.com
- **Changes:** none; distributed as downloaded (decompressed from `.mmdb.gz`).
- The application shows this attribution in the Network panel wherever
  GeoIP results are displayed.

## Natural Earth 1:110m coastlines

- **File:** `assets/geo/coastline-110m.bin` (compiled into the binary)
- **Source:** https://github.com/nvkelso/natural-earth-vector
- **Licence:** public domain ("Made with Natural Earth")
- **Changes:** converted to a compact binary polyline format by
  `scripts/convert_coastline.py`; no geometry edits.

## Patched `block` 0.1.6 (vendored)

- **Path:** `third_party/block` (used via `[patch.crates-io]` in `Cargo.toml`)
- **Source:** https://crates.io/crates/block/0.1.6 — © Steven Sheldon, MIT licence
- **Changes:** two mechanical edits so the crate builds without future-incompatibility or lint warnings; see `third_party/block/PATCHED.md`.

## Rust crates

Rust dependencies are listed in `Cargo.lock` with their licences recorded in
`docs/DECISIONS.md`. A generated per-crate licence bundle (e.g. with
`cargo about`) is a release-gate item that is not yet automated; see
`docs/IMPLEMENTATION_STATUS.md`.
