use super::*;

fn none() -> Modifiers {
    Modifiers::default()
}

fn key(k: Key, mods: Modifiers) -> Vec<u8> {
    encode_key(&k, mods, ModeFlags::default()).expect("encoded")
}

#[test]
fn arrows_follow_cursor_mode() {
    assert_eq!(key(Key::Up, none()), b"\x1b[A");
    let app = ModeFlags {
        app_cursor: true,
        ..ModeFlags::default()
    };
    assert_eq!(encode_key(&Key::Up, none(), app), Some(b"\x1bOA".to_vec()));
}

#[test]
fn modified_arrows_use_xterm_parameters() {
    let ctrl = Modifiers {
        ctrl: true,
        ..none()
    };
    assert_eq!(key(Key::Right, ctrl), b"\x1b[1;5C");
    let shift_alt = Modifiers {
        shift: true,
        alt: true,
        ..none()
    };
    assert_eq!(key(Key::Left, shift_alt), b"\x1b[1;4D");
}

#[test]
fn editing_and_function_keys() {
    assert_eq!(key(Key::Delete, none()), b"\x1b[3~");
    assert_eq!(
        key(
            Key::PageUp,
            Modifiers {
                shift: true,
                ..none()
            }
        ),
        b"\x1b[5;2~"
    );
    assert_eq!(key(Key::F(1), none()), b"\x1bOP");
    assert_eq!(key(Key::F(5), none()), b"\x1b[15~");
    assert_eq!(
        key(
            Key::F(12),
            Modifiers {
                ctrl: true,
                ..none()
            }
        ),
        b"\x1b[24;5~"
    );
    assert_eq!(encode_key(&Key::F(13), none(), ModeFlags::default()), None);
}

#[test]
fn control_characters() {
    let ctrl = Modifiers {
        ctrl: true,
        ..none()
    };
    assert_eq!(key(Key::Text("c".into()), ctrl), vec![3]);
    assert_eq!(key(Key::Text("C".into()), ctrl), vec![3]);
    assert_eq!(key(Key::Text("[".into()), ctrl), vec![0x1b]);
    assert_eq!(key(Key::Text(" ".into()), ctrl), vec![0]);
    assert_eq!(key(Key::Backspace, none()), vec![0x7f]);
    assert_eq!(
        key(
            Key::Tab,
            Modifiers {
                shift: true,
                ..none()
            }
        ),
        b"\x1b[Z"
    );
}

#[test]
fn alt_prefixes_escape_and_text_is_sent_once() {
    let alt = Modifiers {
        alt: true,
        ..none()
    };
    assert_eq!(key(Key::Text("x".into()), alt), b"\x1bx");
    assert_eq!(key(Key::Text("日本".into()), none()), "日本".as_bytes());
    assert_eq!(
        encode_key(&Key::Text(String::new()), none(), ModeFlags::default()),
        None
    );
}

#[test]
fn enter_honours_line_feed_mode() {
    assert_eq!(key(Key::Enter, none()), b"\r");
    let lnm = ModeFlags {
        line_feed_new_line: true,
        ..ModeFlags::default()
    };
    assert_eq!(encode_key(&Key::Enter, none(), lnm), Some(b"\r\n".to_vec()));
}

#[test]
fn bracketed_paste_wraps_and_rejects_terminator() {
    let mode = ModeFlags {
        bracketed_paste: true,
        ..ModeFlags::default()
    };
    assert_eq!(
        encode_paste("ls", mode),
        Ok(b"\x1b[200~ls\x1b[201~".to_vec())
    );
    assert_eq!(
        encode_paste("a\x1b[201~rm", mode),
        Err(PasteError::ContainsTerminator)
    );
    assert_eq!(
        encode_paste("plain", ModeFlags::default()),
        Ok(b"plain".to_vec())
    );
}

#[test]
fn oversized_paste_is_rejected_not_truncated() {
    let big = "x".repeat(MAX_PASTE_BYTES + 1);
    assert!(matches!(
        encode_paste(&big, ModeFlags::default()),
        Err(PasteError::TooLarge { .. })
    ));
}

#[test]
fn paste_risk_detection() {
    assert!(!paste_risk("echo hi\n").needs_confirmation());
    assert!(paste_risk("a\nb").multiline);
    assert!(paste_risk("a\x1b[31m").control_chars);
}

#[test]
fn mouse_reports_only_when_requested() {
    let off = ModeFlags::default();
    assert_eq!(
        encode_mouse(MouseButton::Left, MouseAction::Press, 0, 0, none(), off),
        None
    );
    let sgr = ModeFlags {
        mouse_report: true,
        sgr_mouse: true,
        ..ModeFlags::default()
    };
    assert_eq!(
        encode_mouse(MouseButton::Left, MouseAction::Press, 4, 2, none(), sgr),
        Some(b"\x1b[<0;5;3M".to_vec())
    );
    assert_eq!(
        encode_mouse(MouseButton::Left, MouseAction::Release, 4, 2, none(), sgr),
        Some(b"\x1b[<0;5;3m".to_vec())
    );
    assert_eq!(
        encode_mouse(MouseButton::Left, MouseAction::Drag, 4, 2, none(), sgr),
        None
    );
}

#[test]
fn legacy_mouse_encoding_clamps_coordinates() {
    let normal = ModeFlags {
        mouse_report: true,
        ..ModeFlags::default()
    };
    let bytes = encode_mouse(
        MouseButton::WheelUp,
        MouseAction::Press,
        500,
        0,
        none(),
        normal,
    );
    assert_eq!(bytes, Some(vec![0x1b, b'[', b'M', 32 + 64, 255, 33]));
}
