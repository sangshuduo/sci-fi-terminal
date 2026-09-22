//! Action handlers: sessions, panes, tabs and configuration changes.

use std::time::Duration;

use iced::Task;
use iced::widget::pane_grid::{self, Axis, Pane};
use platform::{NativePty, resolve_profile};
use terminal_core::{Limits, SessionId, TermSize};

use super::actions::{Action, Keymap};
use super::app::{App, AppEvent, Confirm, Message, SearchBar, font_for, resolve_theme};
use super::layout::{self, LayoutFile, MAX_PANES_PER_TAB};
use super::palette::Palette;
use super::settings::{Settings, overrides_toml};
use super::state::{PaneState, Tab, splits};
use crate::config::{Config, DEFAULT_PROFILE_ID, LayoutPreset, write_atomic};
use crate::panels::metrics::BACKGROUND_INTERVAL;
use crate::panels::{MonitorPlan, MonitorSample, MonitorWorker};
use crate::render::CellMetrics;
use crate::session::{SessionCommand, SessionConfig, spawn_session};
use crate::sound::Cue;

/// Initial window size in logical pixels; `main.rs` opens the window at this size.
pub const INITIAL_WINDOW: (f32, f32) = (1100.0, 700.0);

impl App {
    pub(super) fn session_count(&self) -> usize {
        self.tabs.iter().map(|tab| tab.panes.len()).sum()
    }

    fn limits(&self) -> Limits {
        Limits {
            scrollback_lines: self.config.terminal.scrollback_lines as usize,
            history_bytes: self.config.terminal.scrollback_max_mib as usize * 1024 * 1024,
            ..Limits::default()
        }
    }

    /// Create a pane with a fresh session. Failures are kept and shown in the pane.
    pub(super) fn spawn_pane(&mut self, profile_id: &str) -> PaneState {
        let session = self.start_session(profile_id);
        self.play(if session.is_ok() {
            Cue::SessionStart
        } else {
            Cue::Error
        });
        PaneState::new(profile_id.to_owned(), session)
    }

    /// Best estimate of a new pane's grid, so shell startup output is laid out
    /// at (nearly) the final width. The exact size follows on first layout.
    fn initial_size(&self) -> TermSize {
        let focused = self
            .active_tab()
            .and_then(Tab::focused)
            .and_then(|p| p.pixel_size);
        let panel = if self.panel_visible {
            super::view::PANEL_WIDTH + 8.0
        } else {
            0.0
        };
        let (width, height) = focused.map_or(
            (INITIAL_WINDOW.0 - panel - 16.0, INITIAL_WINDOW.1 - 80.0),
            |size| (size.width, size.height),
        );
        let (columns, rows) = self.cell.grid_for(
            width - 2.0 * crate::render::PADDING,
            height - 2.0 * crate::render::PADDING,
        );
        TermSize::new(columns, rows).with_cell_pixels(
            self.cell.width.round() as u16,
            self.cell.height.round() as u16,
        )
    }

    fn start_session(&mut self, profile_id: &str) -> Result<crate::session::SessionHandle, String> {
        let profile = self
            .config
            .profiles
            .get(profile_id)
            .or_else(|| self.config.profiles.get(DEFAULT_PROFILE_ID))
            .cloned()
            .unwrap_or_default();
        let env: Vec<(String, String)> = profile.env.into_iter().collect();
        let cwd = profile.cwd.as_deref().map(std::path::Path::new);
        let shell = resolve_profile(
            &profile.executable,
            &profile.args,
            profile.login_shell,
            cwd,
            &env,
        )
        .map_err(|err| err.to_string())?;
        let id = SessionId::new(self.next_slot, 1);
        self.next_slot += 1;
        let config = SessionConfig {
            id,
            shell,
            size: self.initial_size(),
            limits: self.limits(),
        };
        let handle = spawn_session(&NativePty, config, self.notify.clone())
            .map_err(|err| err.to_string())?;
        let _ = handle.send(SessionCommand::SetQueryPalette(Box::new(query_palette(
            &self.theme,
        ))));
        Ok(handle)
    }

