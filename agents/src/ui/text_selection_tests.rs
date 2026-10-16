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
        prompt_images: Vec::new(),
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
        background_job: None,
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
    let start = position(&buf, "⏺ Ran");
    let end = position(&buf, "对，");
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1));
    let message = view.rows[0].last_message.as_ref().unwrap();
    let expected = format!(
        "{}\n{}",
        message,
        &view.rows[0].prompt[..view.rows[0].prompt.find("对，").unwrap()]
    );
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1)),
        Some(Action::Copy {
            text: expected,
            label: "Copied selection".into()
        })
    );
}

fn list_fixture(count: usize) -> View {
    let mut view = fixture();
    let template = view.rows[0].clone();
    let rows = (0..count)
        .map(|index| {
            let mut row = template.clone();
            row.id.id = format!("selection-{index:02}");
            row.title = format!("{:02} {}", index, template.prompt.lines().next().unwrap());
            row.updated_at -= chrono::Duration::seconds(index as i64 * 60);
            row
        })
        .collect();
    view.set_sessions(rows, false);
    view.selected = view.visible_indices()[0];
    view
}

fn dashboard(view: &View, area: Rect) -> Buffer {
    let mut buf = Buffer::empty(area);
    view.render_at(
        area,
        &mut buf,
        view.rows[0].updated_at + chrono::Duration::seconds(60),
    );
    buf
}

/// Read occupied terminal cells, skipping the continuation cells of wide glyphs.
fn drawn_cells(buf: &Buffer, y: u16, from: u16, to: u16) -> String {
    let mut text = String::new();
    let mut x = from;
    while x < to {
        let symbol = buf[(x, y)].symbol();
        text.push_str(symbol);
        x += symbol.width().max(1) as u16;
    }
    text.trim_end().to_owned()
}

fn position_in(buf: &Buffer, text: &str, area: Rect) -> (u16, u16) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if drawn_cells(buf, y, x, area.right()).starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("text not drawn in panel: {text}");
}

fn copied_text(action: Option<Action>) -> String {
    match action {
        Some(Action::Copy { text, .. }) => text,
        other => panic!("missing selection copy: {other:?}"),
    }
}

#[test]
fn task_list_drag_copies_rendered_indicator_ellipsis_and_time() {
    let mut view = list_fixture(3);
    let area = Rect::new(0, 0, 160, 30);
    let initial = dashboard(&view, area);
    let list = view.viewport.borrow().list_rows;
    let start = position_in(&initial, "00 对，", list);
    let end = position_in(&initial, "02 对，", list);
    let start = (list.x, start.1);
    let end = (list.right(), end.1);
    let expected = (start.1..=end.1)
        .map(|y| drawn_cells(&initial, y, list.x, list.right()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(expected.contains('›'));
    assert!(expected.contains('…'));
    assert!(expected.contains("Working"));
    assert!(expected.contains("1m ago"));
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            start.0,
            start.1
        ))
        .is_none()
    );
    dashboard(&view, area);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1))
            .is_none()
    );
    let held = dashboard(&view, area);
    assert!(
        held[start]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    assert_eq!(
        copied_text(view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1))),
        expected
    );
    assert_eq!(dashboard(&view, area), initial);
}

fn path_log_fixture() -> View {
    let mut view = fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let events = crate::providers::claude::log::tail(&path, usize::MAX).unwrap();
    let mut event = events
        .into_iter()
        .find(|event| event.kind == crate::log::Kind::User)
        .unwrap();
    // Isolate a verbatim path from the real pasted command so abbreviation is visible.
    let source_path = event
        .body
        .split('"')
        .find(|part| part.starts_with('/'))
        .unwrap()
        .to_owned();
    event.body = source_path.clone();
    event.paths = std::iter::once(0..source_path.len()).collect();
    view.rows[0].transcript = Some(path);
    view.log_session = Some(view.rows[0].id.clone());
    view.log = vec![event];
    view
}

