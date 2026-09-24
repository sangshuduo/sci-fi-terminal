//! Application state, messages and the update loop.
//!
//! The GUI thread only routes input, applies small state changes, lays out
//! and draws. Sessions, metrics and config writes never block it: sessions
//! wake the UI through a coalesced channel and the UI reads published state.

use std::path::PathBuf;
use std::sync::Arc;

use iced::futures::channel::mpsc::{UnboundedSender, unbounded};
use iced::widget::pane_grid::{self, Pane};
use iced::{Event, Font, Subscription, Task, event, keyboard, window};
use terminal_core::SessionId;

use super::actions::{Action, Keymap};
use super::input::KeyPress;
use super::palette::{Palette, PaletteMsg};
use super::settings::{Settings, SettingsMsg};
use super::state::Tab;
use crate::config::{
    Config, ConfigPaths, Theme, builtin_themes, find_theme, load_effective, load_user_themes,
};
use crate::osk::{Keyboard, Layout, OskMessage, builtin_layout, load_user_layouts};
use crate::panels::{BuiltinPanel, MonitorSample, MonitorWorker, PanelMsg, PanelRegistry};
use crate::render::globe::{GlobeState, Marker, markers_from};
use crate::render::{CellMetrics, TerminalEvent};
use crate::session::Notify;
use crate::sound::{Cue, SoundPlayer, SoundSettings};

/// Command-line options that affect startup.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub config_dir: Option<PathBuf>,
    pub safe_mode: bool,
}

/// Events from worker threads, delivered through one unbounded but
/// coalesced channel (at most one pending wake per session).
#[derive(Debug, Clone)]
pub enum AppEvent {
    Wake(SessionId),
    Monitor(Box<MonitorSample>),
}

struct EventNotify(UnboundedSender<AppEvent>);

