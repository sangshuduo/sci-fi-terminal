# Vendored `block` 0.1.6 (patched)

- **Upstream:** https://crates.io/crates/block/0.1.6 (https://github.com/SSheldon/rust-block), MIT licence.
- **Why:** it is pulled in transitively on macOS by `iced 0.14` → `wgpu 27` → `wgpu-hal` → `metal 0.32`. Rust reports a future-incompatibility warning for it ("static of uninhabited type") that will become a hard error. `block` has no newer release, and `wgpu-hal` 30.x still depends on it.
- **Change:** `enum Class { }` became `#[repr(C)] struct Class { _private: [u8; 0] }` in `src/lib.rs`. In addition, bare `extern` / `extern fn` became `extern "C"` / `extern "C" fn`, which is identical to the implicit default and silences the `missing_abi` lint now that this crate is built as a path dependency (lints are no longer capped). Nothing else was changed. The static is only used by address, so layout and ABI are unchanged.
- **Remove when:** the resolved `wgpu-hal`/`metal` no longer depend on `block`. Check with `cargo tree -i block --target all`.
