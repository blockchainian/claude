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
    view.event_session = view.selected_id().cloned();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let mut event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == crate::event::Kind::Assistant)
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
    view.set_event(events);
    view.select_event(Some(1));
    view.active_panel = Panel::EventDetail;
    (view, now)
}

fn panel(view: &View, now: DateTime<Utc>) -> String {
    panel_at(view, now, 10)
}

fn panel_at(view: &View, now: DateTime<Utc>, height: u16) -> String {
    let screen = Rect::new(0, 0, 120, 30);
    let area = Rect::new(0, 10, 120, height);
    view.viewport.borrow_mut().event_detail = area;
    let mut buffer = Buffer::empty(screen);
    view.render_event_detail(area, &mut buffer, now);
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
fn sending_preserves_scrolled_event_detail_after_append() {
    for images in [false, true] {
        let (mut view, now) = fixture(true);
        panel(&view, now);
        for _ in 0..3 {
            wheel(&mut view, true);
            panel(&view, now);
        }
        let before = panel(&view, now);
        let scroll = view.event_detail.scroll.borrow().offset;
        assert!(scroll > 0);
        let id = view.selected_id().unwrap().clone();
        let input = if images {
            crate::providers::SessionInput::PromptWithImages {
                text: "next".into(),
                images: Vec::new(),
            }
        } else {
            crate::providers::SessionInput::Prompt("next".into())
        };
        view.prepare_prompt();
        view.input_sending(&id, &input);
        view.input_complete(&id, &input, Ok(()));
        let mut events = view.event_panel.clone();
        events.push(events.last().unwrap().clone());
        view.set_event(events);
        assert_eq!(view.event_list.cursor, Some(1));
        assert_eq!(view.event_detail.scroll.borrow().offset, scroll);
        assert_eq!(panel(&view, now), before);
    }
}

#[test]
fn event_scroll_short_content_moves_one_event_per_wheel() {
    let (mut view, now) = fixture(false);
    panel(&view, now);
    wheel(&mut view, true);
    assert_eq!(view.event_list.cursor, Some(2));
    panel(&view, now);
    wheel(&mut view, false);
    assert_eq!(view.event_list.cursor, Some(1));
}

#[test]
fn event_scroll_long_content_reveals_then_moves_at_both_boundaries() {
    let (mut view, now) = fixture(true);
    let initial = panel(&view, now);
    assert!(
        initial.contains('…'),
        "long body must start folded: {initial}"
    );
    wheel(&mut view, true);
    let scrolled = panel(&view, now);
    assert_eq!(view.event_list.cursor, Some(1));
    assert_ne!(scrolled, initial);
    for _ in 0..60 {
        wheel(&mut view, true);
        let text = panel(&view, now);
        if view.event_list.cursor == Some(2) {
            assert!(text.contains("after"));
            break;
        }
    }
    assert_eq!(view.event_list.cursor, Some(2));
    // Returning to an unchanged entry restores its open curtain, like task details.
    wheel(&mut view, false);
    let returned = panel(&view, now);
    assert_eq!(view.event_list.cursor, Some(1));
    assert!(returned.contains("detail row 30"), "{returned}");
    for _ in 0..60 {
        wheel(&mut view, false);
        panel(&view, now);
        if view.event_list.cursor == Some(0) {
            break;
        }
    }
    assert_eq!(view.event_list.cursor, Some(0));
}

#[test]
fn event_scroll_keyboard_uses_list_bindings_inside_event_detail() {
    let (mut view, now) = fixture(true);
    let initial = panel(&view, now);
    view.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(view.event_list.cursor, Some(1));
    assert_ne!(panel(&view, now), initial);
    view.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    assert_eq!(panel(&view, now), initial);
    view.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert_eq!(view.event_list.cursor, Some(1));
    assert_ne!(panel(&view, now), initial);
    view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert_eq!(view.event_list.cursor, Some(1));
    assert_eq!(panel(&view, now), initial);
}

#[test]
fn event_scroll_keyboard_changes_event_at_boundaries_like_the_wheel() {
    for (down, up) in [
        (KeyCode::Char('j'), KeyCode::Char('k')),
        (KeyCode::PageDown, KeyCode::PageUp),
    ] {
        let (mut view, now) = fixture(false);
        panel(&view, now);
        view.handle_key(KeyEvent::new(down, KeyModifiers::NONE));
        assert_eq!(view.event_list.cursor, Some(2), "{down:?}");
        panel(&view, now);
        view.handle_key(KeyEvent::new(up, KeyModifiers::NONE));
        assert_eq!(view.event_list.cursor, Some(1), "{up:?}");

        let (mut view, now) = fixture(true);
        panel(&view, now);
        for _ in 0..60 {
            view.handle_key(KeyEvent::new(down, KeyModifiers::NONE));
            panel(&view, now);
            if view.event_list.cursor == Some(2) {
                break;
            }
        }
        assert_eq!(view.event_list.cursor, Some(2), "{down:?}");
    }
}

fn tool_fixture() -> (View, DateTime<Utc>) {
    let (mut view, now) = fixture(true);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let mut event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| !event.calls.is_empty())
        .unwrap();
    event.calls.truncate(1);
    let mut call = event.calls[0].clone();
    call.name = "Second tool".into();
    call.title = "Second tool".into();
    event.calls[0].input = vec![crate::event::Section::Text {
        label: String::new(),
        text: (1..=20)
            .map(|i| format!("first body {i:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }];
    event.calls[0].output = None;
    call.input = vec![crate::event::Section::Text {
        label: String::new(),
        text: "second body".into(),
    }];
    call.output = None;
    event.calls.push(call);
    view.set_event(vec![event]);
    view.select_event(Some(0));
    (view, now)
}

#[test]
fn event_scroll_multiple_sections_keep_full_bodies_and_scroll_as_document() {
    let (mut view, now) = tool_fixture();
    let before = panel_at(&view, now, 14);
    let bodies = view.event_detail.scroll.borrow().state.bodies.clone();
    assert!(bodies[0].len() >= 20, "first body was cut: {bodies:?}");
    assert!(!before.contains('…'), "body must not be folded: {before}");
    assert!(!before.contains("Second tool"), "{before}");
    assert!(view.event_detail.scroll.borrow().state.hidden.is_empty());

    wheel(&mut view, true);
    let after = panel_at(&view, now, 14);
    assert_eq!(view.event_detail.scroll.borrow().offset, 3);
    assert_eq!(view.event_detail.scroll.borrow().state.reveal, [0, 0]);
    assert_eq!(view.event_detail.scroll.borrow().state.bodies, bodies);
    let before_rows: Vec<_> = before.lines().collect();
    let after_rows: Vec<_> = after.lines().collect();
    assert_eq!(after_rows[1..10], before_rows[4..13]);
    wheel(&mut view, false);
    assert_eq!(panel_at(&view, now, 14), before);

    let mut seen = after;
    for _ in 0..40 {
        if seen.contains("Second tool") {
            break;
        }
        wheel(&mut view, true);
        seen = panel_at(&view, now, 14);
    }
    assert!(seen.contains("Second tool"), "{seen}");
    assert!(seen.contains("first body 20"), "{seen}");
}

#[test]
fn event_scroll_sections_move_together_under_pointer() {
    let (mut view, now) = tool_fixture();
    let before = panel_at(&view, now, 14);
    assert!(!before.contains("Second tool"), "{before}");
    assert!(!before.contains('…'));
    let pointer = view.event_detail.scroll.borrow().state.content_top
        + view.event_detail.scroll.borrow().state.bodies[0].start as u16;
    view.scroll_content_row(true, true, pointer);
    let after = panel_at(&view, now, 14);
    assert_ne!(before, after);
    assert_eq!(view.event_detail.scroll.borrow().state.reveal, vec![0, 0]);
    assert_eq!(view.event_detail.scroll.borrow().offset, 1);
    assert!(!after.contains("Second tool"), "{after}");
}

#[test]
fn event_scroll_changed_body_folds_when_returning() {
    let (mut view, now) = fixture(true);
    let initial = panel(&view, now);
    wheel(&mut view, true);
    panel(&view, now);
    view.select_event(Some(2));
    panel(&view, now);
    view.event_panel[1].body = view.event_panel[1]
        .body
        .replace("detail row", "changed row");
    view.select_event(Some(1));
    let changed = panel(&view, now);
    assert!(changed.contains("changed row 01"));
    assert!(changed.contains('…'));
    assert_eq!(view.event_detail.scroll.borrow().offset, 0);
    assert_ne!(changed, initial);
}

#[test]
fn event_scroll_later_section_uses_document_offset() {
    let (mut view, now) = tool_fixture();
    panel_at(&view, now, 14);
    assert!(view.event_detail.scroll.borrow().state.hidden.is_empty());
    let max = view.viewport.borrow().event_detail_max;
    view.event_detail.scroll.borrow_mut().offset = max - 1;
    panel_at(&view, now, 14);
    let pointer = {
        let detail = view.event_detail.scroll.borrow();
        detail.state.content_top + (detail.state.bodies[1].start - detail.offset) as u16
    };
    view.scroll_content_row(true, true, pointer);
    panel_at(&view, now, 14);
    assert_eq!(view.event_detail.scroll.borrow().state.reveal, vec![0, 0]);
    assert_eq!(view.event_detail.scroll.borrow().offset, max);
}

#[test]
fn event_scroll_visible_running_section_keeps_animating_after_scrolled_section() {
    let (view, now) = tool_fixture();
    panel_at(&view, now, 14);
    view.event_detail.scroll.borrow_mut().offset = usize::MAX;
    let text = panel_at(&view, now, 14);
    assert!(text.contains("Second tool"));
    assert!(text.contains(super::tool_detail::running_frame(now)));
    assert!(view.tool_redraw_in(now).is_some());
}

#[test]
fn event_scroll_tiny_window_restores_plain_offset() {
    let (mut view, now) = fixture(true);
    panel_at(&view, now, 3);
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, 10, 12));
    panel_at(&view, now, 3);
    let offset = view.event_detail.scroll.borrow().offset;
    assert!(offset > 0);
    view.select_event(Some(2));
    panel_at(&view, now, 3);
    view.select_event(Some(1));
    panel_at(&view, now, 3);
    assert_eq!(view.event_detail.scroll.borrow().offset, offset);
}

#[test]
fn event_scroll_same_source_keeps_position_when_event_kind_changes() {
    let (mut view, now) = fixture(true);
    panel(&view, now);
    wheel(&mut view, true);
    panel(&view, now);
    let offset = view.event_detail.scroll.borrow().offset;
    view.select_event(Some(2));
    panel(&view, now);
    let mut events = view.event_panel.clone();
    events[1].kind = crate::event::Kind::Error;
    view.set_event(events);
    view.select_event(Some(1));
    panel(&view, now);
    assert_eq!(view.event_detail.scroll.borrow().offset, offset);
}

#[test]
fn event_scroll_chinese_json_round_trip() {
    for height in 3..20 {
        let (mut view, now) = tool_fixture();
        view.event_panel[0].calls.truncate(1);
        view.event_panel[0].calls[0].input = vec![crate::event::Section::Prose {
            markdown: false,
            text: "请检查会话面板的滚动行为，确保标题和内容都能完整显示。".repeat(80),
        }];
        view.event_panel[0].calls[0].output = Some(vec![crate::event::Section::Structured {
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
            let before = view.event_detail.scroll.borrow().offset;
            view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            panel_at(&view, now, height);
            assert!(
                view.event_detail.scroll.borrow().offset < before,
                "height {height}, step {step}, before {before}"
            );
        }
        assert_eq!(panel_at(&view, now, height), initial, "height {height}");
    }
}

#[test]
fn event_scroll_new_rows_do_not_consume_short_entry_navigation() {
    for new in [0, 1, 2, 3, 7] {
        for detail in [false, true] {
            let (mut view, now) = fixture(false);
            let template = view.event_panel[1].clone();
            let events = (0..20)
                .map(|i| {
                    let mut event = template.clone();
                    event.source = Some(format!("short-{i}"));
                    event.body = format!("short item {i}");
                    event
                })
                .collect();
            view.set_event(events);
            view.select_event(Some(12));
            view.event_list.unseen.set(0);
            let draw = |view: &View| {
                let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
                view.viewport.borrow_mut().event_panel = Rect::new(0, 0, 120, 9);
                view.event_list.scroll.set(9);
                view.render_event(Rect::new(0, 0, 120, 9), &mut buf, now);
                panel(view, now);
            };
            draw(&view);
            let mut events = view.event_panel.clone();
            events.extend((0..new).map(|i| {
                let mut event = template.clone();
                event.source = Some(format!("new-{i}"));
                event
            }));
            view.set_event(events);
            draw(&view);
            assert_eq!(view.event_list.unseen.get(), new);
            for forward in [true, false] {
                let index = view.event_list.cursor.unwrap();
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
                    view.event_list.cursor,
                    Some(if forward { index + 1 } else { index - 1 }),
                    "new={new}, detail={detail}"
                );
            }
        }
    }
}

#[test]
fn event_scroll_wheel_leaves_from_the_title_at_once() {
    let (mut view, now) = fixture(true);
    panel(&view, now);
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let scrolled = panel(&view, now);
    assert_eq!(view.event_detail.scroll.borrow().offset, 1);
    wheel(&mut view, false);
    assert_eq!(
        view.event_list.cursor,
        Some(1),
        "first wheel must restore the title: {scrolled}"
    );
    panel(&view, now);
    assert_eq!(view.event_detail.scroll.borrow().offset, 0);
    wheel(&mut view, false);
    assert_eq!(view.event_list.cursor, Some(0));
}

#[test]
fn event_scroll_short_entry_uses_its_layout_during_live_input_batch() {
    for (new, forward) in [(2, false), (2, true), (3, false), (3, true)] {
        let (mut view, now) = fixture(true);
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/background/two-awaits.jsonl");
        view.event_panel[2] = crate::providers::claude::event_reader::tail(&path, usize::MAX)
            .unwrap()
            .remove(0);
        let mut events = view.event_panel.clone();
        let mut tail = events[0].clone();
        tail.source = Some("tail".into());
        events.push(tail);
        view.set_event(events);
        view.event_list.unseen.set(0);
        panel(&view, now);
        wheel(&mut view, true);
        panel(&view, now);
        // The app drains input and EventsLoaded events before drawing the next frame.
        view.move_event_selection(true);
        let mut events = view.event_panel.clone();
        for i in 0..new {
            let mut event = events[0].clone();
            event.source = Some(format!("new-{i}"));
            events.push(event);
        }
        view.set_event(events);
        assert_eq!(view.event_list.unseen.get(), new);
        assert_eq!(view.event_list.cursor, Some(2));
        wheel(&mut view, forward);
        assert_eq!(
            view.event_list.cursor,
            Some(if forward { 3 } else { 1 }),
            "short entry must leave on first wheel, new={new}"
        );
    }
}

#[test]
fn event_scroll_wheel_stops_at_bottom_before_changing_event() {
    let (mut view, now) = fixture(true);
    panel(&view, now);
    let max = view.viewport.borrow().event_detail_max;
    for _ in 0..max - 1 {
        view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        panel(&view, now);
    }
    wheel(&mut view, true);
    panel(&view, now);
    assert_eq!(view.event_list.cursor, Some(1));
    assert_eq!(view.event_detail.scroll.borrow().offset, max);
    wheel(&mut view, true);
    assert_eq!(view.event_list.cursor, Some(2));
}

#[test]
fn event_scroll_wheel_entering_from_above_scrolls_and_leaves_bottom_at_once() {
    let (mut view, now) = fixture(true);
    for _ in 0..2 {
        view.select_event(Some(0));
        panel(&view, now);
        wheel(&mut view, true);
        panel(&view, now);
        assert_eq!(view.event_list.cursor, Some(1));
        assert_eq!(view.event_detail.scroll.borrow().offset, 0);
        wheel(&mut view, true);
        panel(&view, now);
        assert!(view.event_detail.scroll.borrow().offset > 0);
        let max = view.viewport.borrow().event_detail_max;
        while view.event_detail.scroll.borrow().offset < max {
            wheel(&mut view, true);
            panel(&view, now);
            assert_eq!(view.event_list.cursor, Some(1));
        }
        // Reentering an entry left at its bottom starts at its title again.
        wheel(&mut view, true);
        assert_eq!(view.event_list.cursor, Some(2));
    }
}

#[test]
fn event_scroll_wheel_entering_from_below_starts_at_bottom() {
    let (mut view, now) = fixture(true);
    view.select_event(Some(2));
    panel(&view, now);
    wheel(&mut view, false);
    panel(&view, now);
    assert_eq!(view.event_list.cursor, Some(1));
    let max = view.viewport.borrow().event_detail_max;
    assert!(max > 0);
    assert_eq!(view.event_detail.scroll.borrow().offset, max);
    while view.event_detail.scroll.borrow().offset > 0 {
        wheel(&mut view, false);
        panel(&view, now);
        assert_eq!(view.event_list.cursor, Some(1));
    }
    wheel(&mut view, false);
    assert_eq!(view.event_list.cursor, Some(0));
}

#[test]
fn event_scroll_wheel_reversing_at_title_does_not_pause() {
    let (mut view, now) = fixture(true);
    view.select_event(Some(0));
    panel(&view, now);
    wheel(&mut view, true);
    panel(&view, now);
    wheel(&mut view, false);
    assert_eq!(view.event_list.cursor, Some(0));
}

#[test]
fn event_sort_key_only_changes_the_active_event_list() {
    let (mut view, _) = fixture(false);
    assert!(view.active_panel == Panel::EventDetail);
    let action = view.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    assert_eq!(action, None);
    assert_eq!(view.event_list.sort, EventSort::Time);
    view.set_active_panel(Panel::EventList);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)),
        Some(Action::PersistSorting {
            id: view.selected_id().unwrap().clone(),
            sorting: EventSort::Duration,
        })
    );
}

