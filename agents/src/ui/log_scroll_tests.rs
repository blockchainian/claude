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

#[test]
fn log_scroll_chinese_json_round_trip() {
    for height in 3..20 {
        let (mut view, now) = tool_fixture();
        view.log[0].calls.truncate(1);
        view.log[0].calls[0].input = vec![crate::log::Section::Prose {
            markdown: false,
            text: "请检查会话面板的滚动行为，确保标题和内容都能完整显示。".repeat(80),
        }];
        view.log[0].calls[0].output = Some(vec![crate::log::Section::Structured {
            label: String::new(),
            text: serde_json::json!({"status":"ok", "results": (0..30).collect::<Vec<_>>()})
                .to_string(),
        }]);
        let initial = panel_at(&view, now, height);
        for _ in 0..10 {
            view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            panel_at(&view, now, height);
        }
        for step in 0..10 {
            let before = view.viewport.borrow().log_detail_scroll;
            view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            panel_at(&view, now, height);
            assert!(
                view.viewport.borrow().log_detail_scroll < before,
                "height {height}, step {step}, before {before}"
            );
        }
        assert_eq!(panel_at(&view, now, height), initial, "height {height}");
    }
}

#[test]
fn log_scroll_new_rows_do_not_consume_short_entry_navigation() {
    for new in [0, 1, 2, 3, 7] {
        for detail in [false, true] {
            let (mut view, now) = fixture(false);
            let template = view.log[1].clone();
            let events = (0..20)
                .map(|i| {
                    let mut event = template.clone();
                    event.source = Some(format!("short-{i}"));
                    event.body = format!("short item {i}");
                    event
                })
                .collect();
            view.set_log(events);
            view.select_log_event(Some(12));
            view.viewport.borrow_mut().log_unseen = 0;
            let draw = |view: &View| {
                let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
                view.viewport.borrow_mut().log = Rect::new(0, 0, 120, 9);
                view.viewport.borrow_mut().log_scroll = 9;
                view.render_log(Rect::new(0, 0, 120, 9), &mut buf, now);
                panel(view, now);
            };
            draw(&view);
            let mut events = view.log.clone();
            events.extend((0..new).map(|i| {
                let mut event = template.clone();
                event.source = Some(format!("new-{i}"));
                event
            }));
            view.set_log(events);
            draw(&view);
            assert_eq!(view.viewport.borrow().log_unseen, new);
            for forward in [true, false] {
                let index = view.log_selected.unwrap();
                view.handle_mouse(mouse(
                    if forward {
                        MouseEventKind::ScrollDown
                    } else {
                        MouseEventKind::ScrollUp
                    },
                    10,
                    if detail { 15 } else { 3 },
                ));
                draw(&view);
                assert_eq!(
                    view.log_selected,
                    Some(if forward { index + 1 } else { index - 1 }),
                    "new={new}, detail={detail}"
                );
            }
        }
    }
}

#[test]
fn log_scroll_wheel_pauses_at_title_before_changing_event() {
    let (mut view, now) = fixture(true);
    panel(&view, now);
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let scrolled = panel(&view, now);
    assert_eq!(view.viewport.borrow().log_detail_scroll, 1);
    wheel(&mut view, false);
    assert_eq!(
        view.log_selected,
        Some(1),
        "first wheel must restore the title: {scrolled}"
    );
    panel(&view, now);
    assert_eq!(view.viewport.borrow().log_detail_scroll, 0);
    wheel(&mut view, false);
    panel(&view, now);
    assert_eq!(view.log_selected, Some(1), "the title holds one wheel");
    wheel(&mut view, false);
    assert_eq!(view.log_selected, Some(0));
}

