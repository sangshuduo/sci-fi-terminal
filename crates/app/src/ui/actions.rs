//! Typed action registry and keybinding resolution.
//!
//! One registry drives the command palette, menus, shortcuts and settings
//! help. Keybindings bind known actions only; there are no command strings.

use std::collections::BTreeMap;
use std::fmt;

use crate::config::KeyBinding;

/// Every user-invokable application action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    NewTab,
    ClosePane,
    SplitRight,
    SplitDown,
    FocusNext,
    FocusPrevious,
    ZoomPane,
    NextTab,
    PreviousTab,
    Copy,
    Paste,
    Search,
    ScrollPageUp,
    ScrollPageDown,
    TogglePalette,
    OpenSettings,
    ToggleMetrics,
    ResetLayout,
    FontIncrease,
    FontDecrease,
    FontReset,
    ToggleKeyboard,
    ToggleSound,
}

/// Where a binding applies. Global bindings win over terminal encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Context {
    Global,
    Terminal,
}

impl Context {
    pub fn id(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Terminal => "terminal",
        }
    }

    fn parse(id: &str) -> Option<Self> {
        match id {
            "global" => Some(Self::Global),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

/// Static metadata for one action.
pub struct ActionInfo {
    pub action: Action,
    pub id: &'static str,
    pub title: &'static str,
    pub context: Context,
    /// Default shortcut on Windows/Linux.
    pub default_keys: Option<&'static str>,
    /// Default shortcut on macOS.
    pub mac_keys: Option<&'static str>,
}

macro_rules! action {
    ($action:ident, $id:literal, $title:literal, $ctx:ident, $keys:expr, $mac:expr) => {
        ActionInfo {
            action: Action::$action,
            id: $id,
            title: $title,
            context: Context::$ctx,
            default_keys: $keys,
            mac_keys: $mac,
        }
    };
}

/// The registry, in palette order.
pub const ACTIONS: &[ActionInfo] = &[
    action!(
        NewTab,
        "session.new_tab",
        "New tab",
        Global,
        Some("Ctrl+Shift+T"),
        Some("Cmd+T")
    ),
    action!(
        ClosePane,
        "session.close_pane",
        "Close pane",
        Global,
        Some("Ctrl+Shift+W"),
        Some("Cmd+W")
    ),
    action!(
        SplitRight,
        "pane.split_right",
        "Split right",
        Global,
        Some("Ctrl+Shift+D"),
        Some("Cmd+D")
    ),
    action!(
        SplitDown,
        "pane.split_down",
        "Split down",
        Global,
        Some("Ctrl+Shift+E"),
        Some("Cmd+Shift+D")
    ),
    action!(
        FocusNext,
        "pane.focus_next",
        "Focus next pane",
        Global,
        Some("Ctrl+Shift+]"),
        Some("Cmd+]")
    ),
    action!(
        FocusPrevious,
        "pane.focus_previous",
        "Focus previous pane",
        Global,
        Some("Ctrl+Shift+["),
        Some("Cmd+[")
    ),
    action!(
        ZoomPane,
        "pane.zoom",
        "Zoom pane",
        Global,
        Some("Ctrl+Shift+Z"),
        Some("Cmd+Shift+Enter")
    ),
    action!(
        NextTab,
        "tab.next",
        "Next tab",
        Global,
        Some("Ctrl+PageDown"),
        Some("Cmd+Shift+]")
    ),
    action!(
        PreviousTab,
        "tab.previous",
        "Previous tab",
        Global,
        Some("Ctrl+PageUp"),
        Some("Cmd+Shift+[")
    ),
    action!(
        Copy,
        "terminal.copy",
        "Copy selection",
        Terminal,
        Some("Ctrl+Shift+C"),
        Some("Cmd+C")
    ),
    action!(
        Paste,
        "terminal.paste",
        "Paste",
        Terminal,
        Some("Ctrl+Shift+V"),
        Some("Cmd+V")
    ),
    action!(
        Search,
        "terminal.search",
        "Find in scrollback",
        Terminal,
        Some("Ctrl+Shift+F"),
        Some("Cmd+F")
    ),
    action!(
        ScrollPageUp,
        "terminal.scroll_page_up",
        "Scroll up one page",
        Terminal,
        Some("Shift+PageUp"),
        Some("Shift+PageUp")
    ),
    action!(
        ScrollPageDown,
        "terminal.scroll_page_down",
        "Scroll down one page",
        Terminal,
        Some("Shift+PageDown"),
        Some("Shift+PageDown")
    ),
    action!(
        TogglePalette,
        "palette.toggle",
        "Command palette",
        Global,
        Some("Ctrl+Shift+P"),
        Some("Cmd+Shift+P")
    ),
    action!(
        OpenSettings,
        "settings.open",
        "Settings",
        Global,
        Some("Ctrl+,"),
        Some("Cmd+,")
    ),
    action!(
        ToggleMetrics,
        "panel.toggle_metrics",
        "Toggle system panel",
        Global,
        Some("Ctrl+Shift+M"),
        Some("Cmd+Shift+M")
    ),
    action!(
        ResetLayout,
        "layout.reset",
        "Reset layout",
        Global,
        None,
        None
    ),
    action!(
        FontIncrease,
        "font.increase",
        "Increase font size",
        Global,
        Some("Ctrl+="),
        Some("Cmd+=")
    ),
    action!(
        FontDecrease,
        "font.decrease",
        "Decrease font size",
        Global,
        Some("Ctrl+-"),
        Some("Cmd+-")
    ),
    action!(
        ToggleKeyboard,
        "keyboard.toggle",
        "Toggle on-screen keyboard",
        Global,
        Some("Ctrl+Shift+K"),
        Some("Cmd+Shift+K")
    ),
    action!(
        ToggleSound,
        "sound.toggle",
        "Toggle sound effects",
        Global,
        None,
        None
    ),
    action!(
        FontReset,
        "font.reset",
        "Reset font size",
        Global,
        Some("Ctrl+0"),
        Some("Cmd+0")
    ),
];

impl Action {
    pub fn info(self) -> &'static ActionInfo {
        // Every variant is registered; the registry test enforces this.
        ACTIONS
            .iter()
            .find(|info| info.action == self)
            .unwrap_or(&ACTIONS[0])
    }

    pub fn from_id(id: &str) -> Option<Self> {
        ACTIONS
            .iter()
            .find(|info| info.id == id)
            .map(|info| info.action)
    }

    pub fn title(self) -> &'static str {
        self.info().title
    }
}

