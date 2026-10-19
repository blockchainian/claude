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
    view.active_panel = Panel::Output;
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
    assert_eq!(view.log_selected, Some(1));
    assert_eq!(panel(&view, now), initial);
    view.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
    assert_eq!(view.log_selected, Some(1));
    assert_ne!(panel(&view, now), initial);
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
    for _ in 0..2 {
        wheel(&mut view, false);
        panel(&view, now);
        assert_eq!(view.log_selected, Some(1), "the title holds two wheels");
    }
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
        for _ in 0..2 {
            wheel(&mut view, true);
            panel(&view, now);
            assert_eq!(
                view.viewport.borrow().log_detail_scroll,
                0,
                "the title holds two wheels"
            );
        }
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
    for _ in 0..2 {
        wheel(&mut view, false);
        panel(&view, now);
        assert_eq!(view.log_selected, Some(1), "the title holds two wheels");
    }
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

#[test]
fn log_sort_key_only_changes_the_active_log_list() {
    let (mut view, _) = fixture(false);
    assert!(view.active_panel == Panel::Output);
    let action = view.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    assert_eq!(action, None);
    assert_eq!(view.log_sort, LogSort::Time);
    view.set_active_panel(Panel::LogList);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)),
        Some(Action::PersistSorting(LogSort::Duration))
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

