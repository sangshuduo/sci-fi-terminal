//! Widget tree: session bar, pane grid, side panel, search, status and modals.

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{
    Space, button, canvas, center, column, container, opaque, row, scrollable, stack, text,
    text_input,
};
use iced::{Element, Length};

use super::app::{App, Confirm, Message};
use super::palette;
use super::settings;
use super::state::{PaneState, Tab};
use super::style;
use crate::panels::PanelContext;
use crate::render::TerminalView;

pub const SEARCH_ID: &str = "search-input";
const PANEL_WIDTH: f32 = 240.0;
const PREVIEW_LINES: usize = 12;

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let body = row![self.panes_view()]
            .extend(self.panel_view())
            .spacing(4)
            .height(Length::Fill);
        let base = column![self.tab_bar(), body]
            .extend(self.search_view())
            .push(self.status_bar())
            .spacing(4)
            .padding(4);
        let base: Element<'_, Message> = container(base)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| iced::widget::container::Style {
                background: Some(
                    crate::render::cells::to_color(self.theme.colors.background).into(),
                ),
                ..Default::default()
            })
            .into();
        match self.modal_view() {
            Some(modal) => stack![base, modal].into(),
            None => base,
        }
    }

    fn tab_bar(&self) -> Element<'_, Message> {
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .fold(row![].spacing(4), |bar, (index, tab)| {
                let active = index == self.active;
                let label = truncate(&tab.title(), 28);
                let close = button(text("×").size(13))
                    .padding([0, 4])
                    .style(button::text)
                    .on_press(Message::CloseTab(index));
                let tab_button = button(
                    row![text(label).size(13), close]
                        .spacing(6)
                        .align_y(iced::Alignment::Center),
                )
                .padding([3, 10])
                .style(style::tab(&self.theme, active))
                .on_press(Message::SelectTab(index));
                bar.push(tab_button)
            });
        let action = |label: &'static str, action| {
            button(text(label).size(13))
                .padding([3, 8])
                .style(button::text)
                .on_press(Message::Run(action))
        };
        let bar = row![
            tabs,
            action("+", super::actions::Action::NewTab),
            Space::new().width(Length::Fill),
            action("Commands", super::actions::Action::TogglePalette),
            action("System", super::actions::Action::ToggleMetrics),
            action("Settings", super::actions::Action::OpenSettings),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);
        container(bar)
            .padding([2, 6])
            .width(Length::Fill)
            .style(|_| style::bar(&self.theme))
            .into()
    }

    fn panes_view(&self) -> Element<'_, Message> {
        let Some(tab) = self.active_tab() else {
            return center(text("No sessions. Press the + button to open one.")).into();
        };
        let multiple = tab.panes.len() > 1;
        PaneGrid::new(&tab.panes, |pane, state, _maximized| {
            let focused = pane == tab.focus;
            let mut content = pane_grid::Content::new(self.terminal(pane, state, focused));
            if multiple {
                let title = text(truncate(&state.title(), 60)).size(12);
                content = content.title_bar(
                    pane_grid::TitleBar::new(title)
                        .padding([2, 8])
                        .style(move |_| style::bar(&self.theme)),
                );
            }
            content
        })
        .spacing(4)
        .on_click(Message::PaneClicked)
        .on_drag(Message::PaneDragged)
        .on_resize(8, Message::PaneResized)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn terminal<'a>(
        &'a self,
        pane: pane_grid::Pane,
        state: &'a PaneState,
        focused: bool,
    ) -> Element<'a, Message> {
        let view = TerminalView {
            snapshot: state.published.snapshot.as_ref(),
            theme: &self.theme,
            metrics: self.cell,
            font: self.font,
            focused: focused && self.window_focused,
            cache: &state.cache,
            search: if focused && self.search.is_some() {
                state.published.search.as_deref()
            } else {
                None
            },
            effects: if self.config.appearance.reduced_motion
                && self.config.effects.intensity <= 0.0
            {
                crate::config::EffectsPreset::Off
            } else {
                self.config.effects.preset
            },
            overlay: state.overlay(),
            on_event: Box::new(move |event| Message::Terminal(pane, event)),
        };
        canvas(view).width(Length::Fill).height(Length::Fill).into()
    }

    fn panel_view(&self) -> Option<Element<'_, Message>> {
        if !self.panel_visible || !self.config.panels.metrics.enabled {
            return None;
        }
        let live = self
            .tabs
            .iter()
            .flat_map(|t| t.panes.iter())
            .filter(|(_, p)| p.is_live())
            .count();
        let context = PanelContext {
            metrics: self.last_metrics.as_ref(),
            session_count: self.session_count(),
            live_sessions: live,
        };
        let sections = self
            .panels
            .iter()
            .fold(column![].spacing(16), |col, panel| {
                col.push(column![text(panel.title()).size(15), panel.view(&context)].spacing(8))
            });
        Some(
            container(scrollable(sections))
                .padding(10)
                .width(PANEL_WIDTH)
                .height(Length::Fill)
                .style(|_| style::surface(&self.theme))
                .into(),
        )
    }

    fn search_view(&self) -> Option<Element<'_, Message>> {
        let search = self.search.as_ref()?;
        let result = self
            .active_tab()
            .and_then(Tab::focused)
            .and_then(|p| p.published.search.as_ref());
        let summary = match result {
            Some(r) if !search.query.is_empty() && r.matches.is_empty() => "No matches".to_owned(),
            Some(r) if !search.query.is_empty() => {
                let more = if r.truncated { "+" } else { "" };
                format!("{} of {}{more}", search.index + 1, r.matches.len())
            }
            _ => String::new(),
        };
        let bar = row![
            text("Find").size(13),
            text_input("Literal text in scrollback", &search.query)
                .id(iced::widget::Id::new(SEARCH_ID))
                .on_input(Message::SearchQuery)
                .on_submit(Message::SearchStep(true))
                .size(13)
                .width(Length::Fill),
            text(summary).size(12),
            button(text("Older").size(12))
                .style(button::secondary)
                .on_press(Message::SearchStep(true)),
            button(text("Newer").size(12))
                .style(button::secondary)
                .on_press(Message::SearchStep(false)),
            button(text("Close").size(12))
                .style(button::text)
                .on_press(Message::SearchClose),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        Some(
            container(bar)
                .padding([4, 8])
                .style(|_| style::bar(&self.theme))
                .into(),
        )
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let focused = self.active_tab().and_then(Tab::focused);
        let session = focused.map_or_else(String::new, |pane| {
            let label = pane.session.as_ref().map_or("no shell", |s| s.label());
            let size = pane
                .published
                .snapshot
                .as_ref()
                .map_or_else(String::new, |s| format!("  {}×{}", s.columns, s.rows));
            let busy = if pane.session.as_ref().is_some_and(|s| s.input_busy()) {
                "  busy"
            } else {
                ""
            };
            format!("{label}{size}{busy}")
        });
        let mut bar = row![text(session).size(12), Space::new().width(Length::Fill)].spacing(12);
        if let Some(notice) = self.notices.last() {
            let count = if self.notices.len() > 1 {
                format!(" (+{} more)", self.notices.len() - 1)
            } else {
                String::new()
            };
            bar = bar
                .push(
                    text(format!("{}{count}", truncate(notice, 120)))
                        .size(12)
                        .style(text::warning),
                )
                .push(
                    button(text("Dismiss").size(12))
                        .padding([0, 6])
                        .style(button::text)
                        .on_press(Message::DismissNotice),
                );
        }
        container(bar.align_y(iced::Alignment::Center))
            .padding([2, 8])
            .style(|_| style::bar(&self.theme))
            .into()
    }

    fn modal_view(&self) -> Option<Element<'_, Message>> {
        let content: Element<'_, Message> = if let Some(confirm) = &self.confirm {
            self.confirm_view(confirm)
        } else if let Some(settings) = &self.settings {
            container(settings::view(settings, &self.themes, Message::Settings))
                .width(760)
                .height(560)
                .into()
        } else if let Some(palette) = &self.palette {
            palette::view(
                palette,
                &self.themes,
                &self.keymap,
                &self.theme,
                Message::Palette,
            )
        } else {
            return None;
        };
        let dialog = container(content)
            .padding(16)
            .style(|_| style::dialog(&self.theme));
        Some(opaque(center(dialog).style(|_| style::backdrop())))
    }

    fn confirm_view(&self, confirm: &Confirm) -> Element<'_, Message> {
        let (title, detail, accept, on_accept, on_cancel) = match confirm {
            Confirm::Paste { text: content, .. } => (
                "Paste this text?".to_owned(),
                paste_preview(content),
                "Paste",
                Message::PasteDecision(true),
                Message::PasteDecision(false),
            ),
            Confirm::ClosePane(_) => (
                "Close this pane?".to_owned(),
                "The running shell and any programs it started in this terminal will be terminated.".to_owned(),
                "Close",
                Message::CloseDecision(true),
                Message::CloseDecision(false),
            ),
            Confirm::CloseWindow => (
                "Close the window?".to_owned(),
                format!("{} running session(s) will be terminated.", self.session_count()),
                "Close all",
                Message::CloseDecision(true),
                Message::CloseDecision(false),
            ),
        };
        column![
            text(title).size(18),
            scrollable(text(detail).size(13).font(iced::Font::MONOSPACE)).height(Length::Shrink),
            row![
                Space::new().width(Length::Fill),
                button(text("Cancel"))
                    .style(button::secondary)
                    .on_press(on_cancel),
                button(text(accept))
                    .style(button::primary)
                    .on_press(on_accept),
            ]
            .spacing(8),
            text("Enter confirms · Escape cancels").size(11),
        ]
        .spacing(12)
        .width(560)
        .into()
    }
}