/// A key chord such as `Ctrl+Shift+T`. `key` is a lower-case key name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
    pub key: String,
}

impl Chord {
    /// Parse `Mod+Mod+Key`. Modifiers: Ctrl, Shift, Alt/Option, Cmd/Super/Logo/Win.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut chord = Chord {
            ctrl: false,
            shift: false,
            alt: false,
            logo: false,
            key: String::new(),
        };
        let parts: Vec<&str> = split_chord(text);
        let Some((key, modifiers)) = parts.split_last() else {
            return Err("empty shortcut".into());
        };
        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" | "option" | "opt" => chord.alt = true,
                "cmd" | "command" | "super" | "logo" | "win" | "meta" => chord.logo = true,
                other => return Err(format!("unknown modifier `{other}`")),
            }
        }
        let key = normalize_key(key).ok_or_else(|| format!("unknown key `{key}`"))?;
        chord.key = key;
        Ok(chord)
    }

    fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.logo
    }

    /// Whether binding this chord would steal keys the user types into the
    /// terminal. Shift alone still produces text, so `Shift+A` blocks typing,
    /// while `Shift+PageUp` does not.
    fn blocks_typing(&self) -> bool {
        const TYPING_KEYS: &[&str] = &["enter", "tab", "space", "backspace", "escape", "delete"];
        let texty = self.key.chars().count() == 1 || TYPING_KEYS.contains(&self.key.as_str());
        !self.has_modifier() && texty
    }
}

/// Split on `+` while allowing `+` itself as the final key (`Ctrl++`).
fn split_chord(text: &str) -> Vec<&str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Some(prefix) = trimmed.strip_suffix("++") {
        let mut parts: Vec<&str> = prefix.split('+').map(str::trim).collect();
        parts.push("+");
        return parts;
    }
    trimmed.split('+').map(str::trim).collect()
}

const NAMED_KEYS: &[&str] = &[
    "enter",
    "tab",
    "space",
    "backspace",
    "escape",
    "delete",
    "insert",
    "home",
    "end",
    "pageup",
    "pagedown",
    "up",
    "down",
    "left",
    "right",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
];

fn normalize_key(key: &str) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    let lower = match lower.as_str() {
        "return" => "enter".to_owned(),
        "esc" => "escape".to_owned(),
        "pgup" => "pageup".to_owned(),
        "pgdn" => "pagedown".to_owned(),
        "del" => "delete".to_owned(),
        _ => lower,
    };
    let single_char = lower.chars().count() == 1;
    (single_char || NAMED_KEYS.contains(&lower.as_str())).then_some(lower)
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let logo = if cfg!(target_os = "macos") {
            "Cmd+"
        } else {
            "Super+"
        };
        let parts = [
            (self.ctrl, "Ctrl+"),
            (
                self.alt,
                if cfg!(target_os = "macos") {
                    "Option+"
                } else {
                    "Alt+"
                },
            ),
            (self.shift, "Shift+"),
            (self.logo, logo),
        ];
        for (on, label) in parts {
            if on {
                f.write_str(label)?;
            }
        }
        let mut chars = self.key.chars();
        match chars.next() {
            Some(first) if self.key.len() > 1 => {
                write!(f, "{}{}", first.to_ascii_uppercase(), chars.as_str())
            }
            _ => f.write_str(&self.key.to_ascii_uppercase()),
        }
    }
}