#[test]
fn log_list_drag_copies_abbreviated_path_as_drawn() {
    let mut view = path_log_fixture();
    let area = Rect::new(0, 0, 100, 30);
    let initial = dashboard(&view, area);
    let log = view.viewport.borrow().log;
    let y = log.y + super::log::LOG_TOP;
    let expected = drawn_cells(&initial, y, log.x, log.right());
    assert!(expected.contains('…'), "{expected}");
    for range in &view.log[0].paths {
        if range.len() > 60 {
            assert!(!expected.contains(&view.log[0].body[range.clone()]));
        }
    }
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), log.x, y))
            .is_none()
    );
    dashboard(&view, area);
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            log.right(),
            y
        ))
        .is_none()
    );
    dashboard(&view, area);
    assert!(
        view.log_open.is_none(),
        "drag returns to the list pressed originally"
    );
    assert_eq!(
        copied_text(view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            log.right(),
            y
        ))),
        expected
    );
}

#[test]
fn task_list_edge_drag_caches_and_copies_rows_beyond_first_screen() {
    let mut view = list_fixture(30);
    let area = Rect::new(0, 0, 160, 15);
    let initial = dashboard(&view, area);
    let list = view.viewport.borrow().list_rows;
    let start = position_in(&initial, "00 对，", list);
    let initial_first = view.viewport.borrow().first_entry;
    let mut rows = std::collections::BTreeMap::new();
    let remember =
        |view: &View, buf: &Buffer, rows: &mut std::collections::BTreeMap<usize, String>| {
            let viewport = view.viewport.borrow();
            for y in list.y..list.bottom() {
                rows.insert(
                    viewport.first_entry + usize::from(y - list.y),
                    drawn_cells(buf, y, list.x, list.right()),
                );
            }
        };
    remember(&view, &initial, &mut rows);
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list.x,
            start.1
        ))
        .is_none()
    );
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        list.right(),
        list.bottom(),
    ));
    dashboard(&view, area);
    for tick in 1..=5 {
        assert!(view.tick_text_selection(), "tick {tick} scrolls the list");
        let drawn = dashboard(&view, area);
        assert_eq!(view.viewport.borrow().first_entry, initial_first + tick);
        remember(&view, &drawn, &mut rows);
    }
    let last = view.viewport.borrow().first_entry + usize::from(list.height) - 1;
    let first = initial_first + usize::from(start.1 - list.y);
    let expected = (first..=last)
        .map(|row| rows[&row].clone())
        .collect::<Vec<_>>()
        .join("\n");
    let text = copied_text(view.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        list.right(),
        list.bottom(),
    )));
    assert_eq!(text, expected);
    assert!(text.lines().count() > usize::from(list.height));
    assert!(!view.tick_text_selection());
}

#[test]
fn task_list_click_and_double_click_keep_actions_without_copying() {
    let mut view = list_fixture(3);
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            resume: true,
            ..Default::default()
        },
    )]));
    let area = Rect::new(0, 0, 160, 30);
    let drawn = dashboard(&view, area);
    let at = position_in(&drawn, "01 对，", view.viewport.borrow().list_rows);
    let id = view
        .rows
        .iter()
        .find(|row| row.title.starts_with("01 "))
        .unwrap()
        .id
        .clone();
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert_ne!(view.selected_id(), Some(&id));
    dashboard(&view, area);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert_eq!(view.selected_id(), Some(&id));
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.0, at.1)),
        None
    );
    assert!(view.input_box.focused);
    assert_eq!(view.selected_id(), Some(&id));
}

#[test]
fn log_list_click_opens_cut_row_and_stationary_double_click_copies_nothing() {
    let mut view = path_log_fixture();
    let area = Rect::new(0, 0, 100, 30);
    dashboard(&view, area);
    let log = view.viewport.borrow().log;
    let at = (log.x, log.y + super::log::LOG_TOP);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert!(view.log_open.is_none());
    dashboard(&view, area);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert!(view.log_open.is_none());
    view.flush_pending_click(Instant::now() + view.double_click_interval);
    assert_eq!(view.log_open, Some(view.log[0].clone()));
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), at.0, at.1))
            .is_none()
    );
    assert!(view.log_open.is_some());
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Right), at.0, at.1))
            .is_none()
    );
    assert!(view.log_open.is_none());
}

