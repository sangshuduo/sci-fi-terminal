//! Settings dialog: a validated draft with live preview, Apply/Cancel and
//! a minimal override file that preserves the user's own `config.toml`.

use std::collections::BTreeMap;

use iced::widget::{
    Space, button, checkbox, column, container, pick_list, row, scrollable, slider, text,
    text_input,
};
use iced::{Element, Length};

use super::actions::{ACTIONS, Action, Chord, Keymap};
use crate::config::{Config, CursorShape, EffectsPreset, KeyBinding, Theme, validate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Terminal,
    Panels,
    InputSound,
    Keys,
}

impl Category {
    const ALL: [Category; 5] = [
        Self::Appearance,
        Self::Terminal,
        Self::Panels,
        Self::InputSound,
        Self::Keys,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Terminal => "Terminal",
            Self::Panels => "Panels & effects",
            Self::InputSound => "Input, touch & sound",
            Self::Keys => "Keyboard shortcuts",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SettingsMsg {
    Category(Category),
    Query(String),
    Theme(String),
    FontFamily(String),
    FontSize(f32),
    LineHeight(f32),
    ReducedMotion(bool),
    Effects(EffectsPreset),
    Cursor(CursorShape),
    CursorBlink(bool),
    ConfirmPaste(bool),
    CopyOnSelect(bool),
    Scrollback(String),
    Metrics(bool),
    Processes(bool),
    Network(bool),
    Connections(bool),
    GeoIpPath(String),
    Files(bool),
    ShowHidden(bool),
    Sound(bool),
    Volume(f32),
    TypingSound(bool),
    OptionAsAlt(bool),
    TouchScroll(bool),
    OnScreenKeyboard(bool),
    Key(Action, String),
    ResetSection,
    ResetAll,
    Apply,
    Cancel,
}

pub struct Settings {
    pub draft: Config,
    base: Config,
    category: Category,
    query: String,
    scrollback_text: String,
    key_edits: BTreeMap<Action, String>,
    pub errors: Vec<String>,
}

impl Settings {
    pub fn open(current: &Config, keymap: &Keymap) -> Self {
        let key_edits = ACTIONS
            .iter()
            .map(|info| {
                (
                    info.action,
                    keymap
                        .shortcut_for(info.action)
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                )
            })
            .collect();
        Self {
            draft: current.clone(),
            base: current.clone(),
            category: Category::Appearance,
            query: String::new(),
            scrollback_text: current.terminal.scrollback_lines.to_string(),
            key_edits,
            errors: Vec::new(),
        }
    }

    /// Apply a message to the draft. Returns true when the user chose Apply or Cancel.
    pub fn update(&mut self, message: SettingsMsg) {
        let appearance = &mut self.draft.appearance;
        let terminal = &mut self.draft.terminal;
        match message {
            SettingsMsg::Category(category) => self.category = category,
            SettingsMsg::Query(query) => self.query = query,
            SettingsMsg::Theme(id) => appearance.theme = id,
            SettingsMsg::FontFamily(family) => appearance.font_family = family,
            SettingsMsg::FontSize(size) => appearance.font_size = size.round(),
            SettingsMsg::LineHeight(height) => {
                appearance.line_height = (height * 20.0).round() / 20.0
            }
            SettingsMsg::ReducedMotion(on) => appearance.reduced_motion = on,
            SettingsMsg::Effects(preset) => self.draft.effects.preset = preset,
            SettingsMsg::Cursor(shape) => terminal.cursor_shape = shape,
            SettingsMsg::CursorBlink(on) => terminal.cursor_blink = on,
            SettingsMsg::ConfirmPaste(on) => terminal.confirm_multiline_paste = on,
            SettingsMsg::CopyOnSelect(on) => terminal.copy_on_select = on,
            SettingsMsg::Scrollback(value) => {
                if let Ok(lines) = value.trim().parse() {
                    terminal.scrollback_lines = lines;
                }
                self.scrollback_text = value;
            }
            SettingsMsg::Metrics(on) => self.draft.panels.metrics.enabled = on,
            SettingsMsg::Key(action, keys) => {
                self.key_edits.insert(action, keys);
            }
            SettingsMsg::ResetSection => self.reset_section(),
            SettingsMsg::ResetAll => {
                self.draft = Config::default();
                self.key_edits.values_mut().for_each(String::clear);
                self.reset_keys_to_defaults();
            }
            SettingsMsg::Apply | SettingsMsg::Cancel => {}
            other => self.update_extension(other),
        }
    }

    /// Workspace extensions from ADR-005: panels, sound, input and keyboard.
    fn update_extension(&mut self, message: SettingsMsg) {
        let draft = &mut self.draft;
        match message {
            SettingsMsg::Processes(on) => draft.panels.processes.enabled = on,
            SettingsMsg::Network(on) => draft.panels.network.enabled = on,
            SettingsMsg::Connections(on) => draft.panels.network.connections = on,
            SettingsMsg::GeoIpPath(path) => draft.panels.network.geoip_database = path,
            SettingsMsg::Files(on) => draft.panels.files.enabled = on,
            SettingsMsg::ShowHidden(on) => draft.panels.files.show_hidden = on,
            SettingsMsg::Sound(on) => draft.sound.enabled = on,
            SettingsMsg::Volume(volume) => draft.sound.volume = (volume * 20.0).round() / 20.0,
            SettingsMsg::TypingSound(on) => draft.sound.keypress = on,
            SettingsMsg::OptionAsAlt(on) => draft.input.option_as_alt = on,
            SettingsMsg::TouchScroll(on) => draft.input.touch_scroll = on,
            SettingsMsg::OnScreenKeyboard(on) => draft.keyboard.on_screen = on,
            _ => {}
        }
    }

    fn reset_section(&mut self) {
        let defaults = Config::default();
        match self.category {
            Category::Appearance => self.draft.appearance = defaults.appearance,
            Category::Terminal => {
                self.draft.terminal = defaults.terminal;
                self.scrollback_text = self.draft.terminal.scrollback_lines.to_string();
            }
            Category::Panels => {
                self.draft.panels = defaults.panels;
                self.draft.effects = defaults.effects;
            }
            Category::InputSound => {
                self.draft.sound = defaults.sound;
                self.draft.input = defaults.input;
                self.draft.keyboard = defaults.keyboard;
            }
            Category::Keys => self.reset_keys_to_defaults(),
        }
    }

    fn reset_keys_to_defaults(&mut self) {
        let defaults = Keymap::build(&[], cfg!(target_os = "macos"));
        for (action, keys) in &mut self.key_edits {
            *keys = defaults
                .shortcut_for(*action)
                .map(ToString::to_string)
                .unwrap_or_default();
        }
    }

    /// Validate and produce the candidate configuration, or record errors.
    pub fn candidate(&mut self) -> Option<Config> {
        let mut candidate = self.draft.clone();
        candidate.keybindings = self.keybinding_overrides();
        let mut errors: Vec<String> = validate(&candidate)
            .iter()
            .map(ToString::to_string)
            .collect();
        let keymap = Keymap::build(&candidate.keybindings, cfg!(target_os = "macos"));
        errors.extend(
            keymap
                .diagnostics
                .iter()
                .filter(|d| !d.contains("reserved"))
                .cloned(),
        );
        self.errors = errors;
        self.errors.is_empty().then_some(candidate)
    }

    /// Only shortcuts that differ from platform defaults are written.
    fn keybinding_overrides(&self) -> Vec<KeyBinding> {
        let defaults = Keymap::build(&[], cfg!(target_os = "macos"));
        self.key_edits
            .iter()
            .filter_map(|(action, keys)| {
                let default = defaults.shortcut_for(*action).cloned();
                let edited = Chord::parse(keys).ok();
                let unchanged = edited == default || (keys.trim().is_empty() && default.is_none());
                (!unchanged).then(|| KeyBinding {
                    action: action.info().id.to_owned(),
                    context: action.info().context.id().to_owned(),
                    keys: if keys.trim().is_empty() {
                        "none".into()
                    } else {
                        keys.trim().to_owned()
                    },
                })
            })
            .collect()
    }

    /// Whether a change needs new sessions to take effect.
    pub fn affects_new_sessions_only(&self) -> bool {
        self.draft.profiles != self.base.profiles
            || self.draft.terminal.scrollback_lines != self.base.terminal.scrollback_lines
    }

    fn matches(&self, label: &str) -> bool {
        self.query.is_empty() || label.to_lowercase().contains(&self.query.to_lowercase())
    }
}

impl std::fmt::Display for CursorShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Block => "Block",
            Self::Beam => "Beam",
            Self::Underline => "Underline",
        })
    }
}

