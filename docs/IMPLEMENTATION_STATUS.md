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
  - Command-line flags: `--check-config`, `--config`, `--safe-mode`.
- **Panels.**
  - Panels come from a compile-time registry and receive a scoped, read-only context.
  - The CPU/memory panel samples on its own worker thread and only while visible: every 1 s or more while focused, every 5 s or more otherwise.
  - A second built-in panel (Sessions) confirms that a new panel can be registered without touching terminal internals.
- **CI.** `.github/workflows/ci.yml` runs fmt and clippy on Linux, then tests and a release build on Linux, macOS and Windows. Actions are pinned to commit SHAs.

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
| Option-as-Alt | macOS Option is always treated as a text modifier; the setting is not exposed yet. | TERMINAL §Unicode, selection and input |
| Packaging | No installers, notices bundle, SBOM or signing. | DEVELOPMENT §Packaging |
| Evidence | No benchmark baseline, fuzzing, vttest subset or Linux/Windows desktop smoke tests. | TESTING |

## How to run

```sh
cargo run -p sci-fi-terminal --release
cargo run -p sci-fi-terminal -- --check-config
cargo test --workspace
```