#[test]
fn log_list_edge_drag_caches_rows_beyond_first_screen() {
    let mut view = fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let events = crate::providers::claude::log::tail(&path, usize::MAX).unwrap();
    view.rows[0].transcript = Some(path);
    view.log_session = Some(view.rows[0].id.clone());
    view.log = (0..30)
        .map(|index| {
            let mut event = events[index % events.len()].clone();
            event.started_at += chrono::Duration::seconds(index as i64);
            event
        })
        .collect();
    let area = Rect::new(0, 0, 100, 30);
    let initial = dashboard(&view, area);
    let log = view.viewport.borrow().log;
    let height = view.viewport.borrow().log_rows.len();
    let mut rows = std::collections::BTreeMap::new();
    let remember =
        |view: &View, buf: &Buffer, rows: &mut std::collections::BTreeMap<usize, String>| {
            let viewport = view.viewport.borrow();
            for offset in 0..viewport.log_rows.len() {
                rows.insert(
                    viewport.log_scroll + offset,
                    drawn_cells(
                        buf,
                        log.y + super::log::LOG_TOP + offset as u16,
                        log.x,
                        log.right(),
                    ),
                );
            }
        };
    remember(&view, &initial, &mut rows);
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        log.x,
        log.y + super::log::LOG_TOP,
    ));
    dashboard(&view, area);
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        log.right(),
        log.bottom(),
    ));
    dashboard(&view, area);
    for tick in 1..=5 {
        assert!(view.tick_text_selection(), "tick {tick} scrolls log rows");
        let drawn = dashboard(&view, area);
        assert_eq!(view.log_scroll(), tick);
        remember(&view, &drawn, &mut rows);
    }
    let expected = (0..height + 5)
        .map(|row| rows[&row].clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        copied_text(view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            log.right(),
            log.bottom()
        ))),
        expected
    );
    assert!(!view.tick_text_selection());
}

#[test]
fn task_list_stream_selection_trims_rows_and_never_splits_wide_cells() {
    let mut view = list_fixture(3);
    let area = Rect::new(0, 0, 160, 30);
    let initial = dashboard(&view, area);
    let list = view.viewport.borrow().list_rows;
    let first = position_in(&initial, "00 对，", list);
    let last = position_in(&initial, "02 对，", list);
    // Start in the continuation cell of 对, and end before 必 on the last row.
    let start = (first.0 + 4, first.1);
    let end = (last.0 + 7, last.1);
    let expected = [
        drawn_cells(&initial, first.1, first.0 + 3, list.right()),
        drawn_cells(&initial, first.1 + 1, list.x, list.right()),
        drawn_cells(&initial, last.1, list.x, end.0),
    ]
    .join("\n");
    assert!(
        expected.starts_with("对，"),
        "first={first:?} expected={expected:?}"
    );
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        start.0,
        start.1,
    ));
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1));
    let selected = dashboard(&view, area);
    assert!(
        selected[(start.0 - 1, start.1)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    assert!(
        selected[start]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    assert_eq!(
        copied_text(view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1))),
        expected
    );
}

#[test]
fn task_list_reverse_drag_copies_rendered_rows() {
    let mut view = list_fixture(3);
    let area = Rect::new(0, 0, 160, 30);
    let initial = dashboard(&view, area);
    let list = view.viewport.borrow().list_rows;
    let first = position_in(&initial, "00 对，", list);
    let last = position_in(&initial, "02 对，", list);
    let expected = (first.1..=last.1)
        .map(|y| drawn_cells(&initial, y, list.x, list.right()))
        .collect::<Vec<_>>()
        .join("\n");
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        list.right() - 1,
        last.1,
    ));
    let pressed = dashboard(&view, area);
    // Clicking the last row changes its marker; copy the cells actually drawn after the press.
    let expected = expected
        .lines()
        .enumerate()
        .map(|(offset, _)| {
            let y = first.1 + offset as u16;
            // The selection ends at the pressed cell's left boundary.
            let right = list.right() - u16::from(y == last.1);
            drawn_cells(&pressed, y, list.x, right)
        })
        .collect::<Vec<_>>()
        .join("\n");
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        list.x,
        first.1,
    ));
    dashboard(&view, area);
    let text = copied_text(view.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        list.x,
        first.1,
    )));
    assert_eq!(text, expected);
    assert!(!text.contains("Project"));
}