#[test]
fn log_scroll_short_entry_uses_its_layout_during_live_input_batch() {
    for (new, forward) in [(2, false), (2, true), (3, false), (3, true)] {
        let (mut view, now) = fixture(true);
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/background/two-awaits.jsonl");
        view.log[2] = crate::providers::claude::log::tail(&path, usize::MAX)
            .unwrap()
            .remove(0);
        let mut events = view.log.clone();
        let mut tail = events[0].clone();
        tail.source = Some("tail".into());
        events.push(tail);
        view.set_log(events);
        view.viewport.borrow_mut().log_unseen = 0;
        panel(&view, now);
        wheel(&mut view, true);
        panel(&view, now);
        // The app drains input and LogLoaded events before drawing the next frame.
        view.move_log_selection(true);
        let mut events = view.log.clone();
        for i in 0..new {
            let mut event = events[0].clone();
            event.source = Some(format!("new-{i}"));
            events.push(event);
        }
        view.set_log(events);
        assert_eq!(view.viewport.borrow().log_unseen, new);
        assert_eq!(view.log_selected, Some(2));
        wheel(&mut view, forward);
        assert_eq!(
            view.log_selected,
            Some(if forward { 3 } else { 1 }),
            "short entry must leave on first wheel, new={new}"
        );
    }
}

#[test]
fn log_scroll_wheel_stops_at_bottom_before_changing_event() {
    let (mut view, now) = fixture(true);
    panel(&view, now);
    let max = view.viewport.borrow().log_detail_max;
    for _ in 0..max - 1 {
        view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        panel(&view, now);
    }
    wheel(&mut view, true);
    panel(&view, now);
    assert_eq!(view.log_selected, Some(1));
    assert_eq!(view.viewport.borrow().log_detail_scroll, max);
    wheel(&mut view, true);
    assert_eq!(view.log_selected, Some(2));
}

#[test]
fn log_scroll_wheel_entering_from_above_pauses_at_title_and_leaves_bottom_at_once() {
    let (mut view, now) = fixture(true);
    for _ in 0..2 {
        view.select_log_event(Some(0));
        panel(&view, now);
        wheel(&mut view, true);
        panel(&view, now);
        assert_eq!(view.log_selected, Some(1));
        assert_eq!(view.viewport.borrow().log_detail_scroll, 0);
        wheel(&mut view, true);
        panel(&view, now);
        assert_eq!(
            view.viewport.borrow().log_detail_scroll,
            0,
            "the title holds one wheel"
        );
        let max = view.viewport.borrow().log_detail_max;
        while view.viewport.borrow().log_detail_scroll < max {
            wheel(&mut view, true);
            panel(&view, now);
            assert_eq!(view.log_selected, Some(1));
        }
        // Reentering an entry left at its bottom starts at its title again.
        wheel(&mut view, true);
        assert_eq!(view.log_selected, Some(2));
    }
}

#[test]
fn log_scroll_wheel_entering_from_below_starts_at_bottom() {
    let (mut view, now) = fixture(true);
    view.select_log_event(Some(2));
    panel(&view, now);
    wheel(&mut view, false);
    panel(&view, now);
    assert_eq!(view.log_selected, Some(1));
    let max = view.viewport.borrow().log_detail_max;
    assert!(max > 0);
    assert_eq!(view.viewport.borrow().log_detail_scroll, max);
    while view.viewport.borrow().log_detail_scroll > 0 {
        wheel(&mut view, false);
        panel(&view, now);
        assert_eq!(view.log_selected, Some(1));
    }
    wheel(&mut view, false);
    panel(&view, now);
    assert_eq!(view.log_selected, Some(1), "the title holds one wheel");
    wheel(&mut view, false);
    assert_eq!(view.log_selected, Some(0));
}

#[test]
fn log_scroll_wheel_reversing_at_title_does_not_pause() {
    let (mut view, now) = fixture(true);
    view.select_log_event(Some(0));
    panel(&view, now);
    wheel(&mut view, true);
    panel(&view, now);
    wheel(&mut view, false);
    assert_eq!(view.log_selected, Some(0));
}