impl Notify for EventNotify {
    fn wake(&self, session: SessionId) {
        let _ = self.0.unbounded_send(AppEvent::Wake(session));
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Event(AppEvent),
    Key(KeyPress),
    Terminal(Pane, TerminalEvent),
    PaneClicked(Pane),
    PaneResized(pane_grid::ResizeEvent),
    PaneDragged(pane_grid::DragEvent),
    SelectTab(usize),
    CloseTab(usize),
    Run(Action),
    Panel(PanelMsg),
    Osk(OskMessage),
    Palette(PaletteMsg),
    Settings(SettingsMsg),
    SearchQuery(String),
    SearchStep(bool),
    SearchClose,
    Clipboard(Option<String>),
    PasteDecision(bool),
    CloseDecision(bool),
    WindowFocus(bool),
    CloseRequested(window::Id),
    DismissNotice,
    GlobeTick(std::time::Instant),
}

/// A pending confirmation shown as a modal.
pub enum Confirm {
    Paste { pane: Pane, text: String },
    ClosePane(Pane),
    CloseWindow,
}

pub struct SearchBar {
    pub query: String,
    pub index: usize,
}

pub struct App {
    pub(super) paths: Option<ConfigPaths>,
    pub(super) options: Options,
    pub(super) config: Config,
    pub(super) themes: Vec<Theme>,
    pub(super) theme: Theme,
    pub(super) iced_theme: iced::Theme,
    pub(super) keymap: Keymap,
    pub(super) font: Font,
    pub(super) cell: CellMetrics,
    pub(super) tabs: Vec<Tab>,
    pub(super) active: usize,
    pub(super) next_slot: u32,
    pub(super) events: UnboundedSender<AppEvent>,
    pub(super) notify: Arc<dyn Notify>,
    pub(super) panels: PanelRegistry,
    /// Panels currently shown; each is toggled independently.
    pub(super) shown: std::collections::HashSet<BuiltinPanel>,
    pub(super) monitor_worker: Option<MonitorWorker>,
    pub(super) monitor: MonitorSample,
    pub(super) sound: SoundPlayer,
    pub(super) layouts: Vec<Layout>,
    pub(super) keyboard: Option<Keyboard>,
    pub(super) followed_pid: Option<u32>,
    pub(super) globe: GlobeState,
    pub(super) globe_markers: Vec<Marker>,
    pub(super) last_globe_tick: Option<std::time::Instant>,
    pub(super) palette: Option<Palette>,
    pub(super) settings: Option<Settings>,
    pub(super) search: Option<SearchBar>,
    pub(super) confirm: Option<Confirm>,
    pub(super) notices: Vec<String>,
    pub(super) window_focused: bool,
}

impl App {
    pub fn boot(options: Options) -> (Self, Task<Message>) {
        let (sender, receiver) = unbounded();
        let paths = options
            .config_dir
            .clone()
            .map(ConfigPaths::from_dir)
            .or_else(ConfigPaths::platform_default);
        let mut notices = Vec::new();
        let config = match &paths {
            Some(paths) => {
                let loaded = load_effective(paths, options.safe_mode);
                notices.extend(loaded.diagnostics.iter().map(ToString::to_string));
                loaded.config
            }
            None => {
                notices.push("No configuration directory is available; using defaults.".into());
                Config::default()
            }
        };
        let themes = collect_themes(paths.as_ref(), options.safe_mode, &mut notices);
        let theme = resolve_theme(&themes, &config.appearance.theme, &mut notices);
        let keymap = Keymap::build(&config.keybindings, cfg!(target_os = "macos"));
        notices.extend(keymap.diagnostics.iter().cloned());
        let font = font_for(&config.appearance.font_family);
        let cell = CellMetrics::measure(
            font,
            config.appearance.font_size,
            config.appearance.line_height,
        );
        let shown = initially_shown(&config);
        let layouts = collect_layouts(paths.as_ref(), options.safe_mode, &mut notices);
        let keyboard = config
            .keyboard
            .on_screen
            .then(|| keyboard_for(&layouts, &config.keyboard.layout));
        let sound = SoundPlayer::new(sound_settings(&config));
        let mut app = Self {
            paths,
            options,
            iced_theme: super::style::iced_theme(&theme),
            config,
            themes,
            theme,
            keymap,
            font,
            cell,
            tabs: Vec::new(),
            active: 0,
            next_slot: 1,
            notify: Arc::new(EventNotify(sender.clone())),
            events: sender,
            panels: PanelRegistry::builtin(),
            shown,
            monitor_worker: None,
            monitor: MonitorSample::default(),
            sound,
            layouts,
            keyboard,
            followed_pid: None,
            globe: GlobeState::default(),
            globe_markers: Vec::new(),
            last_globe_tick: None,
            palette: None,
            settings: None,
            search: None,
            confirm: None,
            notices,
            window_focused: true,
        };
        app.restore_layout();
        app.sync_metrics_worker();
        (app, Task::run(receiver, Message::Event))
    }

    pub fn title(&self) -> String {
        match self.tabs.get(self.active) {
            Some(tab) => format!("{} — sci-fi-terminal", tab.title()),
            None => "sci-fi-terminal".into(),
        }
    }

    pub fn theme(&self) -> iced::Theme {
        self.iced_theme.clone()
    }

    /// Keyboard and window events, plus a ≤ 30 Hz tick only while the globe
    /// is visibly rotating. Terminal redraws are driven by damage only.
    pub fn subscription(&self) -> Subscription<Message> {
        let globe = if self.globe_rotating() || self.home_flashing() {
            iced::time::every(GLOBE_FRAME).map(Message::GlobeTick)
        } else {
            Subscription::none()
        };
        Subscription::batch([Self::input_events(), globe])
    }

    pub(super) fn is_shown(&self, panel: BuiltinPanel) -> bool {
        self.shown.contains(&panel)
    }

    /// The globe is on screen and motion is allowed.
    fn globe_can_animate(&self) -> bool {
        self.is_shown(BuiltinPanel::Network)
            && self.window_focused
            && self.config.panels.network.globe
            && !self.config.appearance.reduced_motion
    }