    fn can_add_session(&mut self) -> bool {
        let max = usize::from(self.config.layout.max_sessions);
        if self.session_count() >= max {
            self.notices.push(format!(
                "Session limit reached ({max}). Close a pane first."
            ));
            return false;
        }
        true
    }

    pub(super) fn restore_layout(&mut self) {
        let max = usize::from(self.config.layout.max_sessions);
        let saved = match (
            &self.paths,
            self.config.layout.restore && !self.options.safe_mode,
        ) {
            (Some(paths), true) => {
                LayoutFile::load(&paths.layout_file, max).unwrap_or_else(|err| {
                    self.notices.push(format!("Saved layout ignored: {err}"));
                    None
                })
            }
            _ => None,
        };
        let layout = saved.unwrap_or_else(|| match self.config.layout.preset {
            LayoutPreset::Focus => LayoutFile::focus(DEFAULT_PROFILE_ID),
            LayoutPreset::Split => LayoutFile::split(DEFAULT_PROFILE_ID),
        });
        for tab in &layout.tabs {
            let config = layout::restore(&tab.root, &mut |profile: &str| self.spawn_pane(profile));
            if let Some(tab) = Tab::from_state(pane_grid::State::with_configuration(config)) {
                self.tabs.push(tab);
            }
        }
        self.active = layout.active_tab.min(self.tabs.len().saturating_sub(1));
    }

    fn save_layout(&mut self) {
        let Some(paths) = &self.paths else { return };
        if self.options.safe_mode || !self.config.layout.restore || self.tabs.is_empty() {
            return;
        }
        let file = LayoutFile {
            schema_version: layout::LAYOUT_VERSION,
            active_tab: self.active,
            tabs: self
                .tabs
                .iter()
                .map(|tab| layout::TabLayout {
                    root: layout::capture(&tab.panes, |p| p.profile.clone()),
                })
                .collect(),
        };
        if let Err(err) = file.save(&paths.layout_file) {
            self.notices.push(err.to_string());
        }
    }

