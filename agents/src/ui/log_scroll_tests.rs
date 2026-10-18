use super::*;
use crate::providers::Provider;

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn fixture(long: bool) -> (View, DateTime<Utc>) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_str!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.log_session = view.selected_id().cloned();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let mut event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == crate::log::Kind::Assistant)
        .unwrap();
    let now = event.started_at;
    let mut events = Vec::new();
    for (index, body) in [
        "before".to_owned(),
        if long {
            (1..=30)
                .map(|i| format!("detail row {i:02}"))
                .collect::<Vec<_>>()
                .join("  \n")
        } else {
            "short detail".into()
        },
        "after".to_owned(),
    ]
    .into_iter()
    .enumerate()
    {
        event.source = Some(format!("scroll-{index}"));
        event.body = body;
        events.push(event.clone());
    }
    view.set_log(events);
    view.select_log_event(Some(1));
    view.mouse_focus = Panel::LogDetail;
    view.log_detail_focused = true;
    view.input_box.focused = false;
    (view, now)
}

fn panel(view: &View, now: DateTime<Utc>) -> String {
    panel_at(view, now, 10)
}

fn panel_at(view: &View, now: DateTime<Utc>, height: u16) -> String {
    let screen = Rect::new(0, 0, 120, 30);
    let area = Rect::new(0, 10, 120, height);
    view.viewport.borrow_mut().log_detail = area;
    let mut buffer = Buffer::empty(screen);
    view.render_log_detail(area, &mut buffer, now);
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn wheel(view: &mut View, forward: bool) {
    view.handle_mouse(mouse(
        if forward {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        10,
        15,
    ));
}

#[test]
fn log_scroll_short_content_moves_one_event_per_wheel() {
    let (mut view, now) = fixture(false);
    panel(&view, now);
    wheel(&mut view, true);
    assert_eq!(view.log_selected, Some(2));
    panel(&view, now);
    wheel(&mut view, false);
    assert_eq!(view.log_selected, Some(1));
}

#[test]
fn log_scroll_long_content_reveals_then_moves_at_both_boundaries() {
    let (mut view, now) = fixture(true);
    let initial = panel(&view, now);
    assert!(
        initial.contains('…'),
        "long body must start folded: {initial}"
    );
    wheel(&mut view, true);
    let scrolled = panel(&view, now);
    assert_eq!(view.log_selected, Some(1));
    assert_ne!(scrolled, initial);
    for _ in 0..60 {
        wheel(&mut view, true);
        let text = panel(&view, now);
        if view.log_selected == Some(2) {
            assert!(text.contains("after"));
            break;
        }
    }
    assert_eq!(view.log_selected, Some(2));
    // Returning to an unchanged entry restores its open curtain, like task details.
    wheel(&mut view, false);
    let returned = panel(&view, now);
    assert_eq!(view.log_selected, Some(1));
    assert!(returned.contains("detail row 30"), "{returned}");
    for _ in 0..60 {
        wheel(&mut view, false);
        panel(&view, now);
        if view.log_selected == Some(0) {
            break;
        }
    }
    assert_eq!(view.log_selected, Some(0));
}

#[test]
fn log_scroll_keyboard_uses_list_bindings_inside_detail() {
    let (mut view, now) = fixture(true);
    let initial = panel(&view, now);
    view.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(view.log_selected, Some(1));
    assert_ne!(panel(&view, now), initial);
    view.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    assert_eq!(panel(&view, now), initial);
    view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert_eq!(view.log_selected, Some(0));
    panel(&view, now);
    view.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert_eq!(view.log_selected, Some(1));
}

fn tool_fixture() -> (View, DateTime<Utc>) {
    let (mut view, now) = fixture(true);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let mut event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| !event.calls.is_empty())
        .unwrap();
    event.calls.truncate(1);
    let mut call = event.calls[0].clone();
    call.detail_left = "Second tool".into();
    call.detail_title = "Second tool".into();
    event.calls[0].input = vec![crate::log::Section::Text {
        label: String::new(),
        text: (1..=20)
            .map(|i| format!("first body {i:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }];
    event.calls[0].output = None;
    call.input = vec![crate::log::Section::Text {
        label: String::new(),
        text: "second body".into(),
    }];
    call.output = None;
    event.calls.push(call);
    view.set_log(vec![event]);
    view.select_log_event(Some(0));
    (view, now)
}

#[test]
fn log_scroll_sections_reveal_under_pointer_and_keep_later_sections_visible() {
    let (mut view, now) = tool_fixture();
    let before = panel_at(&view, now, 14);
    assert!(before.contains("Second tool"), "{before}");
    assert!(before.contains('…'));
    let pointer = view.viewport.borrow().log_detail_state.content_top
        + view.viewport.borrow().log_detail_state.bodies[0].start as u16;
    view.scroll_content_row(true, true, pointer);
    let after = panel_at(&view, now, 14);
    assert_ne!(before, after);
    assert_eq!(view.viewport.borrow().log_detail_state.reveal, vec![1, 0]);
    assert!(after.contains("Second tool"), "{after}");
    assert!(after.contains("second body"), "{after}");
}

#[test]
fn log_scroll_changed_body_folds_when_returning() {
    let (mut view, now) = fixture(true);
    let initial = panel(&view, now);
    wheel(&mut view, true);
    panel(&view, now);
    view.select_log_event(Some(2));
    panel(&view, now);
    view.log[1].body = view.log[1].body.replace("detail row", "changed row");
    view.select_log_event(Some(1));
    let changed = panel(&view, now);
    assert!(changed.contains("changed row 01"));
    assert!(changed.contains('…'));
    assert_eq!(view.viewport.borrow().log_detail_scroll, 0);
    assert_ne!(changed, initial);
}

#[test]
fn log_scroll_keeps_scrolling_window_when_pointer_section_is_exhausted() {
    let (mut view, now) = tool_fixture();
    panel_at(&view, now, 14);
    assert_eq!(view.viewport.borrow().log_detail_state.hidden[1], 0);
    let pointer = {
        let viewport = view.viewport.borrow();
        viewport.log_detail_state.content_top + viewport.log_detail_state.bodies[1].start as u16
    };
    view.scroll_content_row(true, true, pointer);
    panel_at(&view, now, 14);
    assert_eq!(view.viewport.borrow().log_detail_state.reveal, vec![1, 0]);
}

#[test]
fn log_scroll_visible_running_section_keeps_animating_after_folded_section() {
    let (view, now) = tool_fixture();
    let text = panel_at(&view, now, 14);
    assert!(text.contains("Second tool"));
    assert!(text.contains(super::tool_detail::running_frame(now)));
    assert!(view.tool_redraw_in(now).is_some());
}

#[test]
fn log_scroll_tiny_window_restores_plain_offset() {
    let (mut view, now) = fixture(true);
    panel_at(&view, now, 3);
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, 10, 12));
    panel_at(&view, now, 3);
    let offset = view.viewport.borrow().log_detail_scroll;
    assert!(offset > 0);
    view.select_log_event(Some(2));
    panel_at(&view, now, 3);
    view.select_log_event(Some(1));
    panel_at(&view, now, 3);
    assert_eq!(view.viewport.borrow().log_detail_scroll, offset);
}

#[test]
fn log_scroll_same_source_keeps_position_when_event_kind_changes() {
    let (mut view, now) = fixture(true);
    panel(&view, now);
    wheel(&mut view, true);
    panel(&view, now);
    let offset = view.viewport.borrow().log_detail_scroll;
    view.select_log_event(Some(2));
    panel(&view, now);
    let mut events = view.log.clone();
    events[1].kind = crate::log::Kind::Error;
    view.set_log(events);
    view.select_log_event(Some(1));
    panel(&view, now);
    assert_eq!(view.viewport.borrow().log_detail_scroll, offset);
}
