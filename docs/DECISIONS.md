# Technology decisions and dependency register

Status: proposed, researched 2026-09-22. These are engineering recommendations, not a compiled compatibility matrix. Resolve exact versions, MSRV, enabled features and licenses in M0; commit a lockfile only during implementation. Never independently choose the latest GUI, wgpu, and glyphon releases and assume they share compatible types. No dependencies are installed by these documents.

## ADR-001: GUI and render integration

**Decision:** Start with Iced's wgpu path and an original custom terminal surface. **Drivers:** terminal latency/correctness, maintainable native settings and input, restrained resource use. **Why:** a structured update/view model suits typed settings and pane state while leaving the terminal renderer replaceable. Iced documents a cross-platform Elm-inspired API and explicitly describes itself as experimental; that risk is material. [Iced 0.14 API](https://docs.rs/iced/0.14.0/iced/).

| Viable option | Strengths | Costs and recommendation |
|---|---|---|
| Iced + custom GPU terminal | Rust UI, structured state/actions, common controls and theming | Custom renderer integration, accessibility, and version churn must pass M0; first choice |
| egui/eframe + wgpu terminal callback | Fast tool/panel development, custom painting, native integration | Need disciplined event-driven repainting, deliberate product styling and custom accessible terminal; best fallback |
| winit + wgpu + custom UI | Full scheduling/render control, minimal framework assumptions | Own settings widgets, focus, layout, IME integration and accessibility; excessive initial scope |
| Slint + renderer integration | Declarative UI and mature tooling options | Investigate native GPU embedding and selected license obligations; not selected without integration/license evaluation |
| Qt via Rust bindings | Mature desktop facilities and accessibility | C++/Qt build and distribution surface weakens Rust-first simplicity; viable if native-widget maturity dominates |

