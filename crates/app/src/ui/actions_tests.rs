use super::*;

fn binding(action: &str, context: &str, keys: &str) -> KeyBinding {
    KeyBinding {
        action: action.into(),
        context: context.into(),
        keys: keys.into(),
    }
}

#[test]
fn every_action_is_registered_once() {
    let all = [
        Action::NewTab,
        Action::ClosePane,
        Action::SplitRight,
        Action::SplitDown,
        Action::FocusNext,
        Action::FocusPrevious,
        Action::ZoomPane,
        Action::NextTab,
        Action::PreviousTab,
        Action::Copy,
        Action::Paste,
        Action::Search,
        Action::ScrollPageUp,
        Action::ScrollPageDown,
        Action::TogglePalette,
        Action::OpenSettings,
        Action::ToggleMetrics,
        Action::ResetLayout,
        Action::FontIncrease,
        Action::FontDecrease,
        Action::FontReset,
    ];
    assert_eq!(all.len(), ACTIONS.len());
    for action in all {
        assert_eq!(action.info().action, action);
        assert_eq!(Action::from_id(action.info().id), Some(action));
    }
}

#[test]
fn chord_parsing() {
    let chord = Chord::parse("Ctrl+Shift+T").expect("parse");
    assert!(chord.ctrl && chord.shift && !chord.alt && chord.key == "t");
    assert_eq!(
        Chord::parse("Cmd+,").map(|c| (c.logo, c.key)),
        Ok((true, ",".into()))
    );
    assert_eq!(Chord::parse("Ctrl++").map(|c| c.key), Ok("+".into()));
    assert_eq!(
        Chord::parse("shift+pgup").map(|c| c.key),
        Ok("pageup".into())
    );
    assert!(Chord::parse("Hyper+X").is_err());
    assert!(Chord::parse("Ctrl+Banana").is_err());
    assert!(Chord::parse("").is_err());
}

#[test]
fn default_keymaps_have_no_conflicts() {
    for mac in [false, true] {
        let keymap = Keymap::build(&[], mac);
        assert!(
            keymap.diagnostics.is_empty(),
            "mac={mac}: {:?}",
            keymap.diagnostics
        );
    }
}

#[test]
fn ctrl_c_is_never_intercepted_on_unix_defaults() {
    let keymap = Keymap::build(&[], false);
    let ctrl_c = Chord::parse("Ctrl+C").expect("parse");
    assert_eq!(keymap.resolve(&ctrl_c, true), None);
}

#[test]
fn global_beats_terminal_and_terminal_needs_focus() {
    let keymap = Keymap::build(&[], false);
    let copy = Chord::parse("Ctrl+Shift+C").expect("parse");
    assert_eq!(keymap.resolve(&copy, true), Some(Action::Copy));
    assert_eq!(keymap.resolve(&copy, false), None);
    let palette = Chord::parse("Ctrl+Shift+P").expect("parse");
    assert_eq!(keymap.resolve(&palette, false), Some(Action::TogglePalette));
}

#[test]
fn overrides_rebind_unbind_and_report_conflicts() {
    let overrides = [
        binding("session.new_tab", "global", "Ctrl+Alt+N"),
        binding("pane.zoom", "global", "none"),
        binding("pane.split_right", "global", "Ctrl+Alt+N"),
        binding("does.not_exist", "global", "Ctrl+Alt+X"),
        binding("terminal.copy", "terminal", "C"),
    ];
    let keymap = Keymap::build(&overrides, false);
    let new_tab = Chord::parse("Ctrl+Alt+N").expect("parse");
    let zoom = Chord::parse("Ctrl+Shift+Z").expect("parse");
    assert!(matches!(
        keymap.resolve(&new_tab, true),
        Some(Action::NewTab | Action::SplitRight)
    ));
    assert_eq!(keymap.resolve(&zoom, true), None);
    assert!(
        keymap
            .diagnostics
            .iter()
            .any(|d| d.contains("bound to both"))
    );
    assert!(
        keymap
            .diagnostics
            .iter()
            .any(|d| d.contains("unknown action"))
    );
    assert_eq!(keymap.shortcut_for(Action::ZoomPane), None);
}

#[test]
fn bare_text_key_is_rejected_in_terminal_context_too() {
    for keys in ["c", "Shift+C", "Enter"] {
        let keymap = Keymap::build(&[binding("terminal.copy", "terminal", keys)], false);
        assert!(
            keymap
                .diagnostics
                .iter()
                .any(|d| d.contains("would block typing")),
            "{keys} should be rejected"
        );
        let chord = Chord::parse(keys).expect("parse");
        assert_eq!(
            keymap.resolve(&chord, true),
            None,
            "{keys} must reach the terminal"
        );
    }
    // Non-text keys with only Shift remain valid terminal shortcuts.
    let keymap = Keymap::build(&[], false);
    assert!(keymap.diagnostics.is_empty());
    let page_up = Chord::parse("Shift+PageUp").expect("parse");
    assert_eq!(keymap.resolve(&page_up, true), Some(Action::ScrollPageUp));
}

#[test]
fn unmodified_global_shortcut_is_rejected() {
    let keymap = Keymap::build(&[binding("palette.toggle", "global", "P")], false);
    assert!(
        keymap
            .diagnostics
            .iter()
            .any(|d| d.contains("would block typing"))
    );
}

#[test]
fn reserved_shortcut_warns() {
    let keymap = Keymap::build(&[binding("palette.toggle", "global", "Alt+F4")], false);
    assert!(keymap.diagnostics.iter().any(|d| d.contains("reserved")));
}
