# Terminal subsystem contract

## Emulation choice

Use the independently licensed `alacritty_terminal` crate behind our adapter; this reuses a compatible third-party terminal engine, not eDEX-UI. Our orchestration, views, configuration, policies, input glue, and project assets remain original. The crate exposes a terminal model and events, including host-effect requests, so events must be mediated rather than blindly forwarded. [Core API](https://docs.rs/alacritty_terminal/latest/alacritty_terminal/) · [Event API](https://docs.rs/alacritty_terminal/latest/alacritty_terminal/event/enum.Event.html).

`portable-pty` supplies a common PTY interface; its reader/writer/child lifecycle still needs platform-specific testing. Do not instantiate the Alacritty PTY event loop and portable-pty simultaneously. Our session layer connects one PTY implementation to one model owner. [PTY API](https://docs.rs/portable-pty/latest/portable_pty/).

## Lifecycle and launch

States: Creating → Running → Closing → Exited, with Failed reachable from startup or I/O failures. Create PTY at initial rows/columns, spawn child with executable and argument vector, acquire reader/writer, release unused slave handles, start workers, and publish Running only after handles are ready. A startup failure tears down every acquired handle and preserves the error for display. Exit is recorded once; a closed session never receives new writes.

Resolve Unix default shell from the account database, then validated `SHELL`, then `/bin/sh`; use interactive arguments appropriate to known shell profiles. Login-shell mode is configurable, not silently simulated with `-c`. On Windows use configured profile, otherwise discovered PowerShell if present, otherwise `ComSpec`/cmd with a visible profile label. Never download a shell. GUI-launched macOS PATH differs from a terminal-launched process; document explicit environment overrides instead of running hidden login shells to scrape environment.

Spawn the executable directly with argv, cwd, and environment; never concatenate settings into an executable shell string. Preserve necessary inherited environment for ordinary developer workflows, apply explicit overrides/removals, and never log it. No privilege elevation. Missing cwd offers home-directory fallback visibly, not silent command relocation.

PTY read chunks pass through the byte parser incrementally: UTF-8 and escape sequences can span reads. Parser-generated replies and user input share a serialized writer. The model enqueues without blocking into separate bounded queues; the reserve, fairness and saturation-failure policy are specified in [the architecture](TECHNICAL_SPEC.md). A blocked child reader cannot block the parser owner while it enqueues replies. Use asynchronous UI notifications for exit and errors. Process status, spawn failure, EOF, and read errors are distinct events.

On close, warn if a live session will be terminated; do not promise reliable detection of all foreground jobs. Close input, initiate platform-specific hangup/termination, drain output for a bounded 500 ms, then request termination and reap with a 2 s grace period. Escalation to forced termination is scoped to the session's owned process group/job, never an unrelated PID discovered by name. Unix process groups and Windows Job Object ownership need M0 verification; some detached descendants may intentionally escape ownership and this limitation must be documented. Portable PTY kill alone is not proof of descendant cleanup. Worker cancellation, handle closure order, and reaping run off the UI thread. [ConPTY lifecycle reference](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session).

## MVP compatibility matrix

| Capability | Required behavior and proof |
|---|---|
| Text and SGR | UTF-8, 16/256/true color, inverse/bold/italic/underline/strike; deterministic grid fixtures |
| Cursor and erase | Cursor movement, origin modes, scroll regions, wrap, tab stops, insert/delete; fixtures and vttest subset |
| Buffers | Main/alternate screen, bounded history, resize/reflow without corrupting wide cells |
| Input | Arrows, function keys, modifiers, application cursor/keypad, Ctrl/Alt, AltGr and dead keys |
| Mouse | Supported tracking modes and SGR mouse; Shift bypass to local selection |
| Paste | Bracketed paste when enabled; multiline/control-character confirmation policy |
| Queries | Advertise and answer only supported capabilities; bounded replies |
| Desktop | Selection, copy, paste, literal search, cursor/IME candidate positioning |
| Applications | bash/zsh/fish where installed; PowerShell/cmd; vim or neovim, less, top/htop, tmux on Unix |

Start with `TERM=xterm-256color` only after the relevant compatibility subset passes; set `COLORTERM=truecolor` for implemented true color. Record deliberate deviations. Kitty keyboard, sixel, graphics protocols, shell integration OSC extensions, and remote terminfo installation are deferred. Do not advertise unsupported features or infer compatibility from the engine crate alone.

## Unicode, selection and input

The terminal core is authoritative for width/continuation cells. Map glyph clusters onto those cells and preserve original text for copy/search. Test CJK, combining accents, variation selectors, ZWJ emoji, ambiguous widths, missing fonts, and malformed UTF-8. Emoji presentation and shell wcwidth disagreements can remain documented limitations; never misalign subsequent ASCII columns. M0 decides color-emoji availability; monochrome fallback is acceptable for MVP.

IME preedit is a local overlay, never sent to the shell. Send committed text once; don't also emit the associated key event. Set the IME candidate rectangle from the actual caret at current DPI. Respect keyboard layout and AltGr rather than treating all right-Alt input as escape-prefixed shortcuts. Provide configurable Option/Alt behavior on macOS.

Selection tracks stable logical history coordinates and snapshot generation, not rendered pixel offsets. When history eviction removes anchors, clamp or clear selection visibly. Copy skips wide-cell continuation sentinels and preserves soft-wrap semantics. Search is literal for MVP, cancels on query/session change, and caps results at 10,000. Regex is deferred to avoid adding a second complex workload.

## Untrusted control sequences

Ignore unsupported OSC/DCS/APC effects. Titles are bounded (4 KiB), stripped of unsafe controls, and displayed as text. Bell is a rate-limited visual indication; sound defaults off. OSC 52 clipboard reads and writes are denied by default; future opt-in writes need explicit policy, payload caps, and user initiation/confirmation. Never answer clipboard-read requests automatically. OSC 8 links allow only http/https/mailto with explicit user activation and visible destination; other schemes and file links are disabled for MVP. Terminal output cannot launch commands, open files, write configuration, fetch fonts, or install themes.

Large paste is previewed/chunked, cancellable, and never silently truncated. Cap at 1 MiB and reject larger requests with an explanation. Warn for multiline or control-containing paste even with bracketed mode, because programs decide how pasted bytes are interpreted. Sanitization must not silently rewrite a command; show the actual content to be sent. Avoid blocking model progress if the write queue is full; backpressure and paste cancellation are explicit states.

## Protocol references

Author protocol fixtures from public behavior specifications: [XTerm control-sequence reference](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html) and [Microsoft virtual terminal sequence reference](https://learn.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences). These are interoperability references, not application-source templates. Record the reference revision and exact supported subset with each fixture; platform documentation is not evidence our implementation already conforms.