impl std::fmt::Display for EffectsPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Off => "Off",
            Self::Subtle => "Subtle edge lighting",
        })
    }
}

/// Serialise only the fields where `draft` differs from `base`.
pub fn overrides_toml(base: &Config, draft: &Config) -> Result<String, String> {
    let base = toml::Value::try_from(base).map_err(|e| e.to_string())?;
    let draft = toml::Value::try_from(draft).map_err(|e| e.to_string())?;
    let mut diff =
        diff_tables(&base, &draft).unwrap_or_else(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(table) = &mut diff {
        table.insert(
            "schema_version".into(),
            toml::Value::Integer(crate::config::SCHEMA_VERSION.into()),
        );
    }
    toml::to_string_pretty(&diff).map_err(|e| e.to_string())
}

fn diff_tables(base: &toml::Value, draft: &toml::Value) -> Option<toml::Value> {
    match (base, draft) {
        (toml::Value::Table(base), toml::Value::Table(draft)) => {
            let table: toml::Table = draft
                .iter()
                .filter_map(|(key, value)| match base.get(key) {
                    Some(old) => diff_tables(old, value).map(|v| (key.clone(), v)),
                    None => Some((key.clone(), value.clone())),
                })
                .collect();
            (!table.is_empty()).then_some(toml::Value::Table(table))
        }
        (old, new) if old == new => None,
        (_, new) => Some(new.clone()),
    }
}

pub fn view<'a, M: Clone + 'a>(
    settings: &'a Settings,
    themes: &'a [Theme],
    wrap: impl Fn(SettingsMsg) -> M + Copy + 'a,
) -> Element<'a, M> {
    let categories = Category::ALL
        .iter()
        .fold(column![].spacing(4).width(180), |col, category| {
            let label = text(category.label()).size(14);
            let entry = button(label)
                .width(Length::Fill)
                .on_press(wrap(SettingsMsg::Category(*category)));
            col.push(if *category == settings.category {
                entry.style(button::primary)
            } else {
                entry.style(button::text)
            })
        });
    let search = text_input("Search settings", &settings.query)
        .on_input(move |q| wrap(SettingsMsg::Query(q)))
        .size(14);
    let body = if settings.query.is_empty() {
        section(settings, themes, settings.category, wrap)
    } else {
        Category::ALL
            .iter()
            .fold(column![].spacing(16), |col, c| {
                col.push(section(settings, themes, *c, wrap))
            })
            .into()
    };
    let mut footer = row![
        button(text("Reset section"))
            .style(button::secondary)
            .on_press(wrap(SettingsMsg::ResetSection)),
        button(text("Reset all"))
            .style(button::secondary)
            .on_press(wrap(SettingsMsg::ResetAll)),
        Space::new().width(Length::Fill),
        button(text("Cancel"))
            .style(button::secondary)
            .on_press(wrap(SettingsMsg::Cancel)),
        button(text("Apply"))
            .style(button::primary)
            .on_press(wrap(SettingsMsg::Apply)),
    ]
    .spacing(8);
    if settings.affects_new_sessions_only() {
        footer = footer.push(text("Some changes apply to new sessions only").size(12));
    }
    let errors = settings.errors.iter().fold(column![].spacing(2), |col, e| {
        col.push(text(e).size(12).style(text::danger))
    });
    column![
        text("Settings").size(20),
        row![
            categories,
            column![search, scrollable(body).height(Length::Fill)].spacing(10)
        ]
        .spacing(16)
        .height(Length::Fill),
        errors,
        footer,
    ]
    .spacing(12)
    .into()
}

