//! Session wakeups, keyboard routing, pointer handling and paste policy.

use iced::Task;
use iced::keyboard::{Key, key::Named};
use iced::mouse::Button;
use iced::widget::pane_grid::Pane;
use terminal_core::input::{self, MouseAction, MouseButton, paste_risk};
use terminal_core::{GridPoint, SelectionKind, SessionId};

use super::app::{App, Confirm, Message};
use super::input::{KeyPress, chord, encode};
use super::state::{PaneState, Tab};
use crate::render::TerminalEvent;
use crate::session::{InputKind, QueueError, SessionCommand};
use crate::sound::Cue;

impl App {
    /// A session published new state: refresh only that pane.
    pub(super) fn on_wake(&mut self, id: SessionId) -> Task<Message> {
        let pane = self
            .tabs
            .iter_mut()
            .flat_map(|tab| tab.panes.iter_mut())
            .map(|(_, pane)| pane)
            .find(|pane| pane.session.as_ref().is_some_and(|s| s.id() == id));
        let Some(pane) = pane else {
            return Task::none();
        };
        let (bells, was_live) = (pane.published.bells, pane.published.status.is_live());
        let copied = pane.refresh();
        let rang = pane.published.bells != bells;
        let ended = was_live && !pane.published.status.is_live();
        if rang {
            self.play(Cue::Bell);
        }
        if ended {
            self.play(Cue::SessionExit);
        }
        match copied {
            Some(text) => iced::clipboard::write(text),
            None => Task::none(),
        }
    }

    pub(super) fn on_key(&mut self, press: KeyPress) -> Task<Message> {
        if let Some(task) = self.route_modal_key(&press) {
            return task;
        }
        if let Some(action) = chord(&press).and_then(|c| self.keymap.resolve(&c, true)) {
            return self.run(action);
        }
        self.send_key(&press);
        Task::none()
    }

    /// Modals take keys first; `Some` means the key was consumed.
    fn route_modal_key(&mut self, press: &KeyPress) -> Option<Task<Message>> {
        let named = match press.key.as_ref() {
            Key::Named(named) => Some(named),
            _ => None,
        };
        if self.confirm.is_some() {
            return Some(match named {
                Some(Named::Enter) => self.on_close_decision(true),
                Some(Named::Escape) => self.on_close_decision(false),
                _ => Task::none(),
            });
        }
        if let Some(palette) = self.palette.as_mut() {
            let len = super::palette::entries(&palette.query, &self.themes).len();
            match named {
                Some(Named::Escape) => self.palette = None,
                Some(Named::ArrowDown) => palette.move_selection(1, len),
                Some(Named::ArrowUp) => palette.move_selection(-1, len),
                _ => {}
            }
            return Some(Task::none());
        }
        if self.settings.is_some() {
            if named == Some(Named::Escape) {
                return Some(self.on_settings(super::settings::SettingsMsg::Cancel));
            }
            return Some(Task::none());
        }
        if self.search.is_some() && named == Some(Named::Escape) {
            return Some(self.update(Message::SearchClose));
        }
        None
    }

