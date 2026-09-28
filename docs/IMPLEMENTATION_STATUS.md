# Implementation status

Status as of 2026-09-22, branch `feat/terminal-foundation`. This records what exists against [TECHNICAL_SPEC](TECHNICAL_SPEC.md) and what is still open. It is not evidence that any MVP gate in [IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md) has been met. No benchmarks have been run, and only macOS (arm64) has been exercised by hand.

## Workspace

| Crate | Spec module | Contents |
|---|---|---|
| `terminal-core` | `terminal-core` | Adapter over `alacritty_terminal` 0.26, typed ids, immutable snapshots with damage, host-effect mediation, literal search, and key, paste and mouse encoding |
| `platform` | `platform` | `PtyBackend`/`PtyControl`/`ChildWaiter`/`ProcessKiller` traits, `portable-pty` 0.9 backend, shell discovery, platform paths |
| `sci-fi-terminal` (`crates/app`) | `app::{session,ui,render,config,panels}` | Session workers, Iced 0.14 UI, canvas terminal renderer, configuration and themes, panels |

## Implemented

- **Sessions.** Each session runs four threads (reader, writer, lifecycle waiter, model owner) behind a doorbell channel.
  - Output is capped at 256 KiB (four 64 KiB chunks). A full queue applies backpressure and drops no bytes.
  - Protocol replies have a 16 KiB reserve, drained four replies for every one input chunk. A child that stops reading moves the session to `Failed` rather than deadlocking.
  - Typed input is capped at 64 KiB and explicit pastes at 1 MiB.
  - Work runs in batches of 64 KiB or 2 ms, and only the newest snapshot is kept.
  - Wakeups are coalesced.
  - Resizes are coalesced, and each applied resize increments an epoch. If the PTY resize fails, the old epoch stays in place and the error is shown.
  - Close runs as a state machine entirely off the GUI thread: close input, hang up, drain for up to 500 ms, then reap with a 2 s grace period.
- **Terminal policy.**
  - Titles are sanitised and capped at 4 KiB.
  - OSC 52 reads and writes are denied.
  - Colour and size queries are answered from the active palette.
  - Overlong OSC sequences are discarded up to their terminator.
  - Bracketed paste rejects any payload containing the end-of-paste terminator. Multi-line or control-character pastes are shown for confirmation with control characters made visible.
  - Shells are spawned from an argv only, never a command string.
- **Rendering.**
  - The Iced canvas draws on Iced's own wgpu device; no second device is created.
  - Glyphs are placed on the grid using a cell advance measured through the renderer's cosmic-text.
  - Wide characters, combining marks, SGR attributes, selection, search hits and all cursor shapes render.
  - Frames are cached per pane, so idle panes do not redraw.
- **UI.**
  - A typed `Action` registry drives shortcuts, the command palette and settings help.
  - Default shortcuts differ between macOS and Windows/Linux. Ctrl+C is never intercepted on Unix.
  - Tabs and splits: pane ratios are clamped to 0.1–0.9, with at most 4 panes per tab and 8 sessions in total. Zoom, focus cycling and drag-resize work.
  - The layout file is versioned. It stores geometry and profile ids only; restoring starts fresh sessions.
  - Settings open as a draft with live preview. Apply writes a minimal diff to `ui-overrides.toml`; Cancel discards it. Settings have reset actions and a keybinding editor that reports conflicts.
  - Literal search over scrollback is capped at 10,000 results.
  - Exit and error states are drawn over the affected pane.
- **Configuration.**
  - A versioned TOML schema with range validation. Diagnostics name the file and field.
  - Layers apply in precedence order. Files are written atomically and the previous version is kept as a backup.
  - Four original themes: Graphite, Daylight, Signal and High contrast.
  - Command-line flags: `--check-config`, `--config`, `--safe-mode`, `-d/--working-directory`.
  - New shells start in the directory the terminal was launched from. When launched from Finder, the Dock or a desktop menu (working directory `/`), or when that directory no longer exists, they start in home. `-d DIR` overrides the launch directory, and a profile `cwd` overrides both.
- **Panels.**
  - Panels come from a compile-time registry and receive a scoped, read-only context.
  - The CPU/memory panel samples on its own worker thread and only while visible: every 1 s or more while focused, every 5 s or more otherwise.
  - Three independent panels: System (CPU, memory, swap and top processes), Network and Directory. Each has its own title-bar toggle and shortcut (Ctrl/Cmd+Shift+M, N and O). Directory docks on the left; System and Network are separate cards on the right. The earlier Sessions panel was removed.
- **CI.** `.github/workflows/ci.yml` runs fmt and clippy on Linux, then tests and a release build on Linux, macOS and Windows. Actions are pinned to commit SHAs.

## Workspace extensions (ADR-005)