fn section<'a, M: Clone + 'a>(
    settings: &'a Settings,
    themes: &'a [Theme],
    category: Category,
    wrap: impl Fn(SettingsMsg) -> M + Copy + 'a,
) -> Element<'a, M> {
    let rows: Vec<(&'static str, Element<'a, M>)> = match category {
        Category::Appearance => appearance_rows(settings, themes, wrap),
        Category::Terminal => terminal_rows(settings, wrap),
        Category::Panels => panel_rows(settings, wrap),
        Category::InputSound => input_sound_rows(settings, wrap),
        Category::Keys => key_rows(settings, wrap),
    };
    rows.into_iter()
        .filter(|(label, _)| settings.matches(label))
        .fold(
            column![text(category.label()).size(16)].spacing(10),
            |col, (label, control)| {
                col.push(
                    row![text(label).size(14).width(200), control]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                )
            },
        )
        .into()
}

type Rows<'a, M> = Vec<(&'static str, Element<'a, M>)>;

fn appearance_rows<'a, M: Clone + 'a>(
    settings: &'a Settings,
    themes: &'a [Theme],
    wrap: impl Fn(SettingsMsg) -> M + Copy + 'a,
) -> Rows<'a, M> {
    let a = &settings.draft.appearance;
    let ids: Vec<String> = themes.iter().map(|t| t.id.clone()).collect();
    vec![
        (
            "Theme",
            pick_list(ids, Some(a.theme.clone()), move |id| {
                wrap(SettingsMsg::Theme(id))
            })
            .into(),
        ),
        (
            "Font family",
            text_input("monospace", &a.font_family)
                .on_input(move |f| wrap(SettingsMsg::FontFamily(f)))
                .width(220)
                .into(),
        ),
        (
            "Font size",
            row![
                slider(8.0..=48.0, a.font_size, move |v| wrap(
                    SettingsMsg::FontSize(v)
                ))
                .width(180),
                text(format!("{:.0} px", a.font_size))
            ]
            .spacing(8)
            .into(),
        ),
        (
            "Line height",
            row![
                slider(1.0..=2.0, a.line_height, move |v| wrap(
                    SettingsMsg::LineHeight(v)
                ))
                .step(0.05_f32)
                .width(180),
                text(format!("{:.2}", a.line_height))
            ]
            .spacing(8)
            .into(),
        ),
        (
            "Reduced motion",
            checkbox(a.reduced_motion)
                .on_toggle(move |v| wrap(SettingsMsg::ReducedMotion(v)))
                .into(),
        ),
    ]
}