    pub(super) fn run(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::NewTab => self.new_tab(),
            Action::ClosePane => return self.request_close_focused(),
            Action::SplitRight => self.split(Axis::Vertical),
            Action::SplitDown => self.split(Axis::Horizontal),
            Action::FocusNext | Action::FocusPrevious => {
                let forward = action == Action::FocusNext;
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.cycle_focus(forward);
                    tab.panes
                        .iter_mut()
                        .for_each(|(_, pane)| pane.cache.clear());
                }
            }
            Action::ZoomPane => self.toggle_zoom(),
            Action::NextTab => return self.select_tab((self.active + 1) % self.tabs.len().max(1)),
            Action::PreviousTab => {
                let len = self.tabs.len().max(1);
                return self.select_tab((self.active + len - 1) % len);
            }
            Action::Copy => self.for_focused(|pane| pane.send(SessionCommand::CopySelection)),
            Action::Paste => return iced::clipboard::read().map(Message::Clipboard),
            Action::Search => {
                self.search = Some(SearchBar {
                    query: String::new(),
                    index: 0,
                });
                return iced::widget::operation::focus(super::view::SEARCH_ID);
            }
            Action::ScrollPageUp | Action::ScrollPageDown => {
                self.scroll_page(action == Action::ScrollPageUp)
            }
            Action::TogglePalette => {
                self.palette = if self.palette.is_some() {
                    None
                } else {
                    Some(Palette::default())
                };
                return self.palette_task();
            }
            Action::OpenSettings => {
                self.settings = Some(Settings::open(&self.config, &self.keymap))
            }
            Action::ToggleMetrics => {
                self.panel_visible = !self.panel_visible;
                self.sync_metrics_worker();
                self.play(if self.panel_visible {
                    Cue::PanelOpen
                } else {
                    Cue::PanelClose
                });
            }
            Action::ResetLayout => self.reset_layout(),
            Action::FontIncrease => self.adjust_font(1.0),
            Action::FontDecrease => self.adjust_font(-1.0),
            Action::FontReset => self.adjust_font(0.0),
            Action::ToggleKeyboard => {
                self.keyboard = match self.keyboard {
                    Some(_) => None,
                    None => Some(super::app::keyboard_for(
                        &self.layouts,
                        &self.config.keyboard.layout,
                    )),
                };
                let cue = if self.keyboard.is_some() {
                    Cue::PanelOpen
                } else {
                    Cue::PanelClose
                };
                self.play(cue);
            }
            Action::ToggleSound => {
                let mut config = self.config.clone();
                config.sound.enabled = !config.sound.enabled;
                self.apply_config(config);
                self.play(Cue::PanelOpen);
            }
        }
        Task::none()
    }

    fn new_tab(&mut self) {
        if !self.can_add_session() {
            return;
        }
        let pane = self.spawn_pane(DEFAULT_PROFILE_ID);
        self.tabs.push(Tab::new(pane));
        self.active = self.tabs.len() - 1;
    }

    fn split(&mut self, axis: Axis) {
        let panes_here = self.tabs.get(self.active).map_or(0, |tab| tab.panes.len());
        if panes_here >= MAX_PANES_PER_TAB {
            self.notices.push(format!(
                "A tab shows at most {MAX_PANES_PER_TAB} panes; open a new tab instead."
            ));
            return;
        }
        if !self.can_add_session() {
            return;
        }
        let profile = self
            .tabs
            .get(self.active)
            .and_then(Tab::focused)
            .map_or_else(
                || DEFAULT_PROFILE_ID.to_owned(),
                |pane| pane.profile.clone(),
            );
        let state = self.spawn_pane(&profile);
        if let Some(tab) = self.tabs.get_mut(self.active)
            && let Some((pane, _)) = tab.panes.split(axis, tab.focus, state)
        {
            tab.focus = pane;
        }
    }

    fn toggle_zoom(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if tab.panes.maximized().is_some() {
                tab.panes.restore();
            } else {
                tab.panes.maximize(tab.focus);
            }
        }
    }

    fn reset_layout(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.panes.restore();
            for split in splits(&tab.panes) {
                tab.panes.resize(split, 0.5);
            }
        }
    }

    fn scroll_page(&mut self, up: bool) {
        self.for_focused(|pane| {
            let rows = pane
                .published
                .snapshot
                .as_ref()
                .map_or(24, |s| i32::from(s.rows));
            pane.send(SessionCommand::Scroll(if up { rows } else { -rows }));
        });
    }

    pub(super) fn select_tab(&mut self, index: usize) -> Task<Message> {
        if index < self.tabs.len() {
            self.active = index;
            if let Some(tab) = self.tabs.get_mut(index) {
                // Re-check sizes: panes in hidden tabs may have missed resizes.
                tab.panes.iter_mut().for_each(|(_, pane)| {
                    pane.pixel_size = None;
                    pane.cache.clear();
                });
            }
        }
        Task::none()
    }

    pub(super) fn close_tab(&mut self, index: usize) -> Task<Message> {
        if index >= self.tabs.len() {
            return Task::none();
        }
        let tab = self.tabs.remove(index);
        drop(tab); // Dropping handles requests teardown on session threads.
        if self.tabs.is_empty() {
            return self.exit();
        }
        self.active = self.active.min(self.tabs.len() - 1);
        Task::none()
    }

    fn request_close_focused(&mut self) -> Task<Message> {
        let Some(tab) = self.tabs.get(self.active) else {
            return Task::none();
        };
        let pane = tab.focus;
        if tab.focused().is_some_and(PaneState::is_live) {
            self.confirm = Some(Confirm::ClosePane(pane));
            return Task::none();
        }
        self.close_pane(pane)
    }

    pub(super) fn close_pane(&mut self, pane: Pane) -> Task<Message> {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return Task::none();
        };
        if tab.panes.len() == 1 {
            return self.close_tab(self.active);
        }
        if let Some((_state, sibling)) = tab.panes.close(pane) {
            tab.focus = sibling;
        }
        Task::none()
    }

    pub(super) fn request_window_close(&mut self) -> Task<Message> {
        let live = self
            .tabs
            .iter()
            .flat_map(|t| t.panes.iter())
            .filter(|(_, p)| p.is_live())
            .count();
        if live > 0 {
            self.confirm = Some(Confirm::CloseWindow);
            return Task::none();
        }
        self.exit()
    }

    pub(super) fn on_close_decision(&mut self, accept: bool) -> Task<Message> {
        let confirm = self.confirm.take();
        if !accept {
            return Task::none();
        }
        match confirm {
            Some(Confirm::ClosePane(pane)) => self.close_pane(pane),
            Some(Confirm::CloseWindow) => self.exit(),
            Some(paste @ Confirm::Paste { .. }) => {
                self.confirm = Some(paste);
                self.on_paste_decision(true)
            }
            None => Task::none(),
        }
    }

    /// Save layout, request teardown of every session and exit. Never joins workers.
    pub(super) fn exit(&mut self) -> Task<Message> {
        self.save_layout();
        self.monitor_worker = None;
        self.tabs.clear();
        iced::exit()
    }

    /// What the visible panels need sampled; everything else stays off.
    fn monitor_plan(&self) -> MonitorPlan {
        let panels = &self.config.panels;
        let visible = self.panel_visible;
        let focused = Duration::from_millis(panels.metrics.interval_ms);
        let interval = if self.window_focused {
            focused
        } else {
            BACKGROUND_INTERVAL.max(focused)
        };
        let network_every = Duration::from_millis(panels.network.interval_ms).max(interval);
        let geoip = panels.network.geoip_database.trim();
        let files_pid = self.focused_pid();
        MonitorPlan {
            interval,
            system: visible && panels.metrics.enabled,
            processes: (visible && panels.processes.enabled)
                .then_some(usize::from(panels.processes.count)),
            network: (visible && panels.network.enabled).then_some(network_every),
            connections: panels.network.connections,
            geoip_database: (!geoip.is_empty()).then(|| std::path::PathBuf::from(geoip)),
            files_pid: if visible && panels.files.enabled {
                files_pid
            } else {
                None
            },
            show_hidden: panels.files.show_hidden,
        }
    }

    /// Start, retarget or stop the monitor worker to match what is visible.
    pub(super) fn sync_metrics_worker(&mut self) {
        let plan = self.monitor_plan();
        if plan.is_idle() {
            self.monitor_worker = None;
            self.monitor = MonitorSample::default();
            return;
        }
        match &self.monitor_worker {
            Some(worker) => worker.set_plan(plan),
            None => {
                let events = self.events.clone();
                let sink = std::sync::Arc::new(move |sample| {
                    let _ = events.unbounded_send(AppEvent::Monitor(Box::new(sample)));
                });
                self.monitor_worker = MonitorWorker::start(plan, sink);
            }
        }
    }

    /// Type a quoted `cd` into the focused terminal without pressing Enter.
    pub(super) fn insert_cd(&mut self, path: &std::path::Path) {
        let command = crate::panels::files::cd_command(path);
        let mut failed = false;
        self.for_focused(|pane| {
            if let Some(session) = &pane.session {
                failed = session
                    .write(command.as_bytes(), crate::session::InputKind::Typed)
                    .is_err();
            }
        });
        if failed {
            self.notices
                .push("The terminal did not accept the cd command.".into());
        }
    }

    /// On-screen keyboard presses go through the same routing as physical keys.
    pub(super) fn on_osk(&mut self, message: crate::osk::OskMessage) -> Task<Message> {
        let crate::osk::OskMessage::Press(row, col) = message;
        match self
            .keyboard
            .as_mut()
            .and_then(|keyboard| keyboard.press(row, col))
        {
            Some(press) => self.on_key(press),
            None => Task::none(),
        }
    }

    fn adjust_font(&mut self, delta: f32) {
        let mut appearance = self.config.appearance.clone();
        appearance.font_size = if delta == 0.0 {
            Config::default().appearance.font_size
        } else {
            (appearance.font_size + delta).clamp(8.0, 48.0)
        };
        self.config.appearance = appearance;
        let config = self.config.clone();
        self.apply_appearance(&config);
    }

    /// Live appearance: theme, font and metrics. Triggers consistent resize epochs.
    pub(super) fn apply_appearance(&mut self, config: &Config) {
        let mut notices = Vec::new();
        self.theme = resolve_theme(&self.themes, &config.appearance.theme, &mut notices);
        self.iced_theme = super::style::iced_theme(&self.theme);
        self.font = font_for(&config.appearance.font_family);
        self.cell = CellMetrics::measure(
            self.font,
            config.appearance.font_size,
            config.appearance.line_height,
        );
        let palette = query_palette(&self.theme);
        let cell = self.cell;
        for (_, pane) in self.tabs.iter_mut().flat_map(|tab| tab.panes.iter_mut()) {
            pane.apply_size(cell);
            pane.send(SessionCommand::SetQueryPalette(Box::new(palette)));
        }
        self.notices.extend(notices);
    }

    pub(super) fn apply_config(&mut self, config: Config) {
        self.keymap = Keymap::build(&config.keybindings, cfg!(target_os = "macos"));
        self.notices.extend(self.keymap.diagnostics.iter().cloned());
        self.apply_appearance(&config);
        self.sound.set_settings(super::app::sound_settings(&config));
        if config.keyboard.layout != self.config.keyboard.layout && self.keyboard.is_some() {
            self.keyboard = Some(super::app::keyboard_for(
                &self.layouts,
                &config.keyboard.layout,
            ));
        }
        if config.keyboard.on_screen != self.config.keyboard.on_screen {
            self.keyboard = config
                .keyboard
                .on_screen
                .then(|| super::app::keyboard_for(&self.layouts, &config.keyboard.layout));
        }
        self.config = config;
        self.sync_metrics_worker();
    }

    /// Write only the changed fields to `ui-overrides.toml`, keeping `config.toml` untouched.
    pub(super) fn persist_overrides(&mut self, candidate: &Config) {
        let Some(paths) = &self.paths else { return };
        if self.options.safe_mode {
            self.notices
                .push("Safe mode: settings apply for this run only.".into());
            return;
        }
        let base = crate::config::load_effective(paths, true).config;
        let result = overrides_toml(&base, candidate)
            .map_err(|e| e.to_string())
            .and_then(|text| write_atomic(&paths.overrides_file, &text).map_err(|e| e.to_string()));
        if let Err(err) = result {
            self.notices
                .push(format!("Settings were applied but not saved: {err}"));
        }
    }

    pub(super) fn on_search_query(&mut self, query: String) -> Task<Message> {
        if let Some(search) = self.search.as_mut() {
            search.query = query.clone();
            search.index = 0;
        }
        self.for_focused(|pane| pane.send(SessionCommand::Search(query)));
        Task::none()
    }

    pub(super) fn search_step(&mut self, forward: bool) -> Task<Message> {
        let Some(search) = self.search.as_mut() else {
            return Task::none();
        };
        let Some(pane) = self.tabs.get(self.active).and_then(Tab::focused) else {
            return Task::none();
        };
        let Some(result) = pane.published.search.as_ref() else {
            return Task::none();
        };
        let len = result.matches.len();
        if len == 0 {
            return Task::none();
        }
        // Newest matches are at the end; stepping "forward" walks towards older output.
        search.index = if forward {
            (search.index + 1) % len
        } else {
            (search.index + len - 1) % len
        };
        let line = result.matches[len - 1 - search.index].line;
        pane.send(SessionCommand::RevealLine(line));
        Task::none()
    }
}

/// Palette used to answer OSC colour queries: 16 ANSI, then foreground, background.
fn query_palette(theme: &crate::config::Theme) -> [(u8, u8, u8); 18] {
    let mut palette = [(0, 0, 0); 18];
    for (slot, rgb) in palette.iter_mut().zip(theme.terminal.ansi.iter()) {
        *slot = (rgb.r, rgb.g, rgb.b);
    }
    let fg = theme.terminal.foreground;
    let bg = theme.terminal.background;
    palette[16] = (fg.r, fg.g, fg.b);
    palette[17] = (bg.r, bg.g, bg.b);
    palette
}
