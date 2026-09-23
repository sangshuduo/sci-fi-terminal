# Configuration, themes and customization

## Files and precedence

Use the platform configuration directory: Linux `$XDG_CONFIG_HOME/sci-fi-terminal` (default `~/.config/sci-fi-terminal`), macOS `~/Library/Application Support/sci-fi-terminal`, Windows `%APPDATA%\sci-fi-terminal`. Runtime cache goes to the platform cache/local-data directory, not config. Resolve using platform APIs rather than hardcoded usernames. Proposed filenames: `config.toml`, `ui-overrides.toml`, `layout.toml`, `themes/<id>.toml`.

Effective precedence: built-in defaults → user config → UI-managed overrides → explicit command-line session overrides. Environment variables configure paths only unless explicitly documented; do not expand arbitrary `$VAR` or execute substitutions in values. No implicit current-directory/project config loading. Maps merge by key; arrays replace, except keybindings are keyed by stable action/context ID. UI shows the winning source and can remove an override to reveal the user file.

A single serialized config writer validates the entire candidate, writes a same-directory temporary file with restrictive permissions, flushes it, atomically replaces the destination using OS-appropriate APIs, and preserves a last-good backup. Windows replacement behavior is tested explicitly. UI edits update the generated overrides file, preserving user comments in `config.toml`; concurrent external changes trigger reload/conflict resolution rather than silent overwrite. Debounce file changes by 250 ms. Parse/validate off-thread and swap an immutable validated snapshot on the UI thread.

`schema_version` is required in user-written files. Unknown fields cause a diagnostic with location/suggestion and reject that candidate; newer unsupported schemas open read-only with a clear error. Migrations are explicit version-to-version transforms, make backups, and never execute commands. Startup with invalid settings uses defaults or last-good config and offers a repair view; it must not repeatedly overwrite the broken file. Add proposed `--check-config`, `--config <path>`, and `--safe-mode` flags; safe mode ignores custom themes/layout and overrides without deleting them.

## Illustrative config contract

This is valid TOML illustrating the proposed schema; no application parser exists yet.

```toml
schema_version = 1

[appearance]
theme = "graphite"
font_family = "monospace"
font_size = 14.0
line_height = 1.15
ligatures = false
ui_scale = 1.0
reduced_motion = true

[terminal]
scrollback_lines = 10000
scrollback_max_mib = 32
cursor_shape = "block"
cursor_blink = false
confirm_multiline_paste = true
copy_on_select = false

[profiles.default]
# Empty executable means discover the OS default shell.
executable = ""
args = []
login_shell = false

[layout]
preset = "focus"
restore = true
max_sessions = 8

[panels.metrics]
enabled = true
interval_ms = 1000

[panels.processes]
enabled = true
count = 6              # 1-10; name/PID/CPU/memory only

[panels.network]
enabled = true
interval_ms = 2000     # >= 1000
connections = true
geoip_database = ""    # local .mmdb path; empty = off; never downloaded
globe = true           # wireframe globe with peer markers
globe_rotate = true    # <= 30 Hz while visible; still when reduced_motion = true

[panels.files]
enabled = true
show_hidden = false

[effects]
preset = "off"
intensity = 0.15
max_fps = 30

[sound]
enabled = false
volume = 0.4           # 0-1
keypress = false       # typing ticks, separately toggleable

[input]
option_as_alt = false  # macOS: Option sends ESC-prefixed Meta
touch_scroll = true

[keyboard]
on_screen = false
layout = "en-us"       # built-in, or keyboards/<id>.toml

[[keybindings]]
action = "session.new_tab"
context = "global"
keys = "Ctrl+Shift+T"

[[keybindings]]
action = "terminal.copy"
context = "terminal"
keys = "Ctrl+Shift+C"
```

Default platform shortcuts are generated separately; the example is an explicit override, not a macOS default. Validate font size 8–48 logical px, line height 1–2, UI scale 0.75–3, intensity 0–1, metrics interval ≥1000 ms, and max sessions 1–8 for MVP. Unsupported effect presets are rejected. Lowering a limit must describe affected sessions/history before Apply; increasing limits beyond the supported maximum is rejected rather than causing uncontrolled allocations.

## Theme contract

Three independently designed built-ins: Graphite (neutral dark, cool accent), Daylight (light surfaces, dark text), and Signal (dark amber accent). Also supply a high-contrast preset derived from documented contrast checks, not another project's palette. Store all theme colors in semantic tokens and include the full ANSI16 terminal palette. UI state colors must cover normal/hover/pressed/disabled/focused/error; contrast warnings appear in theme preview. True-color terminal output is controlled by applications and may not match UI contrast.

```toml
schema_version = 1
id = "my-graphite"
name = "My Graphite"

[colors]
background = "#101820"
surface = "#1B2630"
foreground = "#E5EDF3"
muted = "#A6B4BF"
accent = "#77C7E8"
error = "#FF8C8C"
selection = "#345568"
cursor = "#E5EDF3"

[terminal]
foreground = "#E5EDF3"
background = "#101820"
ansi = ["#14212B", "#D96F78", "#83B98A", "#D9BC76",
        "#7DA5D8", "#BB91CE", "#76BFC5", "#C7D0D8",
        "#697B89", "#FF939C", "#A2D7AA", "#F5D998",
        "#A1C4F0", "#D8B0E8", "#9DDFE4", "#F3F7FA"]
```

An optional `[style]` table shapes the chrome without code (ADR-005). This is the supported alternative to stylesheet injection:

```toml
[style]
corner_radius = 2.0     # 0-16 px
border_width = 1.0      # 0-4 px
glow = 0.35             # 0-1, static edge glow when effects are on
ui_font = ""            # font family name for panel titles and tabs; empty = system UI font
```

On-screen keyboard layouts live in `keyboards/<id>.toml`. Each key has exactly one of `text` (with optional `shifted`), `named` (e.g. `enter`, `backspace`, `up`), `modifier` (`shift`/`ctrl`/`alt`) or `action = "caps"`. It also accepts optional `label` and `width` fields. Keys cannot hold multi-character command strings.

Missing state-specific UI tokens derive from built-in defaults with contrast checks; missing required base or ANSI tokens reject the file. Theme previews show editor text, cursor, selection, ANSI palette, dialogs, and focused controls. Theme values never include filesystem imports, URLs, scripts, or shader source. Limit theme/config files to 1 MiB and theme identifiers to safe filename-independent IDs. Theme discovery does not follow symlinks outside the selected theme directory. User font files are a separately selected local resource; no automatic network font fetches.

Theme switching and font changes apply live and invalidate appropriate render caches; font/DPI changes also trigger consistent cell/PTY resize epochs. Shell executable/argv/environment changes affect new sessions only. GPU backend changes require restart. Layout persists geometry and safe profile identifiers, not screen contents, environment secrets, running commands, or executable overrides from untrusted files. Restoring layout creates fresh sessions under explicitly configured profiles and labels them as new.

## Usability and settings coverage

Basic settings expose themes, font family/size, panel visibility, effects/reduced motion, and shortcuts. Advanced settings expose line height, cursor behavior, history caps, shell profiles, explicit environment overrides, and paste policy. The same typed schema powers validation and help; avoid a second divergent UI schema. Provide reset-section, reset-all, export-config, and open-config-directory actions. Show shortcut conflicts and reserved OS combinations before saving. Configuration errors must reference the actual file and field without exposing secret environment values.
