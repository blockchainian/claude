use super::*;

fn click(view: &mut View, button: MouseButton, x: u16, y: u16) -> Option<Action> {
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(button), x, y))
            .is_none()
    );
    view.handle_mouse(mouse(MouseEventKind::Up(button), x, y))
}

#[test]
fn raw_detail_right_click_keeps_event_and_scroll() {
    let (mut view, now) = log_fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    view.set_log(crate::providers::claude::log::tail_session(&path, 200, true).unwrap());
    view.select_log_event(Some(0));
    log_screen(&view, 200, 45, now);
    let selected = view.log_selected;
    view.viewport.borrow_mut().log_detail_scroll = 1;
    let area = view.viewport.borrow().log_detail;
    assert_eq!(
        click(&mut view, MouseButton::Right, area.x + 1, area.y + 1),
        None
    );
    assert_eq!(view.log_selected, selected);
    assert_eq!(view.viewport.borrow().log_detail_scroll, 1);
}

fn source_view(provider: ProviderId) -> (View, chrono::DateTime<Utc>, String) {
    let (mut view, now) = log_fixture();
    let filename = match provider {
        ProviderId::Claude => "claude-raw-detail.jsonl",
        ProviderId::Codex => "codex-raw-detail.jsonl",
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(filename);
    view.rows[view.selected].id.provider = provider;
    view.rows[view.selected].transcript = Some(path.clone());
    view.rows[view.selected].cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    view.log_session = Some(view.rows[view.selected].id.clone());
    let mut reader = crate::providers::log_reader(&view.rows[view.selected]).unwrap();
    view.set_log(reader.refresh(usize::MAX).unwrap().events);
    view.select_log_event(Some(0));
    let record: serde_json::Value = serde_json::from_str(
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let raw = serde_json::to_string_pretty(&record).unwrap();
    (view, now, raw)
}

fn body_position(view: &View) -> (u16, u16) {
    let viewport = view.viewport.borrow();
    let row = viewport
        .log_text
        .rows
        .iter()
        .find(|row| row.cells.len() >= 5)
        .expect("rendered body");
    (row.x + row.cells[0].0.start, row.y)
}

fn flush(view: &mut View) -> Option<Action> {
    view.flush_pending_click(Instant::now() + std::time::Duration::from_secs(1))
}

#[test]
fn raw_detail_single_click_toggles_after_double_click_deadline() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        let (mut view, now, expected) = source_view(provider);
        assert!(!view.log_detail_raw);
        log_screen(&view, 200, 45, now);
        let (x, y) = body_position(&view);
        assert_eq!(click(&mut view, MouseButton::Left, x, y), None);
        assert!(
            !view.log_detail_raw,
            "wait until a second click is excluded"
        );
        assert_eq!(flush(&mut view), None);
        assert!(view.log_detail_raw);
        let rendered = log_screen(&view, 200, 45, now);
        assert!(
            rendered.contains("\"message\"") || rendered.contains("\"payload\""),
            "pretty JSON must be rendered: {rendered}"
        );
        let actual = view
            .viewport
            .borrow()
            .log_text
            .sources
            .iter()
            .map(|source| source.text.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            actual.contains(&expected),
            "raw selection must use pretty-printed source JSON"
        );
        let (x, y) = body_position(&view);
        assert_eq!(click(&mut view, MouseButton::Left, x, y), None);
        assert_eq!(flush(&mut view), None);
        assert!(!view.log_detail_raw);
    }
}

#[test]
fn raw_detail_double_click_copies_current_mode_without_toggle() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        for raw in [false, true] {
            let (mut view, now, source) = source_view(provider);
            view.log_detail_raw = raw;
            let expected = if raw {
                source
            } else {
                log::open_text(view.visible_log_event().unwrap())
            };
            log_screen(&view, 200, 45, now);
            let (x, y) = body_position(&view);
            assert_eq!(click(&mut view, MouseButton::Left, x, y), None);
            let action = click(&mut view, MouseButton::Left, x, y);
            assert!(matches!(action, Some(Action::Copy { text, .. }) if text == expected));
            assert_eq!(view.log_detail_raw, raw);
            assert_eq!(flush(&mut view), None);
            assert_eq!(view.log_detail_raw, raw);
        }
    }
}

#[test]
fn raw_detail_mode_persists_when_event_selection_moves() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        let (mut view, now, _) = source_view(provider);
        view.log_detail_raw = true;
        log_screen(&view, 200, 45, now);
        let (x, y) = {
            let viewport = view.viewport.borrow();
            let row = viewport
                .log_rows
                .iter()
                .position(|index| *index == 1)
                .expect("second event in list");
            (viewport.log.right() - 1, viewport.log.y + row as u16)
        };
        assert_eq!(click(&mut view, MouseButton::Left, x, y), None);
        assert_eq!(view.log_selected, Some(1));
        assert!(view.log_detail_raw);
        let rendered = log_screen(&view, 200, 45, now);
        assert!(
            rendered.contains("\"message\"") || rendered.contains("\"payload\""),
            "next event stays raw: {rendered}"
        );
    }
}

#[test]
fn raw_detail_drag_copies_selection_without_toggle() {
    for raw in [false, true] {
        let (mut view, now, _) = source_view(ProviderId::Claude);
        view.log_detail_raw = raw;
        log_screen(&view, 200, 45, now);
        let (x, y, end) = {
            let viewport = view.viewport.borrow();
            let row = viewport
                .log_text
                .rows
                .iter()
                .find(|row| row.cells.len() >= 5)
                .unwrap();
            (
                row.x + row.cells[0].0.start,
                row.y,
                row.x + row.cells[4].0.end,
            )
        };
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
        view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end, y));
        assert!(
            matches!(view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end, y)), Some(Action::Copy { text, .. }) if !text.is_empty())
        );
        assert_eq!(flush(&mut view), None);
        assert_eq!(view.log_detail_raw, raw);
    }
}

#[test]
fn raw_detail_link_click_opens_path_without_toggle() {
    let (mut view, now, _) = source_view(ProviderId::Claude);
    log_screen(&view, 200, 100, now);
    let (rect, path) = {
        let viewport = view.viewport.borrow();
        viewport
            .log_links
            .iter()
            .find(|(rect, _)| viewport.log_detail.contains((rect.x, rect.y).into()))
            .expect("real prompt names src/app.rs")
            .clone()
    };
    assert_eq!(click(&mut view, MouseButton::Left, rect.x, rect.y), None);
    assert_eq!(flush(&mut view), Some(Action::OpenPath(path)));
    assert!(!view.log_detail_raw);
}

#[test]
fn raw_detail_top_button_scrolls_without_toggle() {
    for raw in [false, true] {
        let (mut view, now, _) = source_view(ProviderId::Claude);
        view.log_detail_raw = raw;
        log_screen(&view, 80, 25, now);
        view.viewport.borrow_mut().log_detail_scroll = usize::MAX;
        log_screen(&view, 80, 25, now);
        let top = view.viewport.borrow().log_detail_jump;
        assert!(top.width > 0);
        assert_eq!(click(&mut view, MouseButton::Left, top.x, top.y), None);
        assert_eq!(view.viewport.borrow().log_detail_scroll, 0);
        assert_eq!(flush(&mut view), None);
        assert_eq!(view.log_detail_raw, raw);
    }
}