/// Show exactly what will be sent, with control characters made visible.
fn paste_preview(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut preview: String = lines
        .iter()
        .take(PREVIEW_LINES)
        .map(|line| line.chars().map(visible_control).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    if lines.len() > PREVIEW_LINES {
        preview.push_str(&format!("\n… {} more line(s)", lines.len() - PREVIEW_LINES));
    }
    format!(
        "{} line(s), {} bytes\n\n{preview}",
        lines.len().max(1),
        content.len()
    )
}

fn visible_control(c: char) -> char {
    match c {
        '\x1b' => '␛',
        '\t' => '⇥',
        c if c.is_control() => char::from_u32(0x2400 + c as u32)
            .filter(|_| (c as u32) < 0x20)
            .unwrap_or('�'),
        c => c,
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_preview_reveals_controls_and_counts() {
        let preview = paste_preview("echo hi\nrm -rf x\x1b[31m");
        assert!(preview.starts_with("2 line(s), 21 bytes"));
        assert!(preview.contains("␛[31m"));
    }

    #[test]
    fn long_pastes_are_summarised() {
        let text = (0..20)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(paste_preview(&text).contains("8 more line(s)"));
    }

    #[test]
    fn truncation_is_char_safe() {
        assert_eq!(truncate("中文标题很长", 4), "中文标…");
        assert_eq!(truncate("short", 10), "short");
    }
}
