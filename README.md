# sci-fi-terminal — independent native terminal

**Status: pre-MVP developer preview.** A working vertical slice exists (real PTY shells, tabs/splits, settings, themes, system panel); MVP gates are not yet met or benchmarked. See [implementation status](docs/IMPLEMENTATION_STATUS.md) for what is done and what is open.
The project and directory name is `sci-fi-terminal`.

```sh
cargo run -p sci-fi-terminal --release      # launch
cargo run -p sci-fi-terminal -- --help       # --config, --check-config, --safe-mode
cargo test --workspace
```

An original Rust-first desktop terminal combining a dependable daily shell with optional GPU-rendered instrumentation and visual styling. Windows, macOS, and Linux share the application core. There is no webview, Electron runtime, mandatory service, or cloud dependency.

The recommended starting architecture is Iced with its wgpu renderer, a custom terminal surface, an isolated `alacritty_terminal` adapter, and `portable-pty`. This is a provisional engineering recommendation, subject to the feasibility gates in the plan; compatible third-party crates are permitted, while eDEX-UI source, implementation details, assets, and project-specific designs are excluded.

## Reading order

1. [Implementation plan](docs/IMPLEMENTATION_PLAN.md): sequence, scope, milestones, decisions, and handoff.
2. [Technical specification](docs/TECHNICAL_SPEC.md): architecture, ownership, rendering, UI, and extension boundaries.
3. [Technology decisions](docs/DECISIONS.md): alternatives, dependencies, version policy, and primary sources.
4. [Terminal contract](docs/TERMINAL.md): PTY lifecycle, emulation, input, Unicode, compatibility, and limits.
5. [Configuration and themes](docs/CONFIGURATION.md): customization model and illustrative files.
6. [Security and provenance](docs/SECURITY_AND_PROVENANCE.md): clean-room process, threat model, and licensing.
7. [Verification and performance](docs/TESTING.md): acceptance cases and measurable budgets.
8. [Delivery and development](docs/DEVELOPMENT.md): proposed repository, platforms, workflow, packaging, and release gates.

9. [Implementation status](docs/IMPLEMENTATION_STATUS.md): what the code delivers today and the known gaps against the spec.

`crates/` and `.github/workflows/ci.yml` now exist; proposed paths under `tests/`, `assets/`, `packaging/`, `benches/` and `fuzz/` are still future work. No third-party source has been vendored.
