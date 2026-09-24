use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use super::*;
use crate::config::{ConfigError, MAX_CONFIG_BYTES};
use crate::ui::input::{KeyPress, encode};

const EXAMPLE: &str = r#"
schema_version = 1
id = "my-layout"
name = "My layout"

[[rows]]
keys = [
  { text = "q", shifted = "Q" },
  { named = "backspace", label = "BS", width = 1.5 },
  { modifier = "shift", width = 2.0 },
  { action = "caps" },
]

[[rows]]
keys = [{ text = "1" }, { named = "f5" }]
"#;

fn path() -> &'static Path {
    Path::new("test.toml")
}

fn parse_err(text: &str) -> String {
    match parse_layout(text, path()) {
        Err(ConfigError::Parse { message, .. }) => message,
        other => panic!("expected parse error, got {other:?}"),
    }
}

fn one_row(keys: &str) -> String {
    format!("schema_version = 1\nid = \"x\"\nname = \"X\"\n[[rows]]\nkeys = [{keys}]\n")
}

fn find(kb: &Keyboard, pred: impl Fn(&KeySpec) -> bool) -> (usize, usize) {
    for (r, row) in kb.layout.rows.iter().enumerate() {
        if let Some(c) = row.iter().position(&pred) {
            return (r, c);
        }
    }
    panic!("key not found");
}

fn text_pos(kb: &Keyboard, base: &str) -> (usize, usize) {
    find(
        kb,
        |k| matches!(&k.action, KeyAction::Text { base: b, .. } if b == base),
    )
}

fn action_pos(kb: &Keyboard, action: KeyAction) -> (usize, usize) {
    find(kb, |k| k.action == action)
}

