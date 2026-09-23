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

## ADR-005: Workspace extensions (monitoring, touch, directory viewer, sound and theme styling)

**Status:** accepted by the project owner, 2026-09-22. It amends the MVP exclusions in [TECHNICAL_SPEC](TECHNICAL_SPEC.md#workspace-extensions-adr-005).

**Context:** the owner asked for a science-fiction workspace with these capabilities: real-time process and network monitoring (with GeoIP), touch displays with an on-screen keyboard, a directory viewer that follows the shell, deep theming, and optional sound effects. Only the *feature concepts* are taken as requirements. The clean-room boundary still applies to every layout, sound, asset and visual.

**Decisions:**

| Capability | Decision | Rejected alternatives |
|---|---|---|
| Processes | `sysinfo` process table, top N by CPU, name/PID/CPU/memory only | Shelling out to `ps`/`top` (spec forbids); collecting command lines (privacy) |
| Interfaces | `sysinfo` network counters, rate = delta ÷ elapsed | Packet capture (privileges, scope) |
| Connections | `netstat2` local socket tables (TCP/UDP, v4/v6), capped at 200 | `lsof`/`netstat` subprocesses; raw sockets |
| GeoIP | `maxminddb` reading a **user-supplied offline** database, opt-in | Online lookup services (sends peer IPs to a third party); bundling a database (licence and update burden) |
| Directory viewer | Process table cwd of the shell PID, polled only while visible; OSC 7 support deferred | Injecting shell hooks into user profiles |
| Touch | Iced touch events: drag-to-scroll, tap-to-focus | Custom gesture engine |
| On-screen keyboard | Built-in Iced widgets. TOML layouts emit ordinary key events | Arbitrary macro strings on keys (hidden command injection) |
| Sound | `rodio` playback of samples synthesised at runtime; off by default | Shipping recorded assets; decoder features (not needed) |
| Styling | Typed `[style]` tokens in theme TOML | CSS/stylesheet injection (no web layer, arbitrary code surface) |

**Consequences:**
- There are three more native-facing dependencies (`netstat2`, `maxminddb`, `rodio` and its audio backend), and each must be inventoried for notices before release.
- Connection listing may need elevated rights for other users' sockets. The panel shows what the OS allows.
- A process's working directory is not available on every OS for every process. The viewer states when it is unknown.

**Follow-up:**
- OSC 7 working-directory reports as a faster, more accurate source than the process table.
- Accessibility review of the on-screen keyboard.
- Audio-device loss handling.

## ADR-006: Peer globe for the network panel

**Status:** accepted by the project owner, 2026-09-22.

**Context:** the owner wants GeoIP results shown on a rotating globe, as an idea seen in another product. The owner explicitly chose a clean-room implementation: the other product's source, assets and visual design were not consulted.

**Decision:** draw an original wireframe globe on an Iced canvas in the network panel:
- **Projection:** orthographic, with an 18° tilt, a 30° graticule, and coastlines from Natural Earth 1:110m. The coastlines are public domain, converted by `scripts/convert_coastline.py` into a 21 KB asset recorded in `docs/provenance.csv`.
- **Markers:** peers located by the offline GeoIP database, deduplicated to a ~1° grid and capped at 64.
- **Rotation:** 6°/s on a 20 Hz tick (within the 30 Hz ceiling). It runs only while the panel is visible, the window is focused, and `reduced_motion` is off. Otherwise the globe is static and faces the first peer.
- **Redraw:** the canvas cache is cleared only on rotation ticks or marker changes. No timer exists while the globe is still.

**Rejected alternatives:**

| Alternative | Why rejected |
|---|---|
| Reading or porting the other product's globe | Violates the clean-room policy and is a GPL-3.0 licence risk |
| A 3-D engine or textured sphere | Unneeded dependency and GPU budget |
| Online tiles or maps | The app makes no network requests |

**Consequences:**
- Continuous redraws while rotating are an intentional, visible animation within the spec's 30 Hz ceiling, and they appear in idle CPU only when the user enables motion.
- The asset adds 21 KB to the binary.

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
| sysinfo | CPU/memory, process table, interface counters | MIT upstream; process sampling only while its panel is visible; never command lines or environment |
| netstat2 | Local socket tables for the network panel | MIT OR Apache-2.0; read-only, capped results (ADR-005) |
| maxminddb | Offline GeoIP lookups from a user-supplied `.mmdb` | ISC; no bundled database, no network (ADR-005) |
| rodio | Playback of runtime-synthesised sound cues | MIT OR Apache-2.0; `playback` feature only, no decoders (ADR-005) |
| tracing | Bounded diagnostic events | No terminal-content logging, no network sink |
| thiserror | Typed library errors | Optional; do not add overlapping error frameworks |
| proptest / criterion / cargo-fuzz | Test/benchmark tooling | Development-only, admitted when harnesses are implemented |

Upstream metadata references: [Iced](https://docs.rs/crate/iced/0.14.0), [wgpu](https://docs.rs/crate/wgpu/latest), [cosmic-text](https://docs.rs/crate/cosmic-text/latest), [glyphon](https://docs.rs/crate/glyphon/latest), [sysinfo](https://docs.rs/crate/sysinfo/latest). These observations are not a license clearance of an eventual resolved binary. Every transitive dependency, native library, font and asset must be inventoried from the actual build.

Use standard-library synchronization initially; add crossbeam or an async runtime only for a measured requirement. Prefer GUI-provided clipboard/input/window facilities over duplicate crates. Do not add a game engine, embedded browser, database, scripting runtime, telemetry SDK, or network client to the MVP without a new justified requirement. Recommendations are not authorization to install dependencies during this documentation phase.

## Version and supply-chain policy

At M0 record exact Rust stable toolchain and minimum supported Rust version; test both when practical. Rust edition 2024 is the proposed starting point. Review enabled features with `cargo tree -e features`, duplicate versions with `cargo tree -d`, and license/advisory results against the lockfile. Pin release builds with `--locked`; avoid git dependencies unless a documented upstream fix is indispensable and pinned to a reviewed commit. Review updates in bounded PRs, rerunning terminal fixtures and GPU/OS smoke cases. Never copy upstream application source as a shortcut around unsupported APIs.