fn detail_wheel(view: &mut View, forward: bool) {
    let detail = view.viewport.borrow().log_detail;
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
fn log_detail_footer_shows_the_scrolled_off_title() {
    for width in [100, 160] {
        let (mut view, _) = fixture(true);
        let rows = screen_rows(&view, width, 40);
        let footer = rows.last().unwrap().clone();
        let detail_y = usize::from(view.viewport.borrow().log_detail.y);
        let title = rows[detail_y + 1]
            .trim()
            .split("  ")
            .next()
            .unwrap()
            .to_owned();
        assert!(!title.is_empty(), "{}", rows.join("\n"));
        assert!(!footer.contains(&title), "title on screen: {footer}");
        let title_x = column_of(&rows[detail_y + 1], &title).unwrap();
        let time_x = view
            .viewport
            .borrow()
            .log_detail_columns
            .map(|columns| usize::from(columns.time.x))
            .unwrap();

        detail_wheel(&mut view, true);
        let rows = screen_rows(&view, width, 40);
        let scrolled = rows.last().unwrap();
        assert!(!rows[detail_y + 1].contains(&title), "{}", rows.join("\n"));
        assert!(
            scrolled.starts_with(footer.trim_end()),
            "hints stay: {scrolled}"
        );
        let x = column_of(scrolled, &title).unwrap_or_else(|| panic!("{scrolled}"));
        if width >= super::render::WIDE_LAYOUT_COLUMNS {
            assert_eq!(
                x, title_x,
                "wide layouts center the footer title: {scrolled}"
            );
        } else {
            assert_eq!(
                x + title.width() + 2,
                time_x,
                "narrow layouts end it before the time column: {scrolled}"
            );
        }
        assert!(scrolled[..].chars().skip(time_x).all(|c| c == ' '));

        detail_wheel(&mut view, false);
        let rows = screen_rows(&view, width, 40);
        assert!(
            !rows.last().unwrap().contains(&title),
            "title back on screen"
        );
    }
}

#[test]
fn log_detail_footer_shows_the_title_of_the_section_on_the_top_row() {
    let (mut view, _) = tool_fixture();
    let mut events = view.log.clone();
    events[0].calls[0].input = vec![crate::log::Section::Text {
        label: String::new(),
        text: "first body".into(),
    }];
    events[0].calls[1].input = vec![crate::log::Section::Text {
        label: String::new(),
        text: (1..=40)
            .map(|i| format!("second body {i:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }];
    let first = events[0].calls[0].detail_title.replace(' ', "");
    view.set_log(events);
    view.select_log_event(Some(0));
    screen_rows(&view, 100, 40);
    let detail_y = usize::from(view.viewport.borrow().log_detail.y);
    let mut footers = Vec::new();
    for _ in 0..20 {
        detail_wheel(&mut view, true);
        let rows = screen_rows(&view, 100, 40);
        let detail = rows[detail_y + 1..rows.len() - 1].join("\n");
        let footer = rows.last().unwrap().clone();
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
fn log_detail_footer_title_is_muted() {
    let (mut view, _) = fixture(true);
    screen_rows(&view, 100, 40);
    detail_wheel(&mut view, true);
    let area = Rect::new(0, 0, 100, 40);
    let mut buffer = Buffer::empty(area);
    view.render(area, &mut buffer);
    let footer = (0..100)
        .map(|x| buffer[(x, 39)].symbol())
        .collect::<String>();
    let x = column_of(&footer, "Assistant").unwrap_or_else(|| panic!("{footer}")) as u16;
    for x in x..x + "Assistant".len() as u16 {
        let cell = &buffer[(x, 39)];
        assert!(
            cell.modifier.contains(ratatui::style::Modifier::DIM),
            "{cell:?}"
        );
        assert_eq!(cell.fg, ratatui::style::Color::Reset, "{cell:?}");
        assert!(
            !cell.modifier.contains(ratatui::style::Modifier::BOLD),
            "{cell:?}"
        );
    }
}

fn recorded_tool_footer(forward: bool) {
    for width in [200, 149, 120, 100] {
        let (mut view, _) = fixture(false);
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
        let event = crate::providers::claude::log::tail(&path, usize::MAX)
            .unwrap()
            .into_iter()
            .find(|event| event.calls.first().is_some_and(|call| call.name == "Bash"))
            .unwrap();
        let title = event.calls[0].detail_title.clone();
        view.set_log(vec![event]);
        view.select_log_event(Some(0));
        let initial = screen_rows(&view, width, 30);
        assert!(!initial.last().unwrap().contains(&title));
        if !forward {
            let path =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
            let after = crate::providers::claude::log::tail(&path, usize::MAX)
                .unwrap()
                .into_iter()
                .find(|event| event.kind == crate::log::Kind::Assistant && event.calls.is_empty())
                .unwrap();
            let event = view.log[0].clone();
            view.set_log(vec![event, after]);
            view.select_log_event(Some(1));
            screen_rows(&view, width, 30);
        }
        detail_wheel(&mut view, forward);
        let rows = screen_rows(&view, width, 30);
        let footer = rows.last().unwrap();
        let viewport = view.viewport.borrow();
        assert!(
            viewport.log_detail_scroll > 0,
            "title must be offscreen: {rows:?}"
        );
        let columns = viewport.log_detail_columns.unwrap();
        let centered =
            super::centering::centered_x(columns.panel, columns.text, title.width() as u16);
        let start = 1 + view.center_footer_hints().width() as u16 + 2;
        let expected = if width >= super::render::WIDE_LAYOUT_COLUMNS && centered >= start {
            centered
        } else {
            columns.text.right() - title.width() as u16
        };
        let compact = footer.replace(' ', "");
        assert!(compact.contains(&title.replace(' ', "")), "{footer}");
        let first = title.chars().next().unwrap().to_string();
        let x = column_of(footer, &first).unwrap();
        assert_eq!(x, usize::from(expected), "{footer}");
        assert!(
            x + title.width() <= usize::from(columns.text.right()),
            "{footer}"
        );
        assert!(
            footer
                .chars()
                .skip(usize::from(columns.time.x))
                .all(|c| c == ' '),
            "{footer}"
        );
    }
}

#[test]
fn log_detail_recorded_footer_scrolling_down() {
    recorded_tool_footer(true);
}

#[test]
fn log_detail_recorded_footer_scrolling_up() {
    recorded_tool_footer(false);
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
            view.set_active_panel(Panel::Output);
            view.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
            let buffer = draw(&view);
            let before = view.viewport.borrow().log_detail_scroll;
            let max = view.viewport.borrow().log_detail_max;
            assert!(
                before > 0 && before < max,
                "{width}x{height}: {before}/{max}"
            );
            let selected = view.log_selected;
            let log_scroll = view.viewport.borrow().log_scroll;
            let output = view.viewport.borrow().log_detail;
            let y = if bottom { height - 2 } else { output.y };
            // The narrow layout shares this rule with the log list.
            let x = view.viewport.borrow().log.x;
            if bottom {
                assert_eq!(buffer[(x, y)].symbol(), "─", "missing lower rule");
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
                view.log_selected, selected,
                "border changed event at {width}x{height}, bottom={bottom}"
            );
            assert_eq!(view.viewport.borrow().log_scroll, log_scroll);
            assert!(view.active_panel == Panel::Output);
            assert_eq!(
                view.viewport.borrow().log_detail_scroll,
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
        let selected = view.log_selected;
        let before = screen.lock().unwrap().screen().contents();
        let x = view.viewport.borrow().log.x;
        let top = view.viewport.borrow().log_detail.y;
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
            assert_eq!(view.log_selected, selected);
            assert!(view.active_panel == Panel::Output);
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
            if !layout.log_detail.is_empty() {
                let [top, bottom] = viewport.output_borders;
                assert_eq!(top.y, viewport.log_detail.y);
                assert_eq!(bottom.y, layout.footer.y - 1);
                assert_eq!(viewport.log_detail.bottom(), bottom.y);
                assert_eq!(bottom.width, width);
                assert!(viewport.log.bottom() <= top.y);
                assert!(viewport.prompt.bottom() <= top.y);
                assert!(!viewport.log_detail.contains((bottom.x, bottom.y).into()));
                for x in 0..width {
                    assert_eq!(buffer[(x, bottom.y)].symbol(), "─");
                }
            } else {
                assert!(viewport.output_borders.iter().all(|rect| rect.is_empty()));
            }
        }
    }
}
