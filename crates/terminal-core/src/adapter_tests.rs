use super::*;

fn model(cols: u16, rows: u16) -> TerminalModel {
    TerminalModel::new(
        SessionId::new(1, 1),
        TermSize::new(cols, rows),
        Limits::default(),
    )
}

fn feed(model: &mut TerminalModel, bytes: &[u8]) -> (Vec<ModelEvent>, Vec<Vec<u8>>) {
    model.advance(bytes)
}

#[test]
fn plain_text_lands_in_grid() {
    let mut m = model(10, 3);
    feed(&mut m, b"hello\r\nworld");
    let snap = m.snapshot();
    assert_eq!(snap.visible_text(), "hello\nworld\n\n");
    assert_eq!(snap.cursor.map(|c| (c.row, c.col)), Some((1, 5)));
}

#[test]
fn sgr_colors_and_attributes_are_symbolic() {
    let mut m = model(10, 1);
    feed(
        &mut m,
        b"\x1b[1;31mA\x1b[0;38;2;1;2;3mB\x1b[38;5;200mC\x1b[7mD",
    );
    let snap = m.snapshot();
    let a = snap.cell(0, 0).expect("cell");
    assert_eq!(a.fg, CellColor::Indexed(1));
    assert!(a.flags.contains(CellFlags::BOLD));
    assert_eq!(snap.cell(0, 1).map(|c| c.fg), Some(CellColor::Rgb(1, 2, 3)));
    assert_eq!(snap.cell(0, 2).map(|c| c.fg), Some(CellColor::Indexed(200)));
    assert!(
        snap.cell(0, 3)
            .is_some_and(|c| c.flags.contains(CellFlags::INVERSE))
    );
}

#[test]
fn arbitrary_chunking_yields_identical_grid() {
    let stream = "a\x1b[31mé中文\x1b[0m\r\n\x1b[2;3Hx\x1b]0;title\x07e\u{301}z".as_bytes();
    let mut whole = model(12, 4);
    feed(&mut whole, stream);
    let expected = whole.snapshot();
    for chunk in 1..=stream.len() {
        let mut split = model(12, 4);
        for piece in stream.chunks(chunk) {
            feed(&mut split, piece);
        }
        let got = split.snapshot();
        assert_eq!(got.cells, expected.cells, "chunk size {chunk}");
        assert_eq!(got.cursor, expected.cursor, "chunk size {chunk}");
    }
}

#[test]
fn wide_characters_occupy_two_cells() {
    let mut m = model(6, 1);
    feed(&mut m, "中a".as_bytes());
    let snap = m.snapshot();
    assert!(
        snap.cell(0, 0)
            .is_some_and(|c| c.flags.contains(CellFlags::WIDE))
    );
    assert!(
        snap.cell(0, 1)
            .is_some_and(|c| c.flags.contains(CellFlags::WIDE_SPACER))
    );
    assert_eq!(
        snap.cell(0, 2).map(|c| c.text.clone()),
        Some(CellText::Char('a'))
    );
    assert_eq!(snap.visible_text(), "中a\n");
}

#[test]
fn combining_marks_form_a_cluster() {
    let mut m = model(6, 1);
    feed(&mut m, "e\u{301}x".as_bytes());
    let snap = m.snapshot();
    assert_eq!(
        snap.cell(0, 0).map(|c| c.text.clone()),
        Some(CellText::Cluster("e\u{301}".into()))
    );
    assert_eq!(
        snap.cell(0, 1).map(|c| c.text.clone()),
        Some(CellText::Char('x'))
    );
}

#[test]
fn titles_are_sanitized_and_bounded() {
    let mut m = model(10, 1);
    let long = "t".repeat(10_000);
    let (events, _) = feed(&mut m, format!("\x1b]2;a\u{202E}b{long}\x07").as_bytes());
    let Some(ModelEvent::Title(Some(title))) = events.first() else {
        panic!("expected title event, got {events:?}");
    };
    assert!(title.starts_with("ab"));
    assert!(title.len() <= Limits::default().max_title_bytes);
}

#[test]
fn osc52_clipboard_requests_are_denied() {
    let mut m = model(10, 1);
    let (events, replies) = feed(&mut m, b"\x1b]52;c;aGVsbG8=\x07\x1b]52;c;?\x07");
    assert!(
        replies.is_empty(),
        "clipboard must never be answered: {replies:?}"
    );
    assert!(events.iter().all(|e| !matches!(e, ModelEvent::Title(_))));
}

#[test]
fn device_status_report_produces_reply() {
    let mut m = model(10, 3);
    feed(&mut m, b"\x1b[2;4H");
    let (_, replies) = feed(&mut m, b"\x1b[6n");
    assert_eq!(replies, vec![b"\x1b[2;4R".to_vec()]);
}