#[test]
fn dragging_across_other_task_rows_preserves_selected_task() {
    let mut view = list_fixture(3);
    let area = Rect::new(0, 0, 160, 30);
    let initial = dashboard(&view, area);
    let selected = view.selected_id().unwrap().clone();
    let list = view.viewport.borrow().list_rows;
    let start = position_in(&initial, "01 对，", list);
    let end = position_in(&initial, "02 对，", list);
    let expected = (start.1..=end.1)
        .map(|y| drawn_cells(&initial, y, list.x, list.right()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list.x,
            start.1
        ))
        .is_none()
    );
    assert_eq!(view.selected_id(), Some(&selected));
    dashboard(&view, area);
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        list.right(),
        end.1,
    ));
    dashboard(&view, area);
    assert_eq!(
        copied_text(view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            list.right(),
            end.1
        ))),
        expected
    );
    assert_eq!(view.selected_id(), Some(&selected));
    assert_eq!(dashboard(&view, area), initial);
}

#[test]
fn dragging_across_cut_log_rows_never_opens_an_event() {
    let mut view = path_log_fixture();
    view.log.push(view.log[0].clone());
    let area = Rect::new(0, 0, 100, 30);
    let initial = dashboard(&view, area);
    let log = view.viewport.borrow().log;
    let y = log.y + super::log::LOG_TOP;
    let expected = (y..=y + 1)
        .map(|row| drawn_cells(&initial, row, log.x, log.right()))
        .collect::<Vec<_>>()
        .join("\n");
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), log.x, y));
    assert!(view.log_open.is_none());
    dashboard(&view, area);
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        log.right(),
        y + 1,
    ));
    assert!(view.log_open.is_none());
    dashboard(&view, area);
    assert_eq!(
        copied_text(view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            log.right(),
            y + 1
        ))),
        expected
    );
    assert!(view.log_open.is_none());
    assert_eq!(dashboard(&view, area), initial);
}

#[test]
fn second_task_press_drag_is_not_a_double_click() {
    let mut view = list_fixture(3);
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            resume: true,
            ..Default::default()
        },
    )]));
    let area = Rect::new(0, 0, 160, 30);
    let drawn = dashboard(&view, area);
    let list = view.viewport.borrow().list_rows;
    let at = position_in(&drawn, "01 对，", list);
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1));
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    let selected = view.selected;
    let drawn = dashboard(&view, area);
    let expected = drawn_cells(&drawn, at.1, at.0, list.right());
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            list.right(),
            at.1
        ))
        .is_none()
    );
    assert_eq!(
        copied_text(view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            list.right(),
            at.1
        ))),
        expected
    );
    assert_eq!(view.selected, selected);
    assert!(view.last_click.is_none());
}

#[test]
fn right_drag_does_not_close_open_log() {
    let mut view = path_log_fixture();
    view.log_open = Some(view.log[0].clone());
    let event = view.log_open.clone();
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), 2, 20));
    assert_eq!(view.log_open, event);
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Right), 3, 20));
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Right), 3, 20));
    assert_eq!(view.log_open, event);
}

#[test]
fn right_click_closes_log_only_on_release() {
    let mut view = path_log_fixture();
    view.log_open = Some(view.log[0].clone());
    let event = view.log_open.clone();
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), 2, 20))
            .is_none()
    );
    assert_eq!(view.log_open, event);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Right), 2, 20))
            .is_none()
    );
    assert!(view.log_open.is_none());
}

#[test]
fn task_click_uses_press_position_and_completed_click_time() {
    let mut view = list_fixture(3);
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            resume: true,
            ..Default::default()
        },
    )]));
    let area = Rect::new(0, 0, 160, 30);
    let drawn = dashboard(&view, area);
    let list = view.viewport.borrow().list_rows;
    let at = position_in(&drawn, "01 对，", list);
    let other = position_in(&drawn, "02 对，", list);
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1));
    assert!(view.last_click.is_none());
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Left),
            other.0,
            other.1
        ))
        .is_none()
    );
    let id = view.selected_id().unwrap().clone();
    assert!(view.selected_row().unwrap().title.starts_with("01 "));
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1));
    view.last_click.as_mut().unwrap().1 -= DOUBLE_CLICK_INTERVAL;
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.0, at.1))
            .is_none()
    );
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), at.0, at.1));
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), at.0, at.1)),
        None
    );
    assert!(view.input_box.focused);
    assert_eq!(view.selected_id(), Some(&id));
}