[egui documentation](https://docs.rs/egui/latest/egui/) and [winit documentation](https://docs.rs/winit/latest/winit/) describe their respective UI and window/event roles. [Slint licensing options](https://slint.dev/pricing) require selection rather than assuming a permissive license. A browser/webview or Electron solution fails the explicit architecture requirement and is not a fallback.

**Consequences:** custom-drawn Rust-native UI is not the same as each OS's stock widgets; input and accessibility still require platform validation. Do not fork GUI internals to rescue a failed spike. **Follow-up:** prove public API integration, common GPU device, fractional DPI, IME, accessibility and idle behavior on every target. If Iced fails, run the same fixture on egui; if neither passes, revise scope/schedule before choosing a bespoke UI.

## ADR-002: Terminal core and PTY

**Decision:** wrap `alacritty_terminal`; use `portable-pty` for process I/O. **Drivers:** escape-sequence correctness, manageable scope, separation of platform lifecycle from UI.

| Option | Benefits | Costs |
|---|---|---|
| Existing independent terminal core | Mature emulation foundations, less protocol work | API coupling and inherited defects; audit boundedness and event policies |
| `vte` parser plus original grid/model | Maximum behavior control, smaller parser boundary | Parser is not a terminal: must implement reflow, modes, selection, history and queries |
| Entirely original parser/model | Maximum ownership | Largest compatibility and security burden; unsuitable for initial MVP |

Selected core is Apache-2.0 according to its [crate metadata](https://docs.rs/crate/alacritty_terminal/0.26.0). Portable-pty publishes a cross-platform interface and MIT license in its [metadata](https://docs.rs/crate/portable-pty/0.9.0). Third-party reuse here is explicitly allowed by the request; no eDEX-UI source or design details are part of this decision. **Consequences:** stable internal adapter types must contain upstream API churn; process lifecycle remains our responsibility. **Follow-up:** map modes/events, prove parser limits, pin compatible versions, run independent protocol tests.

## ADR-003: Rendering and text

**Decision:** wgpu, shared with GUI; reuse the GUI's compatible text infrastructure if usable, otherwise compatible glyphon/cosmic-text integration. [wgpu](https://docs.rs/wgpu/latest/wgpu/) provides cross-platform GPU abstractions; [glyphon](https://docs.rs/glyphon/latest/glyphon/) is a GPU text renderer. Neither automatically solves terminal cluster-to-cell placement.

Direct Metal/D3D/Vulkan backends could optimize platform details but multiply code and validation. OpenGL alone complicates the long-term macOS path. CPU rendering reduces GPU prerequisites but cannot satisfy the primary GPU requirement as the main architecture. **Consequences:** GPU/driver minimums and device recovery are release contracts; optional effects share a finite budget. **Follow-up:** one compatible dependency graph and measured atlas/resize behavior. No production commitment to custom shaders or software fallback until proven.

## ADR-004: Configuration and extension execution

**Decision:** versioned TOML, typed Rust schema, data-only themes, compile-time panels. TOML is readable and comment-friendly; JSON is tool-friendly but lacks comments; Lua offers programmable config but introduces executable startup behavior and a larger attack surface. Use `toml_edit` only if preserving user comments cannot be achieved through managed overrides.

Dynamic native libraries expose unstable Rust ABI and process-wide trust. Subprocess protocols add IPC and packaging but isolate failures; WASM permits resource/capability control at the cost of runtime/host API complexity. Neither is needed for MVP. **Follow-up:** a versioned external protocol ADR before executable extensions; no speculative plugin SDK dependency now.

## Proposed dependency budget

| Dependency | Purpose | License posture / admission condition |
|---|---|---|
| iced | UI and native integration | MIT upstream; enable wgpu and only required widgets/platform features |
| wgpu | GPU abstraction, through compatible GUI graph | MIT OR Apache-2.0 upstream; avoid duplicate major versions |
| alacritty_terminal | Emulation adapter | Apache-2.0; retain notices; inspect enabled transitive graph |
| portable-pty | PTY and child abstraction | MIT; verify Unix and ConPTY handle lifecycle |
| glyphon / cosmic-text | Conditional grid text integration | MIT OR Apache-2.0 upstream; add directly only if existing stack cannot be reused |
| serde + toml | Typed configuration | Validate exact release licenses and features before admission |
| directories | OS config/cache locations | Optional small adapter dependency; use standard platform conventions |
| sysinfo | CPU/memory snapshots | MIT upstream; no full process scans in default polling |
| tracing | Bounded diagnostic events | No terminal-content logging, no network sink |
| thiserror | Typed library errors | Optional; do not add overlapping error frameworks |
| proptest / criterion / cargo-fuzz | Test/benchmark tooling | Development-only, admitted when harnesses are implemented |

Upstream metadata references: [Iced](https://docs.rs/crate/iced/0.14.0), [wgpu](https://docs.rs/crate/wgpu/latest), [cosmic-text](https://docs.rs/crate/cosmic-text/latest), [glyphon](https://docs.rs/crate/glyphon/latest), [sysinfo](https://docs.rs/crate/sysinfo/latest). These observations are not a license clearance of an eventual resolved binary. Every transitive dependency, native library, font and asset must be inventoried from the actual build.

Use standard-library synchronization initially; add crossbeam or an async runtime only for a measured requirement. Prefer GUI-provided clipboard/input/window facilities over duplicate crates. Do not add a game engine, embedded browser, database, scripting runtime, telemetry SDK, or network client to the MVP without a new justified requirement. Recommendations are not authorization to install dependencies during this documentation phase.

## Version and supply-chain policy

At M0 record exact Rust stable toolchain and minimum supported Rust version; test both when practical. Rust edition 2024 is the proposed starting point. Review enabled features with `cargo tree -e features`, duplicate versions with `cargo tree -d`, and license/advisory results against the lockfile. Pin release builds with `--locked`; avoid git dependencies unless a documented upstream fix is indispensable and pinned to a reviewed commit. Review updates in bounded PRs, rerunning terminal fixtures and GPU/OS smoke cases. Never copy upstream application source as a shortcut around unsupported APIs.
