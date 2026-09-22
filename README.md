# sci-fi-terminal — independent native terminal

**Status: design and planning only. No application has been implemented or benchmarked.**
The project and directory name is `sci-fi-terminal`.

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

All proposed paths below `crates/`, `tests/`, `assets/`, and `.github/` describe future implementation, not files already delivered. Example TOML is a proposed contract, not a supported application interface. No packages have been installed and no third-party source has been vendored during this planning task.