fn screen_rows(view: &View, width: u16, height: u16) -> Vec<String> {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    view.render(area, &mut buffer);
    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect()
}

fn event_detail_wheel(view: &mut View, forward: bool) {
    let detail = view.viewport.borrow().event_detail;
    view.handle_mouse(mouse(
        if forward {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        detail.x + 2,
        detail.y + 2,
    ));
}

/// The display column where `needle` starts in `row`.
fn column_of(row: &str, needle: &str) -> Option<usize> {
    row.find(needle).map(|byte| row[..byte].width())
}

#[test]
fn event_detail_top_edge_shows_only_a_scrolled_off_title() {
    for width in [90, 120, 160] {
        let (mut view, _) = fixture(true);
        let rows = screen_rows(&view, width, 40);
        let y = usize::from(view.viewport.borrow().output_borders[0].y);
        assert!(!rows[y].contains("Assistant"));
        event_detail_wheel(&mut view, true);
        let rows = screen_rows(&view, width, 40);
        assert_eq!(
            column_of(&rows[y], "Assistant"),
            Some((usize::from(width) - 11) / 2 + 1)
        );
        assert!(!rows.last().unwrap().contains("Assistant"));
        event_detail_wheel(&mut view, false);
        let rows = screen_rows(&view, width, 40);
        assert!(!rows[y].contains("Assistant"));
    }
}

#[test]
fn footer_feedback_preserves_the_section_title_on_its_edge() {
    for width in [100, 160] {
        let (mut view, _) = fixture(true);
        screen_rows(&view, width, 40);
        event_detail_wheel(&mut view, true);
        let titled = screen_rows(&view, width, 40);
        let y = usize::from(view.viewport.borrow().output_borders[0].y);
        assert!(titled[y].contains("Assistant"));
        view.show_copied("Copied message".into(), std::time::Instant::now());
        let rows = screen_rows(&view, width, 40);
        assert!(rows.last().unwrap().trim_end().ends_with("Copied message"));
        assert_eq!(rows[y], titled[y]);
    }
}

#[test]
fn event_detail_top_edge_tracks_the_section_on_the_top_row() {
    let (mut view, _) = tool_fixture();
    let mut events = view.event_panel.clone();
    events[0].calls[0].input = vec![crate::event::Section::Text {
        label: String::new(),
        text: "first body".into(),
    }];
    events[0].calls[1].input = vec![crate::event::Section::Text {
        label: String::new(),
        text: (1..=40)
            .map(|i| format!("second body {i:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }];
    let first = events[0].calls[0].title.replace(' ', "");
    view.set_event(events);
    view.select_event(Some(0));
    screen_rows(&view, 100, 40);
    let event_detail_y = usize::from(view.viewport.borrow().event_detail.y);
    let mut footers = Vec::new();
    for _ in 0..20 {
        event_detail_wheel(&mut view, true);
        let rows = screen_rows(&view, 100, 40);
        let detail = rows[event_detail_y + 1..rows.len() - 1].join("\n");
        let footer = rows[usize::from(view.viewport.borrow().output_borders[0].y)].clone();
        if detail.contains("Second tool") {
            assert!(!footer.contains("Second tool"), "{footer}");
        } else {
            assert!(footer.contains("Second tool"), "{detail}\n{footer}");
        }
        footers.push(footer);
    }
    // Wide glyphs read back with a blank continuation cell.
    assert!(
        footers
            .iter()
            .any(|footer| footer.replace(' ', "").contains(&first)),
        "{first}"
    );
    assert!(footers.iter().any(|footer| footer.contains("Second tool")));
}

#[test]
fn event_detail_top_edge_title_preserves_the_panel_title_style() {
    let (mut view, _) = fixture(true);
    let area = Rect::new(0, 0, 100, 40);
    let draw = |view: &View| {
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        buffer
    };
    let initial = draw(&view);
    let detail = view.viewport.borrow().event_detail;
    let title_cell = (detail.y..detail.bottom())
        .find_map(|y| {
            let row = (0..area.width)
                .map(|x| initial[(x, y)].symbol())
                .collect::<String>();
            column_of(&row, "Assistant").map(|x| initial[(x as u16, y)].clone())
        })
        .expect("the title is visible inside the panel");
    assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));
    assert_ne!(title_cell.fg, ratatui::style::Color::Reset);
    event_detail_wheel(&mut view, true);
    let buffer = draw(&view);
    let y = view.viewport.borrow().output_borders[0].y;
    let edge = (0..area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>();
    let x = column_of(&edge, "Assistant").expect("the title scrolled onto the edge") as u16;
    assert_eq!(buffer[(x - 1, y)].symbol(), " ");
    assert_eq!(buffer[(x + 9, y)].symbol(), " ");
    assert_eq!(buffer[(x - 2, y)].symbol(), "─");
    assert_eq!(buffer[(x + 10, y)].symbol(), "─");
    for x in x..x + "Assistant".len() as u16 {
        let cell = &buffer[(x, y)];
        assert_eq!(cell.fg, title_cell.fg);
        assert_eq!(cell.modifier, title_cell.modifier);
    }
}

fn recorded_tool_edge(forward: bool) {
    for width in [200, 149, 120, 100] {
        let (mut view, _) = fixture(false);
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
        let event = crate::providers::claude::event_reader::tail(&path, usize::MAX)
            .unwrap()
            .into_iter()
            .find(|event| event.calls.first().is_some_and(|call| call.name == "Bash"))
            .unwrap();
        let title = event.calls[0].title.clone();
        view.set_event(vec![event]);
        view.select_event(Some(0));
        let initial = screen_rows(&view, width, 30);
        assert!(!initial.last().unwrap().contains(&title));
        if !forward {
            let path =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
            let after = crate::providers::claude::event_reader::tail(&path, usize::MAX)
                .unwrap()
                .into_iter()
                .find(|event| event.kind == crate::event::Kind::Assistant && event.calls.is_empty())
                .unwrap();
            let event = view.event_panel[0].clone();
            view.set_event(vec![event, after]);
            view.select_event(Some(1));
            screen_rows(&view, width, 30);
        }
        event_detail_wheel(&mut view, forward);
        let rows = screen_rows(&view, width, 30);
        let viewport = view.viewport.borrow();
        let footer = &rows[usize::from(viewport.output_borders[0].y)];
        assert!(view.event_detail.scroll.borrow().offset > 0, "{rows:?}");
        let compact = footer.replace(' ', "");
        assert!(compact.contains(&title.replace(' ', "")), "{footer}");
        let first = title.chars().next().unwrap().to_string();
        let x = column_of(footer, &first).unwrap();
        assert_eq!(
            x,
            (usize::from(width) - title.width() - 2) / 2 + 1,
            "{footer}"
        );
        assert!(!rows.last().unwrap().contains(&title));
    }
}

#[test]
fn event_detail_recorded_top_edge_scrolling_down() {
    recorded_tool_edge(true);
}

#[test]
fn event_detail_recorded_top_edge_scrolling_up() {
    recorded_tool_edge(false);
}

#[test]
fn output_border_top_double_click_scrolls_without_changing_events() {
    output_border_jump(false);
}

#[test]
fn output_border_bottom_double_click_scrolls_without_changing_events() {
    output_border_jump(true);
}

fn output_border_jump(bottom: bool) {
    for width in [100, 149, 150, 200] {
        for height in [20, 45] {
            let (mut view, now) = fixture(true);
            let draw = |view: &View| {
                let area = Rect::new(0, 0, width, height);
                let mut buffer = Buffer::empty(area);
                view.render_at(area, &mut buffer, now);
                buffer
            };
            draw(&view);
            view.set_active_panel(Panel::EventDetail);
            view.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
            let buffer = draw(&view);
            let before = view.event_detail.scroll.borrow().offset;
            let max = view.viewport.borrow().event_detail_max;
            assert!(
                before > 0 && before < max,
                "{width}x{height}: {before}/{max}"
            );
            let selected = view.event_list.cursor;
            let event_scroll = view.event_list.scroll.get();
            let output = view.viewport.borrow().event_detail;
            let y = if bottom { height - 2 } else { output.y };
            // The medium layout shares this rule with the event list.
            let x = view.viewport.borrow().event_panel.x;
            if bottom {
                assert_eq!(buffer[(0, y)].symbol(), "─", "missing lower rule");
                assert_eq!(output.bottom(), y);
            }
            view.set_active_panel(Panel::TaskList);
            for _ in 0..2 {
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    assert_eq!(view.handle_mouse(mouse(kind, x, y)), None);
                }
            }
            draw(&view);
            assert_eq!(
                view.event_list.cursor, selected,
                "border changed event at {width}x{height}, bottom={bottom}"
            );
            assert_eq!(view.event_list.scroll.get(), event_scroll);
            assert!(view.active_panel == Panel::EventDetail);
            assert_eq!(
                view.event_detail.scroll.borrow().offset,
                if bottom { max } else { 0 }
            );
        }
    }
}

#[test]
fn output_border_terminal_without_scrollback_only_activates_output() {
    for width in [100, 200] {
        let (mut view, now) = fixture(true);
        let id = view.selected_id().unwrap().clone();
        let screen = std::sync::Arc::new(std::sync::Mutex::new(
            crate::session_terminal::TerminalScreen::new(6, 80),
        ));
        screen.lock().unwrap().process(b"terminal contents");
        view.show_terminal(id, screen.clone());
        let area = Rect::new(0, 0, width, 30);
        view.render_at(area, &mut Buffer::empty(area), now);
        let selected = view.event_list.cursor;
        let before = screen.lock().unwrap().screen().contents();
        let x = view.viewport.borrow().event_panel.x;
        let top = view.viewport.borrow().event_detail.y;
        for y in [top, 28] {
            view.set_active_panel(Panel::TaskList);
            for _ in 0..2 {
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    assert_eq!(view.handle_mouse(mouse(kind, x, y)), None);
                }
            }
            assert_eq!(view.event_list.cursor, selected);
            assert!(view.active_panel == Panel::EventDetail);
            assert_eq!(screen.lock().unwrap().screen().contents(), before);
        }
    }
}

#[test]
fn output_border_layout_reserves_one_row_and_keeps_hit_regions_separate() {
    let (view, now) = fixture(true);
    for width in [100, 149, 150, 200] {
        for height in 2..60 {
            let area = Rect::new(0, 0, width, height);
            let mut buffer = Buffer::empty(area);
            view.render_at(area, &mut buffer, now);
            let layout = view.center_layout(area);
            let viewport = view.viewport.borrow();
            if !layout.event_detail.is_empty() {
                let [top, bottom] = viewport.output_borders;
                assert_eq!(top.y, viewport.event_detail.y);
                assert_eq!(bottom.y, layout.footer.y - 1);
                assert_eq!(viewport.event_detail.bottom(), bottom.y);
                assert_eq!(bottom.width, width);
                assert!(viewport.event_panel.bottom() <= top.y);
                assert!(viewport.prompt.bottom() <= top.y);
                assert!(!viewport.event_detail.contains((bottom.x, bottom.y).into()));
                for x in [0, width - 1] {
                    assert_eq!(buffer[(x, bottom.y)].symbol(), "─");
                }
            } else {
                assert!(viewport.output_borders.iter().all(|rect| rect.is_empty()));
            }
        }
    }
}

#[test]
fn event_detail_task_switch_preserves_scroll_and_search() {
    let (mut view, now) = fixture(true);
    let id = view.selected_id().unwrap().clone();
    let mut other = view.selected_row().unwrap().clone();
    other.id.id = "detail-other".into();
    let other_id = other.id.clone();
    view.set_sessions(vec![view.selected_row().unwrap().clone(), other], false);
    panel(&view, now);
    view.set_active_panel(Panel::EventDetail);
    view.event_detail.search.input.insert("detail");
    view.event_detail.search.input.focus();
    view.event_detail.scroll.borrow_mut().offset = 5;
    panel(&view, now);
    let scroll = view.event_detail.scroll.borrow().offset;
    assert!(scroll > 0);
    let events = view.event_panel.clone();
    let index = view.rows.iter().position(|row| row.id == other_id).unwrap();
    view.set_selected(index);
    view.set_event(Vec::new());
    assert!(!view.event_detail.search.input.applied());
    let index = view.rows.iter().position(|row| row.id == id).unwrap();
    view.set_selected(index);
    view.set_event(events);
    view.event_session = Some(id);
    assert_eq!(view.event_detail.search.input.text(), "detail");
    assert!(view.event_detail.search.input.focused());
    assert_eq!(
        view.visible_event()
            .and_then(|event| event.source.as_deref()),
        Some("scroll-1")
    );
    assert_eq!(
        view.event_detail.scroll.borrow().offset,
        scroll,
        "offset before render"
    );
    panel(&view, now);
    assert_eq!(view.event_detail.scroll.borrow().offset, scroll);
}

#[test]
fn event_detail_task_switch_discards_scroll_without_the_selected_identity() {
    for missing_source in [false, true] {
        let (mut view, now) = fixture(true);
        if missing_source {
            view.event_panel[1].source = None;
        }
        let id = view.selected_id().unwrap().clone();
        let mut other = view.selected_row().unwrap().clone();
        other.id.id = "detail-other".into();
        let other_id = other.id.clone();
        view.set_sessions(vec![view.selected_row().unwrap().clone(), other], false);
        panel(&view, now);
        view.event_detail.search.input.insert("detail");
        view.event_detail.scroll.borrow_mut().offset = 5;
        panel(&view, now);
        assert!(view.event_detail.scroll.borrow().offset > 0);
        let mut events = view.event_panel.clone();
        if !missing_source {
            events.remove(1);
        }
        let index = view.rows.iter().position(|row| row.id == other_id).unwrap();
        view.set_selected(index);
        view.set_event(Vec::new());
        let index = view.rows.iter().position(|row| row.id == id).unwrap();
        view.set_selected(index);
        view.set_event(events);
        view.event_session = Some(id);
        panel(&view, now);
        assert_eq!(view.event_detail.scroll.borrow().offset, 0);
        assert_eq!(view.event_detail.search.input.text(), "detail");
    }
}

fn output_file_event(path: &Path) -> crate::event::Event {
    let mut record: serde_json::Value = serde_json::from_str(
        include_str!("../../tests/fixtures/claude-task-notifications.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    record["content"] = record["content"]
        .as_str()
        .unwrap()
        .replace(
            "/tmp/agents-notification/tasks/b9wi31ww0.output",
            path.to_str().unwrap(),
        )
        .into();
    let transcript = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(transcript.path(), format!("{record}\n")).unwrap();
    crate::providers::claude::event_reader::tail(transcript.path(), 1)
        .unwrap()
        .remove(0)
}

fn append_output(path: &Path, start: usize, end: usize) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    for row in start..end {
        writeln!(file, "output row {row:03}").unwrap();
    }
}

#[test]
fn output_file_append_follows_bottom_and_preserves_scrolled_position() {
    for width in [100, 200] {
        for scrolled_up in [false, true] {
            let output = tempfile::NamedTempFile::new().unwrap();
            append_output(output.path(), 0, 60);
            let (mut view, _) = fixture(false);
            view.set_event(vec![output_file_event(output.path())]);
            view.select_event(Some(0));
            screen_rows(&view, width, 30);
            view.event_detail.scroll.borrow_mut().offset = usize::MAX;
            screen_rows(&view, width, 30);
            if scrolled_up {
                event_detail_wheel(&mut view, false);
                screen_rows(&view, width, 30);
            }
            let before = view.event_detail.scroll.borrow().offset;
            assert!(before > 0);
            append_output(output.path(), 60, 90);
            view.refresh_event_output();
            let rendered = screen_rows(&view, width, 30).join("\n");
            let after = view.event_detail.scroll.borrow().offset;
            if scrolled_up {
                assert_eq!(after, before, "{width}");
                assert!(!rendered.contains("output row 089"));
            } else {
                assert!(after > before, "{width}: {before} -> {after}");
                assert_eq!(after, view.viewport.borrow().event_detail_max);
                assert!(rendered.contains("output row 089"), "{rendered}");
            }
        }
    }
}

#[test]
fn empty_output_file_follows_first_content() {
    let output = tempfile::NamedTempFile::new().unwrap();
    let (mut view, _) = fixture(false);
    view.set_event(vec![output_file_event(output.path())]);
    view.select_event(Some(0));
    screen_rows(&view, 120, 30);
    assert_eq!(view.event_detail.scroll.borrow().offset, 0);
    append_output(output.path(), 0, 80);
    view.refresh_event_output();
    let rendered = screen_rows(&view, 120, 30).join("\n");
    assert!(rendered.contains("output row 079"), "{rendered}");
    assert_eq!(
        view.event_detail.scroll.borrow().offset,
        view.viewport.borrow().event_detail_max
    );
}

#[test]
fn output_file_notifications_before_one_draw_keep_following_the_bottom() {
    let output = tempfile::NamedTempFile::new().unwrap();
    let (mut view, _) = fixture(false);
    view.set_event(vec![output_file_event(output.path())]);
    view.select_event(Some(0));
    screen_rows(&view, 120, 30);
    append_output(output.path(), 0, 40);
    view.refresh_event_output();
    append_output(output.path(), 40, 80);
    view.refresh_event_output();
    let rendered = screen_rows(&view, 120, 30).join("\n");
    assert!(rendered.contains("output row 079"), "{rendered}");
}

#[test]
fn output_file_append_follows_fully_revealed_sections() {
    let output = tempfile::NamedTempFile::new().unwrap();
    append_output(output.path(), 0, 60);
    let (mut view, _) = fixture(false);
    view.set_event(vec![output_file_event(output.path())]);
    view.select_event(Some(0));
    screen_rows(&view, 120, 30);
    for _ in 0..100 {
        let bottom = {
            let detail = view.event_detail.scroll.borrow();
            detail.state.reveal == detail.state.hidden
        };
        if bottom {
            break;
        }
        event_detail_wheel(&mut view, true);
        screen_rows(&view, 120, 30);
    }
    {
        let detail = view.event_detail.scroll.borrow();
        assert_eq!(detail.state.fallback_max, None);
        assert_eq!(detail.state.reveal, detail.state.hidden);
    }
    append_output(output.path(), 60, 90);
    view.refresh_event_output();
    let rendered = screen_rows(&view, 120, 30).join("\n");
    assert!(rendered.contains("output row 089"), "{rendered}");
}

#[test]
fn empty_output_file_keeps_following_across_separate_appends() {
    let output = tempfile::NamedTempFile::new().unwrap();
    let (mut view, _) = fixture(false);
    view.set_event(vec![output_file_event(output.path())]);
    view.select_event(Some(0));
    screen_rows(&view, 200, 50);
    for (start, end) in [(0, 80), (80, 90), (90, 100)] {
        append_output(output.path(), start, end);
        view.set_event(view.event_panel.clone());
        view.refresh_event_output();
        let rendered = screen_rows(&view, 200, 50).join("\n");
        assert!(
            rendered.contains(&format!("output row {:03}", end - 1)),
            "{rendered}"
        );
        screen_rows(&view, 200, 50);
    }
}

fn recorded_next_event_fixture(sort: EventSort) -> View {
    let (mut view, _) = fixture(false);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let events = crate::providers::claude::event_reader::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .filter(|event| !event.calls.is_empty())
        .take(4)
        .collect();
    view.event_list.sort = sort;
    view.set_event(events);
    let order: Vec<_> = view
        .shown_events()
        .map(|(_, event)| event.calls[0].name.as_str())
        .collect();
    assert_eq!(
        order,
        match sort {
            EventSort::Time => vec!["Bash", "Read", "Edit", "Write"],
            EventSort::Duration => vec!["Bash", "Write", "Edit", "Read"],
        }
    );
    view.select_event(Some(2));
    view
}

fn assert_next_event_edge(sort: EventSort, next_call: &str) {
    for width in [90, 120, 160] {
        let mut view = recorded_next_event_fixture(sort);
        let next = view
            .event_panel
            .iter()
            .find(|event| event.calls[0].name == next_call)
            .unwrap()
            .clone();
        let title = next.calls[0].title.clone();
        // Render the next event's own title row before comparing the edge's cells.
        view.select_fixture_event(Some(next));
        let area = Rect::new(0, 0, width, 30);
        let draw = |view: &View| {
            let mut buffer = Buffer::empty(area);
            view.render(area, &mut buffer);
            buffer
        };
        let panel = draw(&view);
        let detail = view.viewport.borrow().event_detail;
        let expected = (detail.y..detail.bottom())
            .find_map(|y| {
                let row: String = (0..width).map(|x| panel[(x, y)].symbol()).collect();
                column_of(&row, &title).map(|x| panel[(x as u16, y)].clone())
            })
            .expect("next event's title is visible in its panel");
        view.select_event(Some(2));
        draw(&view);
        view.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        let buffer = draw(&view);
        let y = view.viewport.borrow().output_borders[1].y;
        let row: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
        let x =
            column_of(&row, &title).unwrap_or_else(|| panic!("missing next title: {row}")) as u16;
        assert_eq!(x as usize, (usize::from(width) - title.width() - 2) / 2 + 1);
        for x in x..x + title.width() as u16 {
            assert_eq!(buffer[(x, y)].fg, expected.fg);
            assert_eq!(buffer[(x, y)].modifier, expected.modifier);
        }
        assert_eq!(view.event_list.cursor, Some(2));
    }
}

#[test]
fn event_detail_bottom_edge_shows_the_next_event_in_time_order() {
    assert_next_event_edge(EventSort::Time, "Write");
}

#[test]
fn event_detail_bottom_edge_shows_the_next_event_in_duration_order() {
    assert_next_event_edge(EventSort::Duration, "Read");
}

#[test]
fn event_detail_bottom_edge_shows_when_the_whole_event_fits() {
    for (sort, next) in [(EventSort::Time, "Write"), (EventSort::Duration, "Read")] {
        let view = recorded_next_event_fixture(sort);
        let title = &view
            .event_panel
            .iter()
            .find(|event| event.calls[0].name == next)
            .unwrap()
            .calls[0]
            .title;
        let rows = screen_rows(&view, 120, 200);
        assert_eq!(view.viewport.borrow().event_detail_max, 0);
        let y = view.viewport.borrow().output_borders[1].y;
        assert!(rows[usize::from(y)].contains(title));
    }
}

#[test]
fn event_detail_bottom_edge_waits_for_the_last_visible_line() {
    for (sort, next) in [(EventSort::Time, "Write"), (EventSort::Duration, "Read")] {
        let mut view = recorded_next_event_fixture(sort);
        let title = view
            .event_panel
            .iter()
            .find(|event| event.calls[0].name == next)
            .unwrap()
            .calls[0]
            .title
            .clone();
        for key in [None, Some(KeyCode::Down), Some(KeyCode::End)] {
            if let Some(key) = key {
                view.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            let rows = screen_rows(&view, 120, 30);
            let offset = view.event_detail.scroll.borrow().offset;
            let max = view.viewport.borrow().event_detail_max;
            assert!(max > 1);
            if key == Some(KeyCode::End) {
                assert_eq!(offset, max);
            } else if key.is_some() {
                assert!(offset > 0 && offset < max);
            } else {
                assert_eq!(offset, 0);
            }
            let y = view.viewport.borrow().output_borders[1].y;
            assert_eq!(
                rows[usize::from(y)].contains(&title),
                key == Some(KeyCode::End),
                "{}",
                rows[usize::from(y)]
            );
        }
    }
}

#[test]
fn event_detail_bottom_edge_is_empty_after_the_last_event() {
    for sort in [EventSort::Time, EventSort::Duration] {
        let mut view = recorded_next_event_fixture(sort);
        view.select_event(Some(3));
        for scrolled in [false, true] {
            if scrolled {
                view.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
            }
            let rows = screen_rows(&view, 120, 30);
            let y = view.viewport.borrow().output_borders[1].y;
            assert!(
                rows[usize::from(y)].chars().all(|c| c == '─'),
                "{}",
                rows[usize::from(y)]
            );
        }
    }
}

#[test]
fn event_detail_bottom_edge_hides_with_the_detail_panel() {
    let mut view = recorded_next_event_fixture(EventSort::Time);
    screen_rows(&view, 160, 40);
    view.set_active_panel(Panel::TaskList);
    for key in ['|', '_'] {
        view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        view.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::SHIFT));
    }
    let rows = screen_rows(&view, 160, 40);
    assert!(view.viewport.borrow().event_detail.is_empty());
    assert!(!rows[38].contains("plan.md"));
}

/// Whether each event detail rule, top then bottom, is drawn heavy.
fn heavy_output_borders(view: &View) -> [bool; 2] {
    let rows = screen_rows(view, 120, 40);
    view.viewport
        .borrow()
        .output_borders
        .map(|rule| rows[usize::from(rule.y)].contains('━'))
}

#[test]
fn event_detail_rules_turn_heavy_only_where_the_next_scroll_would_leave() {
    let (mut view, _) = fixture(true);
    // Opening an event that starts at its top is no scroll inside it.
    assert_eq!(heavy_output_borders(&view), [false, false]);
    event_detail_wheel(&mut view, true);
    assert_eq!(heavy_output_borders(&view), [false, false]);
    for _ in 0..60 {
        if heavy_output_borders(&view)[1] {
            break;
        }
        event_detail_wheel(&mut view, true);
    }
    assert_eq!(view.event_list.cursor, Some(1));
    assert_eq!(heavy_output_borders(&view), [false, true]);

    // Returning from the next event lands at the bottom without a scroll inside it.
    event_detail_wheel(&mut view, true);
    assert_eq!(view.event_list.cursor, Some(2));
    event_detail_wheel(&mut view, false);
    assert_eq!(view.event_list.cursor, Some(1));
    assert_eq!(heavy_output_borders(&view), [false, false]);
    for _ in 0..60 {
        if heavy_output_borders(&view)[0] {
            break;
        }
        event_detail_wheel(&mut view, false);
    }
    assert_eq!(view.event_list.cursor, Some(1));
    assert_eq!(heavy_output_borders(&view), [true, false]);
}