/// Resolved bindings plus diagnostics for the settings UI.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: BTreeMap<(Context, Chord), Action>,
    pub diagnostics: Vec<String>,
}

/// Combinations the OS or window manager typically reserves.
const RESERVED: &[&str] = &["Cmd+Q", "Cmd+H", "Cmd+Tab", "Alt+F4", "Alt+Tab", "Super+L"];

impl Keymap {
    /// Build from platform defaults, then apply user overrides keyed by action and context.
    /// An override with empty `keys` or `none` unbinds the action.
    pub fn build(overrides: &[KeyBinding], mac: bool) -> Self {
        let mut chosen: BTreeMap<(Action, Context), Option<String>> = ACTIONS
            .iter()
            .map(|info| {
                let keys = if mac {
                    info.mac_keys
                } else {
                    info.default_keys
                };
                ((info.action, info.context), keys.map(str::to_owned))
            })
            .collect();
        let mut keymap = Keymap::default();
        for binding in overrides {
            keymap.apply_override(binding, &mut chosen);
        }
        for ((action, context), keys) in chosen {
            if let Some(keys) = keys {
                keymap.bind(action, context, &keys);
            }
        }
        keymap
    }

    fn apply_override(
        &mut self,
        binding: &KeyBinding,
        chosen: &mut BTreeMap<(Action, Context), Option<String>>,
    ) {
        let Some(action) = Action::from_id(&binding.action) else {
            self.diagnostics
                .push(format!("unknown action `{}`", binding.action));
            return;
        };
        let Some(context) = Context::parse(&binding.context) else {
            self.diagnostics.push(format!(
                "unknown context `{}` for {}",
                binding.context, binding.action
            ));
            return;
        };
        let keys = binding.keys.trim();
        let value =
            (!keys.is_empty() && !keys.eq_ignore_ascii_case("none")).then(|| keys.to_owned());
        chosen.remove(&(action, action.info().context));
        chosen.insert((action, context), value);
    }

    fn bind(&mut self, action: Action, context: Context, keys: &str) {
        let chord = match Chord::parse(keys) {
            Ok(chord) => chord,
            Err(err) => {
                self.diagnostics
                    .push(format!("{}: {err}", action.info().id));
                return;
            }
        };
        // Both contexts are resolved before terminal encoding, so a bare text key
        // in either would swallow typing; global shortcuts also need a modifier.
        if chord.blocks_typing() || (context == Context::Global && !chord.has_modifier()) {
            self.diagnostics.push(format!(
                "{}: `{keys}` has no Ctrl/Alt/Cmd modifier and would block typing",
                action.info().id
            ));
            return;
        }
        if RESERVED
            .iter()
            .any(|reserved| Chord::parse(reserved).as_ref() == Ok(&chord))
        {
            self.diagnostics.push(format!(
                "{}: `{keys}` is usually reserved by the OS",
                action.info().id
            ));
        }
        if let Some(existing) = self.conflict(context, &chord) {
            self.diagnostics.push(format!(
                "`{keys}` is bound to both {} and {}; keeping {}",
                existing.info().id,
                action.info().id,
                existing.info().id
            ));
            return;
        }
        self.bindings.insert((context, chord), action);
    }

    fn conflict(&self, context: Context, chord: &Chord) -> Option<Action> {
        let other = match context {
            Context::Global => Context::Terminal,
            Context::Terminal => Context::Global,
        };
        self.bindings
            .get(&(context, chord.clone()))
            .or_else(|| self.bindings.get(&(other, chord.clone())))
            .copied()
    }

    /// Resolve a chord: global bindings first, then terminal-scoped ones.
    pub fn resolve(&self, chord: &Chord, terminal_focused: bool) -> Option<Action> {
        self.bindings
            .get(&(Context::Global, chord.clone()))
            .copied()
            .or_else(|| {
                terminal_focused
                    .then(|| {
                        self.bindings
                            .get(&(Context::Terminal, chord.clone()))
                            .copied()
                    })
                    .flatten()
            })
    }

    /// Shortcut shown next to an action in menus and the palette.
    pub fn shortcut_for(&self, action: Action) -> Option<&Chord> {
        self.bindings
            .iter()
            .find(|(_, bound)| **bound == action)
            .map(|((_, chord), _)| chord)
    }
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