    /// Whether the globe should rotate right now.
    pub(super) fn globe_rotating(&self) -> bool {
        self.globe_can_animate() && self.config.panels.network.globe_rotate
    }

    /// Whether the home spot should flash right now.
    pub(super) fn home_flashing(&self) -> bool {
        self.globe_can_animate() && self.globe_home().is_some()
    }

    /// Located home position from the opt-in public IP lookup.
    pub(super) fn globe_home(&self) -> Option<Marker> {
        let location = self.monitor.home.as_ref()?.location.as_ref()?;
        Some(Marker {
            latitude: location.latitude? as f32,
            longitude: location.longitude? as f32,
        })
    }

    fn input_events() -> Subscription<Message> {
        event::listen_with(|event, status, id| match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) if status == event::Status::Ignored => Some(Message::Key(KeyPress {
                key,
                modifiers,
                text: text.map(|t| t.to_string()),
            })),
            Event::Window(window::Event::Focused) => Some(Message::WindowFocus(true)),
            Event::Window(window::Event::Unfocused) => Some(Message::WindowFocus(false)),
            Event::Window(window::Event::CloseRequested) => Some(Message::CloseRequested(id)),
            _ => None,
        })
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.dispatch(message);
        // The directory viewer follows the focused shell; retarget on any focus change.
        let pid = self.focused_pid();
        if pid != self.followed_pid {
            self.followed_pid = pid;
            self.sync_metrics_worker();
        }
        task
    }

    pub(super) fn focused_pid(&self) -> Option<u32> {
        self.active_tab()
            .and_then(Tab::focused)
            .and_then(|pane| pane.session.as_ref())
            .and_then(|session| session.pid())
    }

    fn dispatch(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Event(AppEvent::Wake(id)) => self.on_wake(id),
            Message::Event(AppEvent::Monitor(sample)) => {
                self.monitor = *sample;
                self.refresh_globe_markers();
                Task::none()
            }
            Message::GlobeTick(now) => {
                let elapsed = self
                    .last_globe_tick
                    .map_or(0.0, |last| (now - last).as_secs_f32());
                self.last_globe_tick = Some(now);
                if self.globe_rotating() {
                    self.globe.advance(elapsed);
                }
                if self.home_flashing() {
                    self.globe.pulse(elapsed);
                } else {
                    self.globe.hold_pulse();
                }
                Task::none()
            }
            Message::Panel(PanelMsg::Files(crate::panels::FilesMsg::InsertCd(path))) => {
                self.insert_cd(&path);
                Task::none()
            }
            Message::Osk(message) => self.on_osk(message),
            Message::Key(press) => self.on_key(press),
            Message::Terminal(pane, event) => self.on_terminal(pane, event),
            Message::PaneClicked(pane) => {
                self.focus_pane(pane);
                Task::none()
            }
            Message::PaneResized(resize) => {
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.panes
                        .resize(resize.split, super::layout::clamp_ratio(resize.ratio));
                }
                Task::none()
            }
            Message::PaneDragged(drag) => {
                self.on_drag(drag);
                Task::none()
            }
            Message::SelectTab(index) => self.select_tab(index),
            Message::CloseTab(index) => self.close_tab(index),
            Message::Run(action) => self.run(action),
            Message::Palette(msg) => self.on_palette(msg),
            Message::Settings(msg) => self.on_settings(msg),
            Message::SearchQuery(query) => self.on_search_query(query),
            Message::SearchStep(forward) => self.search_step(forward),
            Message::SearchClose => {
                self.search = None;
                self.for_focused(|pane| {
                    pane.send(crate::session::SessionCommand::Search(String::new()))
                });
                Task::none()
            }
            Message::Clipboard(text) => self.on_clipboard(text),
            Message::PasteDecision(accept) => self.on_paste_decision(accept),
            Message::CloseDecision(accept) => self.on_close_decision(accept),
            Message::WindowFocus(focused) => {
                self.window_focused = focused;
                self.sync_metrics_worker();
                Task::none()
            }
            Message::CloseRequested(_) => self.request_window_close(),
            Message::DismissNotice => {
                self.notices.clear();
                Task::none()
            }
        }
    }

    pub(super) fn on_palette(&mut self, msg: PaletteMsg) -> Task<Message> {
        let Some(palette) = self.palette.as_mut() else {
            return Task::none();
        };
        let entry = match msg {
            PaletteMsg::Query(query) => {
                palette.query = query;
                palette.selected = 0;
                return Task::none();
            }
            PaletteMsg::Submit => {
                let found = super::palette::entries(&palette.query, &self.themes);
                found
                    .get(palette.selected.min(found.len().saturating_sub(1)))
                    .map(|(e, _)| e.clone())
            }
            PaletteMsg::Pick(entry) => Some(entry),
        };
        self.palette = None;
        match entry {
            Some(super::palette::Entry::Action(action)) => self.run(action),
            Some(super::palette::Entry::Theme(id)) => {
                let mut config = self.config.clone();
                config.appearance.theme = id;
                self.apply_config(config);
                Task::none()
            }
            None => Task::none(),
        }
    }

    pub(super) fn on_settings(&mut self, msg: SettingsMsg) -> Task<Message> {
        let Some(settings) = self.settings.as_mut() else {
            return Task::none();
        };
        match msg {
            SettingsMsg::Cancel => {
                self.settings = None;
                // Drop the live preview.
                let config = self.config.clone();
                self.apply_appearance(&config);
            }
            SettingsMsg::Apply => {
                if let Some(candidate) = settings.candidate() {
                    self.settings = None;
                    self.persist_overrides(&candidate);
                    self.apply_config(candidate);
                }
            }
            other => {
                let preview = matches!(
                    other,
                    SettingsMsg::Theme(_)
                        | SettingsMsg::FontSize(_)
                        | SettingsMsg::LineHeight(_)
                        | SettingsMsg::FontFamily(_)
                );
                settings.update(other);
                if preview {
                    let draft = settings.draft.clone();
                    self.apply_appearance(&draft);
                }
            }
        }
        Task::none()
    }

    pub(super) fn focus_pane(&mut self, pane: Pane) {
        if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.panes.get(pane).is_some()
            && tab.focus != pane
        {
            if let Some(old) = tab.focused_mut() {
                old.cache.clear();
            }
            tab.focus = pane;
            if let Some(new) = tab.focused_mut() {
                new.cache.clear();
            }
        }
    }

    fn on_drag(&mut self, drag: pane_grid::DragEvent) {
        if let (pane_grid::DragEvent::Dropped { pane, target }, Some(tab)) =
            (drag, self.tabs.get_mut(self.active))
        {
            tab.panes.drop(pane, target);
        }
    }

    pub(super) fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub(super) fn for_focused(&mut self, f: impl FnOnce(&mut super::state::PaneState)) {
        if let Some(pane) = self.tabs.get_mut(self.active).and_then(Tab::focused_mut) {
            f(pane);
        }
    }

    pub(super) fn palette_task(&self) -> Task<Message> {
        iced::widget::operation::focus(Palette::input_id())
    }
}

