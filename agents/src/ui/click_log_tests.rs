use super::*;

fn real_log_view() -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = log_fixture();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    view.set_log(crate::providers::claude::log::tail_session(&path, 200, true).unwrap());
    assert!(!view.log.is_empty());
    (view, now)
}

fn release_click(view: &mut View, button: MouseButton, x: u16, y: u16) -> Option<Action> {
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(button), x, y))
            .is_none()
    );
    view.handle_mouse(mouse(MouseEventKind::Up(button), x, y))
}

fn with_background() -> (View, chrono::DateTime<Utc>, tempfile::NamedTempFile) {
    use std::io::Write;
    let (mut view, now) = real_log_view();
    let mut output = tempfile::NamedTempFile::new().unwrap();
    output
        .write_all(include_bytes!(
            "../../tests/fixtures/claude-peer-message.txt"
        ))
        .unwrap();
    view.rows[view.selected]
        .background
        .push(crate::model::BackgroundTask {
            description: "Real background output".into(),
            output: output.path().into(),
            started_at: now,
        });
    (view, now, output)
}

fn open_background(view: &mut View, now: chrono::DateTime<Utc>) {
    let screen = log_screen(view, 160, 40, now);
    let (x, y) = rendered_cell(&screen, "Real background output");
    assert_eq!(release_click(view, MouseButton::Left, x, y), None);
    assert_eq!(release_click(view, MouseButton::Left, x, y), None);
    assert!(
        view.log_open.is_some(),
        "Background double click must open its full log"
    );
    assert!(view.log_open.as_ref().unwrap().body.contains("globalState"));
    assert_eq!(view.pending_click_in(Instant::now()), None);
}

#[test]
fn background_double_click_returns_to_previous_list() {
    let (mut view, now, _output) = with_background();
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(view.log_open.is_none());
}

#[test]
fn background_double_click_restores_previous_detail_and_scroll() {
    let (mut view, now, _output) = with_background();
    let previous = view
        .log
        .iter()
        .max_by_key(|event| log::open_text(event).len())
        .unwrap()
        .clone();
    view.set_log_open(Some(previous.clone()));
    log_screen(&view, 160, 40, now);
    view.viewport.borrow_mut().log_open_scroll = 2;
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.log_open.as_ref(), Some(&previous));
    assert_eq!(view.viewport.borrow().log_open_scroll, 2);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(view.log_open.is_none());
}

#[test]
fn open_detail_keeps_its_event_across_refresh_and_log_scroll() {
    let (mut view, now) = real_log_view();
    let previous = view.log.first().unwrap().clone();
    view.set_log_open(Some(previous.clone()));
    log_screen(&view, 100, 30, now);
    let mut refreshed = view.log.clone();
    refreshed.extend(view.log.clone());
    view.set_log(refreshed);
    let area = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, area.x + 22, area.y + 3));
    assert_eq!(view.log_open.as_ref(), Some(&previous));
    release_click(&mut view, MouseButton::Right, area.x + 22, area.y + 3);
    assert!(view.log_open.is_none());
}

#[test]
fn detail_ctrl_r_search_jumps_and_highlights_without_closing() {
    let (mut view, now, _output) = with_background();
    open_background(&mut view, now);
    let expected = view.log_open.clone();
    log_screen(&view, 100, 25, now);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(view.log_open, expected, "Ctrl+R must keep the open event");
    for ch in "globalState".chars() {
        assert_eq!(key(&mut view, KeyCode::Char(ch)), None);
    }
    assert!(view.log_detail_search.editing);
    assert!(
        !view.log_search.active(),
        "detail search must not filter the list"
    );
    let screen = log_screen(&view, 100, 25, now);
    let (x, y) = rendered_cell(&screen, "globalState");
    assert!(
        view.viewport.borrow().log_open_scroll > 0,
        "search must reveal the matching line"
    );
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(buffer.area, &mut buffer, now);
    assert!(
        buffer[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "matched text must be highlighted"
    );
    assert_eq!(view.log_open, expected);
    key(&mut view, KeyCode::Enter);
    assert!(!view.log_detail_search.editing);
    assert_eq!(view.log_open, expected);
}

#[test]
fn right_click_cancels_innermost_modal_before_open_detail() {
    let (mut view, _now) = real_log_view();
    let event = view.log[0].clone();
    view.set_log_open(Some(event.clone()));
    view.rename_target = view.selected_id().cloned();
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(view.rename_target.is_none());
    assert_eq!(view.log_open.as_ref(), Some(&event));
    view.help = true;
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(!view.help);
    assert_eq!(view.log_open.as_ref(), Some(&event));
    view.searching = true;
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(!view.searching);
    assert_eq!(view.log_open.as_ref(), Some(&event));
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert!(view.log_open.is_none());
}

#[test]
fn background_detail_survives_refresh_with_matching_tool_timestamp() {
    let (mut view, now, _output) = with_background();
    let tool_time = view
        .log
        .iter()
        .find(|event| event.kind == crate::log::Kind::Tool)
        .unwrap()
        .started_at;
    view.rows[view.selected].background[0].started_at = tool_time;
    open_background(&mut view, now);
    let expected = view.log_open.clone();
    assert_eq!(expected.as_ref().unwrap().started_at, tool_time);
    view.set_log(view.log.clone());
    assert_eq!(
        view.log_open, expected,
        "transcript Tool events must not replace a Background detail sharing their timestamp"
    );
}

#[test]
fn background_return_restores_detail_search() {
    let (mut view, now, _output) = with_background();
    let previous = view.log[0].clone();
    view.set_log_open(Some(previous.clone()));
    let text = log::open_text(&previous);
    let query = text.split_whitespace().next().unwrap();
    view.log_detail_search.query.set_text(query);
    open_background(&mut view, now);
    release_click(&mut view, MouseButton::Right, 1, 1);
    assert_eq!(view.log_open.as_ref(), Some(&previous));
    assert_eq!(view.log_detail_search.query.text(), query);
}