- **Monitoring** (`crates/app/src/monitor`, `panels/worker.rs`)
  - One background worker samples only what the visible panels need: system, top processes (name/PID/CPU/memory; no command lines or environment), interface rates, and up to 200 sockets through `netstat2`.
  - Offline GeoIP comes from a user-supplied `.mmdb` file. Only public addresses are looked up, and nothing is downloaded.
  - Sampling runs every ≥ 1 s while the window is focused and every ≥ 5 s while unfocused, and stops when the panel is hidden.
- **Directory viewer** (`panels/files.rs`)
  - Follows the focused shell's working directory, read from the process table, and retargets when focus changes.
  - Read-only listing capped at 500 entries, with control characters in names masked.
  - Clicking a folder types a POSIX-quoted `cd -- '…'` without pressing Enter.
- **Touch and on-screen keyboard** (`osk/`, `render/terminal_view.rs`)
  - One-finger drag scrolls (mouse reporting and alternate-screen rules still apply). A tap focuses the pane and clears the selection.
  - The on-screen keyboard is built in (en-us), and user layouts can be added as `keyboards/<id>.toml`. Modifiers are sticky one-shot. Keys produce the same `KeyPress` events as a physical keyboard.
  - Toggled with `keyboard.toggle`, Ctrl/Cmd+Shift+K.
- **Sound** (`sound/`)
  - Seven cues synthesised at runtime; no audio files are shipped. Off by default.
  - Volume and typing sounds are set separately.
  - Rate-limited (keypress ≤ 1 per 30 ms, other cues ≤ 1 per 80 ms each, ≤ 4 overlapping). The audio device is opened on its own thread only when sound is enabled.
  - Cues: session start, exit, error, bell, and panel/keyboard toggles.
- **Theme styling** (`config/style.rs`)
  - An optional `[style]` table controls corner radius, border width, static glow and UI font. It is validated, rejects unknown keys, and has no code, URL or path surface.
  - Signal and Graphite ship with glow; High contrast uses square 2 px borders.
- **Home spot** (`monitor/public_ip.rs`, ADR-007)
  - An opt-in public-IP lookup, off by default. It uses HTTPS only, no redirects, a 5 s timeout and a 4 KiB response cap, at most every 30 min.
  - The default endpoint, ipinfo.io, returns a location, which is used directly and shown as "via ipinfo.io" (ADR-009). A plain-text endpoint falls back to the offline database, whose Lite data put the owner in Ottawa instead of Toronto.
  - The address is located with the offline database and marked on the globe with a flashing spot (1.2 s expanding ring). The spot is steady under reduced motion, and a still globe turns to face it.
  - Verified live with the DB-IP City Lite database (not bundled), which resolved the city correctly, using an `#[ignore]` test run on demand.
  - The flashing itself has not been seen on screen yet: the display was asleep when this was tested.
- **Bundled GeoIP** (ADR-008)
  - The DB-IP City Lite database (CC BY 4.0) is pinned by month and SHA-256 in `assets/geo/dbip-city-lite.toml`. `scripts/fetch-geoip.sh` fetches and verifies it; the 121 MB file itself is not committed.
  - `.github/workflows/release.yml` packages it next to the binary for Linux, macOS and Windows, in unsigned developer-preview archives with checksums, and publishes on `v*` tags.
  - With the setting left empty, the app finds the bundled file automatically. The Network panel shows "IP Geolocation by DB-IP", and the credit is also in `THIRD_PARTY_NOTICES.md`.
  - The release workflow has not run yet, because no tag has been pushed.
- **Application icon** (`assets/icons/`)
  - Original MIT artwork: an amber wireframe globe behind a `›_` prompt, with a teal "you are here" dot. A simplified SVG is used for 16–48 px.
  - `scripts/build-icons.sh` renders the PNG sizes, `.ico`, `.icns` and a raw 64 px RGBA window icon; the outputs are committed.
  - Window and taskbar icon on Linux and Windows comes from the embedded RGBA. On Windows, `crates/app/build.rs` also embeds the `.ico` in the `.exe` (not yet verified on Windows; CI builds it).
  - macOS: `scripts/bundle-macos.sh` builds an unsigned `sci-fi-terminal.app` with the `.icns` and the GeoIP database. The release workflow ships it; Linux archives carry a `.desktop` file and a 256 px PNG.