    fn send_key(&mut self, press: &KeyPress) {
        let option_as_alt = self.config.input.option_as_alt;
        self.play(Cue::KeyPress);
        let mut busy = false;
        self.for_focused(|pane| {
            let Some(session) = &pane.session else { return };
            let mode = pane
                .published
                .snapshot
                .as_ref()
                .map(|s| s.mode)
                .unwrap_or_default();
            let Some(bytes) = encode(press, mode, option_as_alt) else {
                return;
            };
            match session.write(&bytes, InputKind::Typed) {
                Ok(()) => {
                    if pane
                        .published
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.display_offset > 0)
                    {
                        pane.send(SessionCommand::ScrollToBottom);
                    }
                }
                Err(QueueError::InputBusy) => busy = true,
                Err(_) => {}
            }
        });
        if busy {
            self.notices
                .push("The terminal is busy and did not accept that input.".into());
        }
    }

    pub(super) fn on_terminal(&mut self, pane_id: Pane, event: TerminalEvent) -> Task<Message> {
        if !matches!(
            event,
            TerminalEvent::Resized(_) | TerminalEvent::Wheel { .. }
        ) {
            self.focus_pane(pane_id);
        }
        let cell = self.cell;
        let copy_on_select = self.config.terminal.copy_on_select;
        let Some(pane) = self
            .tabs
            .get_mut(self.active)
            .and_then(|tab| tab.panes.get_mut(pane_id))
        else {
            return Task::none();
        };
        match event {
            TerminalEvent::Resized(size) => {
                pane.pixel_size = Some(size);
                pane.apply_size(cell);
            }
            TerminalEvent::Pressed {
                at,
                button,
                clicks,
                shift,
                alt,
                ..
            } => {
                if !report_mouse(pane, button, MouseAction::Press, at, shift)
                    && button == Button::Left
                {
                    let kind = match clicks {
                        2 => SelectionKind::Word,
                        3 => SelectionKind::Line,
                        _ if alt => SelectionKind::Block,
                        _ => SelectionKind::Simple,
                    };
                    pane.send(SessionCommand::SelectStart(at, kind));
                }
            }
            TerminalEvent::Dragged { at } => {
                if !report_mouse(pane, Button::Left, MouseAction::Drag, at, false) {
                    pane.send(SessionCommand::SelectUpdate(at));
                }
            }
            TerminalEvent::Released { at, button } => {
                let reported = report_mouse(pane, button, MouseAction::Release, at, false);
                if !reported && button == Button::Left && copy_on_select {
                    pane.send(SessionCommand::CopySelection);
                }
            }
            TerminalEvent::Wheel { lines, at } => wheel(pane, lines, at),
            TerminalEvent::Tapped { .. } => pane.send(SessionCommand::SelectClear),
        }
        Task::none()
    }

    pub(super) fn on_clipboard(&mut self, text: Option<String>) -> Task<Message> {
        let Some(text) = text.filter(|t| !t.is_empty()) else {
            return Task::none();
        };
        let Some(tab) = self.tabs.get(self.active) else {
            return Task::none();
        };
        let pane = tab.focus;
        if self.config.terminal.confirm_multiline_paste && paste_risk(&text).needs_confirmation() {
            self.confirm = Some(Confirm::Paste { pane, text });
            return Task::none();
        }
        self.send_paste(pane, &text);
        Task::none()
    }

    pub(super) fn on_paste_decision(&mut self, accept: bool) -> Task<Message> {
        if let Some(Confirm::Paste { pane, text }) = self.confirm.take()
            && accept
        {
            self.send_paste(pane, &text);
        }
        Task::none()
    }

    fn send_paste(&mut self, pane_id: Pane, text: &str) {
        let Some(pane) = self
            .tabs
            .get(self.active)
            .and_then(|tab: &Tab| tab.panes.get(pane_id))
        else {
            return;
        };
        let Some(session) = &pane.session else { return };
        let mode = pane
            .published
            .snapshot
            .as_ref()
            .map(|s| s.mode)
            .unwrap_or_default();
        let result = input::encode_paste(text, mode)
            .map_err(|err| err.to_string())
            .and_then(|bytes| {
                session
                    .write(&bytes, InputKind::Paste)
                    .map_err(|err| err.to_string())
            });
        if let Err(err) = result {
            self.notices.push(format!("Paste not sent: {err}"));
        }
    }
}

/// Forward a pointer event to the application when it requested mouse
/// reporting. Shift bypasses reporting so local selection always works.
fn report_mouse(
    pane: &PaneState,
    button: Button,
    action: MouseAction,
    at: GridPoint,
    shift: bool,
) -> bool {
    let Some(snapshot) = pane.published.snapshot.as_ref() else {
        return false;
    };
    if shift || !snapshot.mode.mouse_active() {
        return false;
    }
    let button = match button {
        Button::Left => MouseButton::Left,
        Button::Middle => MouseButton::Middle,
        Button::Right => MouseButton::Right,
        _ => return false,
    };
    let mods = input::Modifiers::default();
    if let (Some(bytes), Some(session)) = (
        input::encode_mouse(button, action, at.col, at.row, mods, snapshot.mode),
        &pane.session,
    ) {
        let _ = session.write(&bytes, InputKind::Typed);
    }
    true
}

fn wheel(pane: &mut PaneState, lines: f32, at: GridPoint) {
    pane.scroll_accum += lines;
    let whole = pane.scroll_accum.trunc() as i32;
    if whole == 0 {
        return;
    }
    pane.scroll_accum -= whole as f32;
    let Some(snapshot) = pane.published.snapshot.clone() else {
        return;
    };
    let Some(session) = &pane.session else { return };
    let mode = snapshot.mode;
    if mode.mouse_active() {
        let button = if whole > 0 {
            MouseButton::WheelUp
        } else {
            MouseButton::WheelDown
        };
        for _ in 0..whole.unsigned_abs().min(10) {
            if let Some(bytes) = input::encode_mouse(
                button,
                MouseAction::Press,
                at.col,
                at.row,
                input::Modifiers::default(),
                mode,
            ) {
                let _ = session.write(&bytes, InputKind::Typed);
            }
        }
    } else if mode.alt_screen && mode.alternate_scroll {
        // Full-screen apps without mouse mode get arrow keys, as users expect in pagers.
        let key = if whole > 0 {
            input::Key::Up
        } else {
            input::Key::Down
        };
        for _ in 0..whole.unsigned_abs().min(10) {
            if let Some(bytes) = input::encode_key(&key, input::Modifiers::default(), mode) {
                let _ = session.write(&bytes, InputKind::Typed);
            }
        }
    } else {
        pane.send(SessionCommand::Scroll(whole));
    }
}