/// Built-in on-screen keyboard layout plus user layouts from `keyboards/`.
fn collect_layouts(
    paths: Option<&ConfigPaths>,
    safe_mode: bool,
    notices: &mut Vec<String>,
) -> Vec<Layout> {
    let mut layouts = vec![builtin_layout()];
    if let (Some(paths), false) = (paths, safe_mode) {
        let (user, errors) = load_user_layouts(&paths.config_dir.join("keyboards"));
        notices.extend(errors.iter().map(ToString::to_string));
        layouts.extend(user.into_iter().filter(|l| l.id != "en-us"));
    }
    layouts
}

/// Panels shown at startup: each panel's `enabled` setting.
pub(super) fn initially_shown(config: &Config) -> std::collections::HashSet<BuiltinPanel> {
    let panels = &config.panels;
    [
        (BuiltinPanel::System, panels.metrics.enabled),
        (BuiltinPanel::Network, panels.network.enabled),
        (BuiltinPanel::Directory, panels.files.enabled),
    ]
    .into_iter()
    .filter_map(|(panel, on)| on.then_some(panel))
    .collect()
}

/// UI chrome font from the theme's `[style] ui_font`; empty means the default.
pub(super) fn font_for_ui(family: &str) -> Font {
    if family.trim().is_empty() {
        Font::DEFAULT
    } else {
        font_for(family)
    }
}