fn terminal_rows<'a, M: Clone + 'a>(
    settings: &'a Settings,
    wrap: impl Fn(SettingsMsg) -> M + Copy + 'a,
) -> Rows<'a, M> {
    let t = &settings.draft.terminal;
    let shapes = vec![
        CursorShape::Block,
        CursorShape::Beam,
        CursorShape::Underline,
    ];
    vec![
        (
            "Cursor shape",
            pick_list(shapes, Some(t.cursor_shape), move |s| {
                wrap(SettingsMsg::Cursor(s))
            })
            .into(),
        ),
        (
            "Cursor blink",
            checkbox(t.cursor_blink)
                .on_toggle(move |v| wrap(SettingsMsg::CursorBlink(v)))
                .into(),
        ),
        (
            "Scrollback lines",
            text_input("10000", &settings.scrollback_text)
                .on_input(move |v| wrap(SettingsMsg::Scrollback(v)))
                .width(120)
                .into(),
        ),
        (
            "Confirm multi-line paste",
            checkbox(t.confirm_multiline_paste)
                .on_toggle(move |v| wrap(SettingsMsg::ConfirmPaste(v)))
                .into(),
        ),
        (
            "Copy on select",
            checkbox(t.copy_on_select)
                .on_toggle(move |v| wrap(SettingsMsg::CopyOnSelect(v)))
                .into(),
        ),
    ]
}

fn panel_rows<'a, M: Clone + 'a>(
    settings: &'a Settings,
    wrap: impl Fn(SettingsMsg) -> M + Copy + 'a,
) -> Rows<'a, M> {
    let presets = vec![EffectsPreset::Off, EffectsPreset::Subtle];
    vec![
        (
            "System panel",
            checkbox(settings.draft.panels.metrics.enabled)
                .on_toggle(move |v| wrap(SettingsMsg::Metrics(v)))
                .into(),
        ),
        (
            "Visual effects",
            pick_list(presets, Some(settings.draft.effects.preset), move |p| {
                wrap(SettingsMsg::Effects(p))
            })
            .into(),
        ),
        toggle(
            "Process panel",
            settings.draft.panels.processes.enabled,
            move |v| wrap(SettingsMsg::Processes(v)),
        ),
        toggle(
            "Network panel",
            settings.draft.panels.network.enabled,
            move |v| wrap(SettingsMsg::Network(v)),
        ),
        toggle(
            "Show connections",
            settings.draft.panels.network.connections,
            move |v| wrap(SettingsMsg::Connections(v)),
        ),
        (
            "GeoIP database (.mmdb)",
            text_input(
                "Local file path; empty = off",
                &settings.draft.panels.network.geoip_database,
            )
            .on_input(move |p| wrap(SettingsMsg::GeoIpPath(p)))
            .width(280)
            .into(),
        ),
        toggle(
            "Directory panel",
            settings.draft.panels.files.enabled,
            move |v| wrap(SettingsMsg::Files(v)),
        ),
        toggle(
            "Show hidden files",
            settings.draft.panels.files.show_hidden,
            move |v| wrap(SettingsMsg::ShowHidden(v)),
        ),
    ]
}

