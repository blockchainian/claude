use super::*;

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

fn fixture() -> View {
    let mut view = View::new(Grouping::Project, false);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == crate::log::Kind::Assistant && event.body.starts_with("对，"))
        .unwrap();
    let row = Session {
        id: SessionKey {
            provider: ProviderId::Claude,
            id: "selection".into(),
        },
        parent: None,
        cwd: std::env::temp_dir(),
        name: None,
        title: "Selection".into(),
        prompt: event.body,
        queued: Vec::new(),
        prompt_at: None,
        turn_messages: 0,
        last_record_at: None,
        model: None,
        effort: None,
        git_branch: None,
        pr: None,
        status: SessionStatus::Working,
        process: None,
        updated_at: event.started_at,
        started_at: None,
        last_message: None,
        last_message_at: None,
        activity: None,
        pending_questions: Vec::new(),
        background: Vec::new(),
        last_prompt: None,
        transcript: None,
    };
    view.set_sessions(vec![row], false);
    view
}

fn details(view: &View) -> Buffer {
    let area = Rect::new(2, 1, 30, 80);
    let mut buf = Buffer::empty(Rect::new(0, 0, 40, 90));
    view.render_details(area, &mut buf, Utc::now());
    buf
}

fn position(buf: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let mut row = String::new();
            let mut column = x;
            while column < buf.area.width {
                let symbol = buf[(column, y)].symbol();
                row.push_str(symbol);
                column += symbol.width().max(1) as u16;
            }
            if row.starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("text not drawn: {text}");
}

#[test]
fn drag_soft_wrapped_body_copies_source_without_wrap_newlines() {
    let mut view = fixture();
    let buf = details(&view);
    let start = position(&buf, "对，");
    let end = position(&buf, "不过");
    assert!(end.1 > start.1);
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            start.0,
            start.1
        ))
        .is_none()
    );
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1))
            .is_none()
    );
    let source = &view.rows[0].prompt;
    let expected = source[..source.find("不过").unwrap()].to_owned();
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1)),
        Some(Action::Copy {
            text: expected,
            label: "Copied prompt".into()
        })
    );
}

#[test]
fn stationary_click_copies_nothing() {
    let mut view = fixture();
    let buf = details(&view);
    let (x, y) = position(&buf, "对，");
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y))
            .is_none()
    );
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x, y))
            .is_none()
    );
    assert_eq!(details(&view), buf);
}

#[test]
fn release_clears_the_highlight() {
    let mut view = fixture();
    let buf = details(&view);
    let start = position(&buf, "对，");
    let end = position(&buf, "不过");
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1));
    assert!(
        details(&view)[start]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1));
    assert_eq!(details(&view), buf);
}

#[test]
fn full_dashboard_highlights_dragged_fenced_prompt() {
    let mut view = fixture();
    let records = include_str!("../../tests/fixtures/transcript.jsonl");
    let record: serde_json::Value = serde_json::from_str(records.lines().next().unwrap()).unwrap();
    view.rows[0].last_message = Some(view.rows[0].prompt.clone());
    view.rows[0].prompt = record["message"]["content"].as_str().unwrap()[..512].to_owned();
    view.rows[0].transcript =
        Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl"));
    let area = Rect::new(0, 0, 120, 42);
    let render = |view: &View| {
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, view.rows[0].updated_at);
        buf
    };
    let initial = render(&view);
    let start = position(&initial, "Stop hook error: codex-manager: 2");
    let end = position(&initial, "thread(s) need attention");
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        end.0 + 4,
        end.1,
    ));
    for _ in 0..2 {
        let held = render(&view);
        assert!(
            held[start]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        );
        assert!(
            initial
                .diff(&held)
                .iter()
                .any(|(x, y, _)| (*x, *y) == start)
        );
    }
}

#[test]
fn edge_drag_scrolls_and_copies_beyond_the_first_screen() {
    let mut view = fixture();
    let area = Rect::new(2, 1, 30, 10);
    let render = |view: &View| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        view.render_details(area, &mut buf, Utc::now());
        buf
    };
    let first = render(&view);
    let start = position(&first, "对，");
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        area.right(),
        area.bottom(),
    ));
    for _ in 0..10 {
        assert!(view.tick_text_selection());
        render(&view);
    }
    let action = view.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        area.right(),
        area.bottom(),
    ));
    let Some(Action::Copy { text, .. }) = action else {
        panic!("missing copy: {action:?}");
    };
    assert!(view.rows[0].prompt.starts_with(&text));
    assert!(text.contains("界面上显示成"), "{text}");
    assert!(!view.tick_text_selection());
}

#[test]
fn open_log_drag_scrolls_in_both_directions_and_clears_on_release() {
    let mut view = fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.body == view.rows[0].prompt)
        .unwrap();
    let source = super::log::open_text(&event);
    view.log_session = Some(view.rows[0].id.clone());
    view.log_open = Some(event);
    let area = Rect::new(2, 1, 30, 10);
    view.viewport.borrow_mut().log = area;
    let render = |view: &View| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 20));
        view.render_log(area, &mut buf, Utc::now());
        buf
    };
    let first = render(&view);
    let start = position(&first, "对，");
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        area.right(),
        area.bottom(),
    ));
    for _ in 0..10 {
        assert!(view.tick_text_selection());
        render(&view);
    }
    assert_eq!(view.viewport.borrow().log_open_scroll, 10);
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        area.x,
        area.y,
    ));
    assert!(view.tick_text_selection());
    render(&view);
    assert_eq!(view.viewport.borrow().log_open_scroll, 9);
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        area.right(),
        area.bottom(),
    ));
    render(&view);
    let Some(Action::Copy { text, .. }) = view.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        area.right(),
        area.bottom(),
    )) else {
        panic!("missing copy");
    };
    assert!(source.starts_with(&text));
    assert!(text.contains("界面上显示成"));
    assert!(
        !render(&view)
            .content
            .iter()
            .any(|cell| cell.modifier.contains(ratatui::style::Modifier::REVERSED))
    );
    assert!(!view.tick_text_selection());
}

#[test]
fn drag_between_bodies_joins_original_text_with_one_newline() {
    let mut view = fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == crate::log::Kind::User)
        .unwrap();
    view.rows[0].last_message = Some(event.body);
    let buf = details(&view);
    let start = position(&buf, "对，");
    let end = position(&buf, "⏺ Ran");
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1));
    let message = view.rows[0].last_message.as_ref().unwrap();
    let expected = format!(
        "{}\n{}",
        view.rows[0].prompt,
        &message[..message.find("⏺ Ran").unwrap()]
    );
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1)),
        Some(Action::Copy {
            text: expected,
            label: "Copied selection".into()
        })
    );
}