pub(super) fn keyboard_for(layouts: &[Layout], id: &str) -> Keyboard {
    let layout = layouts
        .iter()
        .find(|l| l.id == id)
        .cloned()
        .unwrap_or_else(builtin_layout);
    Keyboard::new(layout)
}

pub(super) fn sound_settings(config: &Config) -> SoundSettings {
    SoundSettings {
        enabled: config.sound.enabled,
        volume: config.sound.volume,
        keypress: config.sound.keypress,
    }
}

/// Globe frame interval: 20 Hz, under the spec's 30 Hz ceiling for optional
/// animation; at 6°/s this is 0.3° per frame, visually smooth.
const GLOBE_FRAME: std::time::Duration = std::time::Duration::from_millis(50);

impl App {
    /// Rebuild peer markers from the latest located connections. When the
    /// globe is not rotating, turn it to face the first peer.
    pub(super) fn refresh_globe_markers(&mut self) {
        let located = match &self.monitor.connections {
            Some(Ok(list)) => list
                .iter()
                .filter_map(|view| view.location.as_ref())
                .filter_map(|loc| Some((loc.latitude?, loc.longitude?)))
                .collect(),
            _ => Vec::new(),
        };
        let markers = markers_from(located.into_iter());
        if markers != self.globe_markers {
            self.globe_markers = markers;
            self.globe.cache.clear();
        }
        if !self.home_flashing() {
            self.globe.hold_pulse();
        }
        if !self.globe_rotating() {
            self.last_globe_tick = None;
            // A still globe faces home when known, otherwise the first peer.
            let target = self
                .globe_home()
                .or_else(|| self.globe_markers.first().copied());
            self.globe.face(target);
        }
    }

    /// Play a cue if sound is enabled; never blocks.
    pub(super) fn play(&mut self, cue: Cue) {
        self.sound.play(cue);
    }
}

fn collect_themes(
    paths: Option<&ConfigPaths>,
    safe_mode: bool,
    notices: &mut Vec<String>,
) -> Vec<Theme> {
    let mut themes = builtin_themes();
    if let (Some(paths), false) = (paths, safe_mode) {
        let (user, errors) = load_user_themes(&paths.themes_dir);
        notices.extend(errors.iter().map(ToString::to_string));
        for theme in user {
            if find_theme(&themes, &theme.id).is_some() {
                notices.push(format!(
                    "theme `{}` duplicates a built-in id and was ignored",
                    theme.id
                ));
            } else {
                themes.push(theme);
            }
        }
    }
    themes
}

pub(super) fn resolve_theme(themes: &[Theme], id: &str, notices: &mut Vec<String>) -> Theme {
    find_theme(themes, id).cloned().unwrap_or_else(|| {
        notices.push(format!("theme `{id}` not found; using graphite"));
        builtin_themes()
            .into_iter()
            .next()
            .unwrap_or_else(|| themes[0].clone())
    })
}

/// Resolve the configured family. Names are leaked once per distinct family
/// because Iced fonts require `'static` names; the set is bounded by settings edits.
pub(super) fn font_for(family: &str) -> Font {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let family = family.trim();
    if family.is_empty() || family.eq_ignore_ascii_case("monospace") {
        return Font::MONOSPACE;
    }
    let names = NAMES.get_or_init(Default::default);
    let Ok(mut names) = names.lock() else {
        return Font::MONOSPACE;
    };
    let name: &'static str = names
        .entry(family.to_owned())
        .or_insert_with(|| Box::leak(family.to_owned().into_boxed_str()));
    Font::with_name(name)
}