fn input_sound_rows<'a, M: Clone + 'a>(
    settings: &'a Settings,
    wrap: impl Fn(SettingsMsg) -> M + Copy + 'a,
) -> Rows<'a, M> {
    let draft = &settings.draft;
    vec![
        toggle("Sound effects", draft.sound.enabled, move |v| {
            wrap(SettingsMsg::Sound(v))
        }),
        (
            "Volume",
            row![
                slider(0.0..=1.0, draft.sound.volume, move |v| wrap(
                    SettingsMsg::Volume(v)
                ))
                .step(0.05_f32)
                .width(180),
                text(format!("{:.0}%", draft.sound.volume * 100.0)),
            ]
            .spacing(8)
            .into(),
        ),
        toggle("Typing sounds", draft.sound.keypress, move |v| {
            wrap(SettingsMsg::TypingSound(v))
        }),
        toggle(
            "Option key as Alt (macOS)",
            draft.input.option_as_alt,
            move |v| wrap(SettingsMsg::OptionAsAlt(v)),
        ),
        toggle("Touch drag scrolls", draft.input.touch_scroll, move |v| {
            wrap(SettingsMsg::TouchScroll(v))
        }),
        toggle("On-screen keyboard", draft.keyboard.on_screen, move |v| {
            wrap(SettingsMsg::OnScreenKeyboard(v))
        }),
    ]
}

fn toggle<'a, M: Clone + 'a>(
    label: &'static str,
    value: bool,
    on_toggle: impl Fn(bool) -> M + 'a,
) -> (&'static str, Element<'a, M>) {
    (label, checkbox(value).on_toggle(on_toggle).into())
}

fn key_rows<'a, M: Clone + 'a>(
    settings: &'a Settings,
    wrap: impl Fn(SettingsMsg) -> M + Copy + 'a,
) -> Rows<'a, M> {
    ACTIONS
        .iter()
        .map(|info| {
            let keys = settings
                .key_edits
                .get(&info.action)
                .map_or("", String::as_str);
            let action = info.action;
            let input = text_input("unbound", keys)
                .on_input(move |k| wrap(SettingsMsg::Key(action, k)))
                .width(200);
            (info.title, container(input).into())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_contain_only_changes() {
        let base = Config::default();
        let mut draft = base.clone();
        draft.appearance.font_size = 18.0;
        let text = overrides_toml(&base, &draft).expect("diff");
        let value: toml::Table = toml::from_str(&text).expect("parse");
        assert_eq!(value["schema_version"].as_integer(), Some(1));
        assert_eq!(value["appearance"]["font_size"].as_float(), Some(18.0));
        assert!(value["appearance"].get("theme").is_none());
        assert!(value.get("terminal").is_none());
        crate::config::parse_config(&text, std::path::Path::new("ui-overrides.toml"))
            .expect("valid");
    }

    #[test]
    fn candidate_validates_and_reports_errors() {
        let config = Config::default();
        let mut settings = Settings::open(&config, &Keymap::build(&[], false));
        settings.update(SettingsMsg::FontSize(100.0));
        assert!(settings.candidate().is_none());
        assert!(settings.errors.iter().any(|e| e.contains("font_size")));
        settings.update(SettingsMsg::FontSize(16.0));
        assert!(settings.candidate().is_some());
    }

    #[test]
    fn keybinding_edits_become_minimal_overrides() {
        let config = Config::default();
        let mut settings = Settings::open(&config, &Keymap::build(&[], cfg!(target_os = "macos")));
        assert!(settings.candidate().expect("valid").keybindings.is_empty());
        settings.update(SettingsMsg::Key(Action::ZoomPane, String::new()));
        settings.update(SettingsMsg::Key(Action::ResetLayout, "Ctrl+Alt+R".into()));
        let candidate = settings.candidate().expect("valid");
        let ids: Vec<_> = candidate
            .keybindings
            .iter()
            .map(|k| (k.action.as_str(), k.keys.as_str()))
            .collect();
        assert!(ids.contains(&("pane.zoom", "none")));
        assert!(ids.contains(&("layout.reset", "Ctrl+Alt+R")));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn conflicting_shortcut_blocks_apply() {
        let config = Config::default();
        let mut settings = Settings::open(&config, &Keymap::build(&[], false));
        settings.update(SettingsMsg::Key(Action::ResetLayout, "Ctrl+Shift+T".into()));
        assert!(settings.candidate().is_none());
        assert!(settings.errors.iter().any(|e| e.contains("bound to both")));
    }

    #[test]
    fn reset_section_restores_defaults() {
        let config = Config::default();
        let mut settings = Settings::open(&config, &Keymap::build(&[], false));
        settings.update(SettingsMsg::FontSize(20.0));
        settings.update(SettingsMsg::ResetSection);
        assert_eq!(settings.draft.appearance, Config::default().appearance);
    }
}