- **Launch window.** `[window] mode` (windowed, maximized or full screen) and `width/height` (640–7680 × 400–4320 logical px, default 1100 × 700) is set in Settings → Appearance and applied at the next launch. `main` reads the effective config before opening the window; an invalid file falls back to the default size. Verified: 1400 × 820 opened a 1400 × 820 content area.
- **File hover preview** (`panels/preview.rs`). Hovering a file in the Directory panel opens a floating window: the first 30 lines of UTF-8 text, or a thumbnail of a PNG, JPEG, GIF, WebP or BMP image. It is loaded on a debounced background thread with bounded reads and decoder limits, and FIFOs and devices are refused. Toggle: `panels.files.preview`. Covered by unit tests; the on-screen hover has not been checked by hand yet.
- **Connection owners.** Each Network panel connection line names its owning process, e.g. `tcp 1.2.3.4:443 · Norwell, US · firefox (4211)`. Only the owning pids are refreshed, and only their names are read (no command lines). Owners the OS hides from an unprivileged user show as `pid N`. In a live check on macOS, 73 of 73 remote connections were named.
- **Peer globe** (`render/globe.rs`, ADR-006)
  - Orthographic wireframe globe built from the bundled public-domain Natural Earth coastlines (provenance recorded in `docs/provenance.csv`), with markers for GeoIP-located peers.
  - Measured on an Apple Silicon Mac, release build, all monitoring panels visible (one-off readings, not a benchmark): about 3–4% CPU with the globe still, about 7% while it rotates. The first version cost about 11%. That dropped after coastline points were precomputed as unit vectors, each layer was drawn as a single path, and the tick was lowered to 20 Hz.
  - Rotates at a 20 Hz tick only while it can be seen and `reduced_motion` is off. That setting defaults to on, so set `appearance.reduced_motion = false` to see it turn.
- **Settings.** A new "Input, touch & sound" category, panel toggles, and a GeoIP path field. The config schema gains `[panels.processes|network|files]`, `[sound]`, `[input]` and `[keyboard]` sections.

## Known gaps (spec requirements not yet met)

| Area | Gap | Spec reference |
|---|---|---|
| Renderer | Uses a cached Iced canvas and issues one text draw per non-blank cell, rather than the custom wgpu primitive with instanced quads and a bounded 64 MiB glyph atlas. Performance against the frame-time and GPU-memory budgets has not been measured. | TECHNICAL_SPEC §GPU and text |
| IME | The canvas cannot request an input method, so preedit and candidate-window placement are missing. Committed text from dead keys does work. The fix is a custom widget that calls `request_input_method`. | TERMINAL §Unicode, selection and input |
| Accessibility | Iced 0.14 has no accessibility tree. The bounded text representation (`ViewportSnapshot::visible_text`) exists but is not connected to a native bridge. | TECHNICAL_SPEC §UI and component contract |
| Descendant cleanup | Termination is portable-pty's SIGHUP (Unix) or TerminateProcess (Windows). The process group is not escalated to SIGKILL, and no Windows Job Object is used. | TERMINAL §Lifecycle |
| Resize stall | PTY resize runs synchronously on the owner thread. The 500 ms stall timeout and cancellation are not implemented, and old-epoch hit tests are not letterboxed. | TECHNICAL_SPEC §Resize transaction |
| Search | Runs on the owner thread over the retained grid, not over off-thread 1 MiB chunks. It is not cancellable and not debounced, so each keystroke rescans the whole history and delays output for that session. Matches do not cross soft-wrapped rows. | TECHNICAL_SPEC §Architecture |
| History accounting | Scrollback is capped by the minimum of line count and bytes ÷ (columns × 32 B). Snapshot and search retention are not accounted separately. | TECHNICAL_SPEC §Scheduling |
| Config reload | No file watching and no 250 ms debounce; configuration changes take effect on restart or through the settings dialog. | CONFIGURATION §Files and precedence |
| Links / bell | OSC 8 links are not activated, and the bell is counted but not shown. | TERMINAL §Untrusted control sequences |
| Cursor blink | The setting is stored but ignored; the cursor is always steady. | CONFIGURATION |
| Extensions verification | Several paths have no test that exercises them for real. **GeoIP**: lookups against a real `.mmdb` are not unit-tested because no database fixture is bundled. **Sound**: playback was not checked by ear. **Touch**: gestures were not tried on touch hardware. **On-screen keyboard**: clicks were only exercised through unit tests (key → bytes). | ADR-005 |
| Directory viewer source | Reads the working directory of the shell process itself, not of its foreground child, and has no OSC 7 support yet. Windows may not expose it (the panel then shows "unavailable"). | ADR-005 follow-up |
| Connections | Sockets owned by other users may be hidden without elevated rights. PIDs are captured but not shown. | ADR-005 |
| Packaging | Release archives exist (tar.gz/zip with the binary or an unsigned `.app`, the GeoIP data, icon and notices), but there are no installers, SBOM, generated per-crate licence bundle or signing. | DEVELOPMENT §Packaging |
| Evidence | No benchmark baseline, fuzzing, vttest subset or Linux/Windows desktop smoke tests. | TESTING |

## How to run

```sh
scripts/fetch-geoip.sh   # fetch + verify the bundled DB-IP database (once)
cargo run -p sci-fi-terminal --release
cargo run -p sci-fi-terminal -- --check-config
cargo test --workspace
```