#[test]
fn color_query_uses_palette_when_set() {
    let mut m = model(10, 1);
    let (events, replies) = feed(&mut m, b"\x1b]11;?\x07");
    assert!(replies.is_empty());
    assert_eq!(events, vec![ModelEvent::Denied("color-query")]);
    let mut palette = [(0, 0, 0); 18];
    palette[17] = (0x10, 0x18, 0x20);
    m.set_query_palette(palette);
    let (_, replies) = feed(&mut m, b"\x1b]11;?\x07");
    let reply = String::from_utf8(replies.concat()).expect("utf8");
    assert!(reply.contains("rgb:1010/1818/2020"), "{reply:?}");
}

#[test]
fn alternate_screen_restores_main_buffer() {
    let mut m = model(10, 2);
    feed(&mut m, b"main");
    feed(&mut m, b"\x1b[?1049h\x1b[2J\x1b[Halt");
    let alt = m.snapshot();
    assert!(alt.mode.alt_screen);
    assert!(alt.visible_text().starts_with("alt"));
    feed(&mut m, b"\x1b[?1049l");
    let main = m.snapshot();
    assert!(!main.mode.alt_screen);
    assert!(main.visible_text().starts_with("main"));
}

#[test]
fn resize_advances_epoch_and_keeps_cursor_in_bounds() {
    let mut m = model(20, 5);
    feed(&mut m, b"\x1b[5;20Hx");
    let before = m.epoch();
    for (cols, rows) in [(5, 2), (80, 24), (2, 1), (13, 7)] {
        let epoch = m.resize(TermSize::new(cols, rows));
        assert!(epoch > before);
        let snap = m.snapshot();
        assert_eq!(snap.damage, Damage::Full);
        assert_eq!(snap.cells.len(), usize::from(cols) * usize::from(rows));
        if let Some(cursor) = snap.cursor {
            assert!(cursor.row < rows && cursor.col < cols);
        }
    }
}

#[test]
fn damage_is_partial_after_small_update() {
    let mut m = model(10, 5);
    m.snapshot();
    feed(&mut m, b"\x1b[3;1Hz");
    let snap = m.snapshot();
    match snap.damage {
        Damage::Rows(rows) => assert!(rows.contains(&2)),
        Damage::Full => panic!("expected partial damage"),
    }
}

#[test]
fn scrollback_is_bounded_by_limits() {
    let limits = Limits {
        scrollback_lines: 50,
        ..Limits::default()
    };
    let mut m = TerminalModel::new(SessionId::new(1, 1), TermSize::new(10, 3), limits);
    for i in 0..500 {
        feed(&mut m, format!("{i}\r\n").as_bytes());
    }
    assert!(m.snapshot().history_lines <= 50);
}

#[test]
fn selection_extracts_text_and_spans_viewport() {
    let mut m = model(10, 2);
    feed(&mut m, b"hello world");
    let at = |col| GridPoint {
        row: 0,
        col,
        right_half: false,
    };
    m.start_selection(at(0), SelectionKind::Simple);
    m.update_selection(GridPoint {
        row: 0,
        col: 4,
        right_half: true,
    });
    assert_eq!(m.selection_text().as_deref(), Some("hello"));
    let span = m.snapshot().selection.expect("selection");
    assert_eq!((span.start, span.end), ((0, 0), (0, 4)));
    m.clear_selection();
    assert!(m.snapshot().selection.is_none());
}

#[test]
fn literal_search_finds_history_matches_case_insensitively() {
    let mut m = model(20, 3);
    for i in 0..10 {
        feed(&mut m, format!("line {i} Needle\r\n").as_bytes());
    }
    let result = m.search_literal("needle", SnapshotVersion(1));
    assert_eq!(result.matches.len(), 10);
    assert!(!result.truncated);
    assert!(result.matches[0].line < 0, "first match is in history");
    assert_eq!(
        (result.matches[0].start_col, result.matches[0].end_col),
        (7, 12)
    );
}

#[test]
fn search_results_are_capped() {
    let limits = Limits {
        max_search_results: 3,
        ..Limits::default()
    };
    let mut m = TerminalModel::new(SessionId::new(1, 1), TermSize::new(20, 3), limits);
    feed(&mut m, b"x x x x x x");
    let result = m.search_literal("x", SnapshotVersion(1));
    assert_eq!(result.matches.len(), 3);
    assert!(result.truncated);
}

#[test]
fn scrolling_changes_display_offset() {
    let mut m = model(10, 2);
    for i in 0..20 {
        feed(&mut m, format!("{i}\r\n").as_bytes());
    }
    m.scroll(5);
    assert_eq!(m.snapshot().display_offset, 5);
    m.scroll_to_bottom();
    assert_eq!(m.snapshot().display_offset, 0);
}

#[test]
fn malformed_utf8_does_not_break_following_text() {
    let mut m = model(10, 1);
    feed(&mut m, b"\xff\xfe\xc3ok");
    assert!(m.snapshot().visible_text().contains("ok"));
}

#[test]
fn overlong_osc_is_discarded_through_terminator() {
    let mut m = model(10, 1);
    let mut stream = b"\x1b]2;".to_vec();
    stream.extend(std::iter::repeat_n(b'A', 200_000));
    stream.extend_from_slice(b"\x07ok");
    feed(&mut m, &stream);
    assert!(m.snapshot().visible_text().starts_with("ok"));
}