fn press_at(kb: &mut Keyboard, pos: (usize, usize)) -> Option<KeyPress> {
    kb.press(pos.0, pos.1)
}

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("osk-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// --- layout ---------------------------------------------------------------

#[test]
fn builtin_layout_is_valid() {
    let layout = builtin_layout();
    assert_eq!(layout.id, "en-us");
    assert_eq!(layout.rows.len(), 5);
    let keys: Vec<&KeySpec> = layout.rows.iter().flatten().collect();
    assert!(keys.len() <= MAX_KEYS);
    assert!(layout.rows.iter().all(|row| !row.is_empty()));
    let has = |a: KeyAction| keys.iter().any(|k| k.action == a);
    assert!(has(KeyAction::Named(Named::Enter)));
    assert!(has(KeyAction::Named(Named::Backspace)));
    assert!(has(KeyAction::Named(Named::Space)));
    assert!(has(KeyAction::Modifier(Modifier::Shift)));
    assert!(has(KeyAction::ToggleCaps));
    assert!(keys.iter().all(|k| (0.5..=8.0).contains(&k.width)));
}

#[test]
fn parses_example() {
    let layout = parse_layout(EXAMPLE, path()).unwrap();
    assert_eq!(layout.id, "my-layout");
    assert_eq!(layout.name, "My layout");
    assert_eq!(layout.rows.len(), 2);
    let row = &layout.rows[0];
    assert_eq!(
        row[0].action,
        KeyAction::Text {
            base: "q".into(),
            shifted: Some("Q".into())
        }
    );
    assert_eq!(row[0].label, "q");
    assert_eq!(row[0].width, 1.0);
    assert_eq!(row[1].action, KeyAction::Named(Named::Backspace));
    assert_eq!(row[1].label, "BS");
    assert_eq!(row[1].width, 1.5);
    assert_eq!(row[2].action, KeyAction::Modifier(Modifier::Shift));
    assert_eq!(row[2].label, "Shift");
    assert_eq!(row[3].action, KeyAction::ToggleCaps);
    assert_eq!(layout.rows[1][1].action, KeyAction::Named(Named::F5));
}

#[test]
fn rejects_unknown_field() {
    let msg = parse_err(&one_row(r#"{ text = "a", colour = "red" }"#));
    assert!(msg.contains("colour"), "{msg}");
    let top = format!("extra = 1\n{}", one_row(r#"{ text = "a" }"#));
    assert!(parse_err(&top).contains("extra"));
}

#[test]
fn rejects_two_actions_on_one_key() {
    let msg = parse_err(&one_row(
        r#"{ text = "a" }, { text = "b", named = "enter" }"#,
    ));
    assert!(msg.contains("row 0, key 1"), "{msg}");
    assert!(msg.contains("exactly one"), "{msg}");
    assert!(parse_err(&one_row("{ width = 1.0 }")).contains("exactly one"));
}

#[test]
fn rejects_bad_text() {
    let msg = parse_err(&one_row(r#"{ text = "a\u0003" }"#));
    assert!(msg.contains("control"), "{msg}");
    assert!(parse_err(&one_row(r#"{ text = "abcde" }"#)).contains("1-4"));
    assert!(parse_err(&one_row(r#"{ text = "" }"#)).contains("1-4"));
    assert!(parse_err(&one_row(r#"{ text = "a", shifted = "\n" }"#)).contains("control"));
    assert!(parse_err(&one_row(r#"{ named = "enter", shifted = "x" }"#)).contains("shifted"));
}

#[test]
fn rejects_unknown_named_and_action() {
    assert!(parse_err(&one_row(r#"{ named = "launch" }"#)).contains("unknown named"));
    assert!(parse_err(&one_row(r#"{ action = "run" }"#)).contains("unknown action"));
    assert!(parse_err(&one_row(r#"{ modifier = "super" }"#)).contains("super"));
}

#[test]
fn rejects_too_many_keys_and_rows() {
    let keys = vec![r#"{ text = "a" }"#; MAX_KEYS + 1].join(", ");
    assert!(parse_err(&one_row(&keys)).contains("maximum"));
    let mut rows = String::from("schema_version = 1\nid = \"x\"\nname = \"X\"\n");
    for _ in 0..9 {
        rows.push_str("[[rows]]\nkeys = [{ text = \"a\" }]\n");
    }
    assert!(parse_err(&rows).contains("rows"));
    assert!(parse_err(&one_row("")).contains("row 0"));
}

#[test]
fn rejects_bad_id_and_version() {
    let bad = one_row(r#"{ text = "a" }"#).replace("id = \"x\"", "id = \"../Evil\"");
    assert!(parse_err(&bad).contains("id"));
    let v2 = one_row(r#"{ text = "a" }"#).replace("schema_version = 1", "schema_version = 2");
    assert!(parse_err(&v2).contains("schema_version"));
}

#[test]
fn rejects_bad_width() {
    for width in ["0.2", "9.0", "nan", "-1.0"] {
        let msg = parse_err(&one_row(&format!(r#"{{ text = "a", width = {width} }}"#)));
        assert!(msg.contains("width"), "{width}: {msg}");
    }
}

#[test]
fn load_user_layouts_reads_valid_and_reports_invalid() {
    let dir = unique_dir("load");
    fs::write(dir.join("good.toml"), EXAMPLE).unwrap();
    fs::write(dir.join("bad.toml"), "id = 3").unwrap();
    fs::write(dir.join("notes.txt"), "ignored").unwrap();
    let (layouts, errors) = load_user_layouts(&dir);
    assert_eq!(layouts.len(), 1);
    assert_eq!(errors.len(), 1);
    let (none, no_errors) = load_user_layouts(&dir.join("missing"));
    assert!(none.is_empty() && no_errors.is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn load_user_layouts_rejects_oversize_files() {
    let dir = unique_dir("big");
    let mut text = String::from(EXAMPLE);
    text.push_str(&"#".repeat(MAX_CONFIG_BYTES as usize + 1));
    fs::write(dir.join("big.toml"), text).unwrap();
    let (layouts, errors) = load_user_layouts(&dir);
    assert!(layouts.is_empty());
    assert!(matches!(errors.as_slice(), [ConfigError::TooLarge { .. }]));
    fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn load_user_layouts_skips_symlinks() {
    let dir = unique_dir("link");
    let target = dir.join("real.txt");
    fs::write(&target, EXAMPLE).unwrap();
    std::os::unix::fs::symlink(&target, dir.join("link.toml")).unwrap();
    let (layouts, errors) = load_user_layouts(&dir);
    assert!(layouts.is_empty());
    assert!(errors.is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

// --- state ----------------------------------------------------------------

#[test]
fn sticky_modifier_is_one_shot() {
    let mut kb = Keyboard::new(builtin_layout());
    let shift = action_pos(&kb, KeyAction::Modifier(Modifier::Shift));
    let a = text_pos(&kb, "a");
    assert!(press_at(&mut kb, shift).is_none());
    assert!(kb.is_active(Modifier::Shift));
    let first = press_at(&mut kb, a).unwrap();
    assert_eq!(first.text.as_deref(), Some("A"));
    assert!(first.modifiers.shift());
    assert!(!kb.is_active(Modifier::Shift));
    let second = press_at(&mut kb, a).unwrap();
    assert_eq!(second.text.as_deref(), Some("a"));
    assert_eq!(second.modifiers, Modifiers::empty());
}

#[test]
fn modifier_press_toggles_off() {
    let mut kb = Keyboard::new(builtin_layout());
    let ctrl = action_pos(&kb, KeyAction::Modifier(Modifier::Ctrl));
    press_at(&mut kb, ctrl);
    press_at(&mut kb, ctrl);
    assert!(!kb.is_active(Modifier::Ctrl));
}

#[test]
fn caps_affects_letters_only() {
    let mut kb = Keyboard::new(builtin_layout());
    let caps = action_pos(&kb, KeyAction::ToggleCaps);
    assert!(press_at(&mut kb, caps).is_none());
    assert!(kb.caps());
    let a = text_pos(&kb, "a");
    let one = text_pos(&kb, "1");
    assert_eq!(press_at(&mut kb, a).unwrap().text.as_deref(), Some("A"));
    assert_eq!(press_at(&mut kb, one).unwrap().text.as_deref(), Some("1"));
    assert!(kb.caps(), "caps lock persists");
    let shift = action_pos(&kb, KeyAction::Modifier(Modifier::Shift));
    press_at(&mut kb, shift);
    assert_eq!(press_at(&mut kb, a).unwrap().text.as_deref(), Some("a"));
}

#[test]
fn label_follows_shift() {
    let mut kb = Keyboard::new(builtin_layout());
    let (r, c) = text_pos(&kb, "2");
    let key = kb.layout.rows[r][c].clone();
    assert_eq!(kb.label(&key), "2");
    let shift = action_pos(&kb, KeyAction::Modifier(Modifier::Shift));
    press_at(&mut kb, shift);
    assert_eq!(kb.label(&key), "@");
    let enter = action_pos(&kb, KeyAction::Named(Named::Enter));
    let enter_key = kb.layout.rows[enter.0][enter.1].clone();
    assert_eq!(kb.label(&enter_key), "Enter");
}

#[test]
fn out_of_range_press_is_none() {
    let mut kb = Keyboard::new(builtin_layout());
    assert!(kb.press(99, 0).is_none());
    assert!(kb.press(0, 99).is_none());
}

#[test]
fn named_keys_produce_named_presses() {
    let mut kb = Keyboard::new(builtin_layout());
    let space = action_pos(&kb, KeyAction::Named(Named::Space));
    let press = press_at(&mut kb, space).unwrap();
    assert_eq!(press.key, Key::Named(Named::Space));
    assert_eq!(press.text.as_deref(), Some(" "));
    let up = action_pos(&kb, KeyAction::Named(Named::ArrowUp));
    assert_eq!(press_at(&mut kb, up).unwrap().text, None);
}

// --- encode integration ---------------------------------------------------

fn encoded(press: &KeyPress) -> Option<Vec<u8>> {
    encode(press, terminal_core::ModeFlags::default(), false)
}

#[test]
fn encodes_like_a_physical_keyboard() {
    let mut kb = Keyboard::new(builtin_layout());
    let ctrl = action_pos(&kb, KeyAction::Modifier(Modifier::Ctrl));
    let shift = action_pos(&kb, KeyAction::Modifier(Modifier::Shift));
    let c = text_pos(&kb, "c");
    let a = text_pos(&kb, "a");
    let enter = action_pos(&kb, KeyAction::Named(Named::Enter));

    press_at(&mut kb, ctrl);
    let ctrl_c = press_at(&mut kb, c).unwrap();
    assert_eq!(ctrl_c.key, Key::Character("c".into()));
    assert_eq!(ctrl_c.modifiers, Modifiers::CTRL);
    assert_eq!(encoded(&ctrl_c), Some(vec![3]));

    press_at(&mut kb, shift);
    let shift_a = press_at(&mut kb, a).unwrap();
    assert_eq!(encoded(&shift_a), Some(b"A".to_vec()));

    let enter_press = press_at(&mut kb, enter).unwrap();
    assert_eq!(encoded(&enter_press), Some(b"\r".to_vec()));
}

// --- view -----------------------------------------------------------------

#[derive(Debug, Clone)]
enum AppMessage {
    Osk(OskMessage),
}

#[test]
fn view_builds() {
    let mut kb = Keyboard::new(builtin_layout());
    let shift = action_pos(&kb, KeyAction::Modifier(Modifier::Shift));
    press_at(&mut kb, shift);
    let element: iced::Element<'_, AppMessage> = view(&kb, AppMessage::Osk);
    drop(element);
    let AppMessage::Osk(OskMessage::Press(r, c)) = AppMessage::Osk(OskMessage::Press(1, 2));
    assert_eq!((r, c), (1, 2));
}
