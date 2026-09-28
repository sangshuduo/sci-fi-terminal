//! Command palette: fuzzy-ish filtering over the action registry and themes.

use iced::widget::{Id, button, column, row, scrollable, text, text_input};
use iced::{Element, Length};

use super::actions::{ACTIONS, Action, Keymap};
use super::style;
use crate::config::Theme;

pub const INPUT_ID: &str = "palette-input";
const MAX_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Action(Action),
    Theme(String),
}

#[derive(Debug, Clone)]
pub enum PaletteMsg {
    Query(String),
    Submit,
    Pick(Entry),
}

#[derive(Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
}

impl Palette {
    pub fn input_id() -> Id {
        Id::new(INPUT_ID)
    }

    pub fn move_selection(&mut self, delta: isize, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        let current = self.selected.min(len - 1) as isize;
        self.selected = (current + delta).rem_euclid(len as isize) as usize;
    }
}

/// Entries matching the query: every query word must appear in the label.
pub fn entries(query: &str, themes: &[Theme]) -> Vec<(Entry, String)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let matches = |label: &str| {
        let label = label.to_lowercase();
        words.iter().all(|word| label.contains(word))
    };
    let actions = ACTIONS
        .iter()
        .filter(|info| matches(info.title) || matches(info.id))
        .map(|info| (Entry::Action(info.action), info.title.to_owned()));
    let themes = themes
        .iter()
        .map(|theme| {
            (
                Entry::Theme(theme.id.clone()),
                format!("Theme: {}", theme.name),
            )
        })
        .filter(|(_, label)| matches(label));
    actions.chain(themes).collect()
}

pub fn view<'a, M: Clone + 'a>(
    palette: &'a Palette,
    themes: &'a [Theme],
    keymap: &'a Keymap,
    theme: &'a Theme,
    wrap: impl Fn(PaletteMsg) -> M + Copy + 'a,
) -> Element<'a, M> {
    let input = text_input("Type a command…", &palette.query)
        .id(Palette::input_id())
        .on_input(move |q| wrap(PaletteMsg::Query(q)))
        .on_submit(wrap(PaletteMsg::Submit))
        .size(16)
        .padding(8);
    let found = entries(&palette.query, themes);
    let selected = palette.selected.min(found.len().saturating_sub(1));
    let list = found.into_iter().take(MAX_ROWS).enumerate().fold(
        column![].spacing(2),
        |col, (index, (entry, label))| {
            let shortcut = match &entry {
                Entry::Action(action) => keymap
                    .shortcut_for(*action)
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                Entry::Theme(_) => String::new(),
            };
            let content = row![
                text(label).size(14).width(Length::Fill),
                text(shortcut).size(12)
            ]
            .spacing(12);
            col.push(
                button(content)
                    .width(Length::Fill)
                    .style(style::list_row(theme, index == selected))
                    .on_press(wrap(PaletteMsg::Pick(entry))),
            )
        },
    );
    column![input, scrollable(list).height(Length::Shrink)]
        .spacing(8)
        .width(520)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::builtin_themes;

    #[test]
    fn empty_query_lists_everything() {
        let themes = builtin_themes();
        assert_eq!(entries("", &themes).len(), ACTIONS.len() + themes.len());
    }

    #[test]
    fn words_filter_labels_and_ids() {
        let themes = builtin_themes();
        let found = entries("split right", &themes);
        assert_eq!(
            found.first().map(|(e, _)| e.clone()),
            Some(Entry::Action(Action::SplitRight))
        );
        assert!(
            entries("theme daylight", &themes)
                .iter()
                .any(|(e, _)| *e == Entry::Theme("daylight".into()))
        );
        assert!(entries("zzzz", &themes).is_empty());
    }

    #[test]
    fn selection_wraps() {
        let mut palette = Palette::default();
        palette.move_selection(-1, 3);
        assert_eq!(palette.selected, 2);
        palette.move_selection(1, 3);
        assert_eq!(palette.selected, 0);
        palette.move_selection(1, 0);
        assert_eq!(palette.selected, 0);
    }
}
