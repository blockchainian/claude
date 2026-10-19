use super::*;
use crate::providers::Provider;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn fixture() -> (tempfile::TempDir, View) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, include_str!("../../tests/fixtures/transcript.jsonl")).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    key(&mut view, KeyCode::Tab);
    view.log_session = view.selected_id().cloned();
    view.set_log(crate::providers::claude::log::tail_session(&path, usize::MAX, true).unwrap());
    (dir, view)
}

fn key(view: &mut View, code: KeyCode) {
    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn draw_detail(view: &View) {
    let area = Rect::new(0, 0, 100, 30);
    view.render_log_detail(area, &mut Buffer::empty(area), Utc::now());
    assert!(view.viewport.borrow().log_detail_layout.is_some());
}

#[test]
fn list_search_finds_trailing_tool_name_after_non_ascii_text() {
    let (_dir, mut view) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        dir.path().to_owned(),
        dir.path().join("manager"),
    );
    let mut session = view.selected_row().unwrap().clone();
    session.transcript = Some(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex-inner/command.jsonl"),
    );
    let event = provider
        .log(&session, usize::MAX, false)
        .unwrap()
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "exec_command"))
        .unwrap();
    for body in ["type", "检查日志", "type · 1", "❮ type"] {
        let mut event = event.clone();
        event.body = body.into();
        event.paths.clear();
        view.set_log(vec![event.clone()]);
        view.prepare_log_search();
        assert_eq!(
            view.log_search.bodies[0].to_string(),
            format!("Error {body} exec_command")
        );
        view.log_search.query.set_text("exec_command");
        view.rematch_log();
        assert_eq!(view.matching_log_count(), 1, "{body}");
        assert_eq!(view.visible_log_event(), Some(&event), "{body}");
    }
}

#[test]
fn list_search_keeps_layout_when_the_selected_event_survives() {
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    draw_detail(&view);
    let selected = view.visible_log_event().unwrap().clone();
    for code in "Stop".chars().map(KeyCode::Char).chain([
        KeyCode::Backspace,
        KeyCode::Backspace,
        KeyCode::Esc,
    ]) {
        key(&mut view, code);
        assert_eq!(view.visible_log_event(), Some(&selected));
        assert!(
            view.viewport.borrow().log_detail_layout.is_some(),
            "unchanged event lost its layout on {code:?}"
        );
    }
}

#[test]
fn list_search_discards_layout_when_the_selected_event_changes() {
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    draw_detail(&view);
    let selected = view.visible_log_event().unwrap().clone();
    view.log_search.query.set_text("run_in_background");
    view.rematch_log();
    assert_ne!(view.visible_log_event(), Some(&selected));
    assert!(view.viewport.borrow().log_detail_layout.is_none());
    draw_detail(&view);
}

#[test]
fn list_search_reuses_bodies_and_preserves_smart_matching() {
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    let bodies = view.log_search.bodies.as_ptr();
    for query in [
        "Stop",
        "Sto",
        "stop hook",
        "stop",
        "session",
        "错误",
        "",
        "Claude",
    ] {
        view.log_search.query.set_text(query);
        view.rematch_log();
        let pattern = view.log_search.pattern();
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buffer = Vec::new();
        let cwd = &view.selected_row().unwrap().cwd;
        let expected: Vec<_> = view
            .log
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                let body = format!(
                    "{} {}",
                    super::super::log::kind_label(&event.kind),
                    super::super::log::body_line(event, usize::MAX, cwd)
                );
                pattern
                    .score(Utf32Str::new(&body, &mut buffer), &mut matcher)
                    .map(|_| index)
            })
            .collect();
        assert_eq!(view.log_search.matches, expected, "{query}");
        assert_eq!(view.log_search.bodies.as_ptr(), bodies);
    }
    key(&mut view, KeyCode::Esc);
    assert_eq!(view.log_search.bodies.as_ptr(), bodies);
}

#[test]
fn list_search_keeps_partial_highlights_at_the_row_edge() {
    use ratatui::style::Modifier;
    use unicode_segmentation::UnicodeSegmentation;
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('/'));
    view.log_search.query.set_text("hooks");
    view.rematch_log();
    assert_eq!(view.matching_log_count(), 1);
    let event = view.visible_log_event().unwrap().clone();
    view.log_selected = None;
    let cwd = &view.selected_row().unwrap().cwd;
    for width in 11..100 {
        let content_width = width - 10;
        let mut expected = super::super::log::body_line(&event, content_width as usize, cwd);
        let indices = view.log_search.indices(&expected.to_string());
        let mut offset = 0;
        expected.spans = expected
            .spans
            .into_iter()
            .flat_map(|span| {
                span.content
                    .graphemes(true)
                    .map(|glyph| {
                        let style = if indices.binary_search(&offset).is_ok() {
                            span.style.add_modifier(Modifier::BOLD)
                        } else {
                            span.style
                        };
                        offset += 1;
                        Span::styled(glyph.to_owned(), style)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let expected = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
            expected,
            content_width as usize,
        );
        let area = Rect::new(0, 0, width, 3);
        let mut actual = Buffer::empty(area);
        view.render_log(area, &mut actual, Utc::now());
        let mut reference = Buffer::empty(area);
        expected.render(Rect::new(0, 0, content_width, 1), &mut reference);
        for x in 0..content_width {
            assert_eq!(actual[(x, 0)].symbol(), reference[(x, 0)].symbol());
            assert_eq!(
                actual[(x, 0)].modifier.contains(Modifier::BOLD),
                reference[(x, 0)].modifier.contains(Modifier::BOLD),
                "width={width}, x={x}"
            );
        }
    }
}

#[test]
fn duration_toggle_selects_slowest_and_keeps_search_order() {
    let (_dir, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    let mut events: Vec<_> = view.log.iter().cycle().take(4).cloned().collect();
    for (index, event) in events.iter_mut().enumerate() {
        event.kind = crate::log::Kind::Tool;
        event.completed_time = true;
        event.ended_at = Some(event.started_at);
        event.duration = Some(chrono::Duration::seconds([30, 10, 20, 10][index]));
        event.body = if index % 2 == 0 {
            "duration target"
        } else {
            "duration other"
        }
        .into();
    }
    view.set_log(events);
    view.select_log_event(Some(1));
    let slowest = view.log[0].clone();
    key(&mut view, KeyCode::Char('t'));
    assert_eq!(
        view.shown_log_events().map(|(i, _)| i).collect::<Vec<_>>(),
        [1, 3, 2, 0]
    );
    assert_eq!(view.visible_log_event(), Some(&slowest));
    key(&mut view, KeyCode::Char('/'));
    for c in "target".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    assert_eq!(
        view.shown_log_events().map(|(i, _)| i).collect::<Vec<_>>(),
        [2, 0]
    );
    key(&mut view, KeyCode::Esc);
    assert_eq!(
        view.shown_log_events().map(|(i, _)| i).collect::<Vec<_>>(),
        [1, 3, 2, 0]
    );
    key(&mut view, KeyCode::Char('t'));
    assert_eq!(
        view.shown_log_events().map(|(i, _)| i).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(view.visible_log_event(), Some(&slowest));
    assert_eq!(view.log_selected, Some(0));
    assert!(!view.following_log());
}

#[test]
fn duration_keys_include_messages_none_ties_and_running_update_time() {
    let (_dir, mut view) = fixture();
    let now = Utc::now();
    let mut events: Vec<_> = view.log.iter().cycle().take(7).cloned().collect();
    for (i, event) in events.iter_mut().enumerate() {
        event.kind = crate::log::Kind::Tool;
        event.started_at = now - chrono::Duration::seconds(20 - i as i64);
        event.previous_end = None;
        event.completed_time = true;
        event.ended_at = Some(event.started_at);
        event.duration = Some(chrono::Duration::seconds(10));
    }
    events[0].duration = None;
    events[2].kind = crate::log::Kind::User;
    events[2].previous_end = Some(events[2].started_at - chrono::Duration::seconds(3));
    events[3].kind = crate::log::Kind::Assistant;
    events[3].previous_end = Some(events[3].started_at - chrono::Duration::seconds(2));
    // Claude spans use previous_end even without Event.duration.
    events[4].completed_time = false;
    events[4].duration = None;
    events[4].previous_end = Some(events[4].started_at - chrono::Duration::seconds(1));
    events[5].duration = None;
    events[6].completed_time = false;
    events[6].duration = None;
    events[6].ended_at = None;
    events[6].previous_end = Some(now - chrono::Duration::seconds(5));
    let selected = view.selected;
    view.rows[selected].process = Some(crate::model::Process {
        pid: 123,
        label: None,
        started_at: Some(now),
    });
    view.rows[selected].status = SessionStatus::Working;
    key(&mut view, KeyCode::Tab);
    view.set_log(events);
    view.last_update = now;
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('t'));
    assert_eq!(view.log_order, [0, 5, 4, 3, 2, 6, 1]);
    assert_eq!(
        view.log_redraw_in(now + chrono::Duration::seconds(30)),
        None
    );
    let before = view.log_order.clone();
    let area = Rect::new(0, 0, 100, 10);
    let mut first = Buffer::empty(area);
    view.render_log(area, &mut first, now);
    let mut later = Buffer::empty(area);
    view.render_log(area, &mut later, now + chrono::Duration::seconds(30));
    assert_ne!(first, later, "running duration ticks between updates");
    assert_eq!(
        view.log_redraw_in(now),
        Some(std::time::Duration::from_secs(1))
    );
    assert_eq!(before, view.log_order);
    let row = view.shown_position(6).unwrap() as u16;
    let text: String = (0..100).map(|x| first[(x, row)].symbol()).collect();
    assert!(text.trim_end().ends_with("5s"), "{text}");
    assert!(view.shown_position(6).unwrap() < view.shown_position(1).unwrap());
    view.log_search.query.set_text("no matching row");
    view.rematch_log();
    assert_eq!(view.matching_log_count(), 0);
    view.log_search.clear();
    view.rematch_log();
    assert_eq!(view.log_order, before);
}

#[test]
fn duration_toggle_preserves_background_return_event() {
    for duration_first in [false, true] {
        for update_before_return in [false, true] {
            let (_dir, mut view) = fixture();
            let mut events: Vec<_> = view.log.iter().cycle().take(3).cloned().collect();
            for (index, event) in events.iter_mut().enumerate() {
                event.kind = crate::log::Kind::Tool;
                event.completed_time = true;
                event.ended_at = Some(event.started_at);
                event.duration = Some(chrono::Duration::seconds([30, 10, 20][index]));
            }
            view.set_log(events.clone());
            view.mouse_focus = Panel::LogList;
            if duration_first {
                key(&mut view, KeyCode::Char('t'));
            }
            view.select_log_event(view.shown_position(0));
            let output = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(
                output.path(),
                include_str!("../../tests/fixtures/claude-peer-message.txt"),
            )
            .unwrap();
            view.rows[view.selected]
                .background
                .push(crate::model::BackgroundTask {
                    description: "Background output".into(),
                    output: Some(output.path().into()),
                    started_at: events[0].started_at,
                });
            let area = Rect::new(0, 0, 160, 40);
            view.render_at(area, &mut Buffer::empty(area), Utc::now());
            let (x, y) = {
                let viewport = view.viewport.borrow();
                (viewport.details.x, viewport.details_background[0].0.start)
            };
            for _ in 0..2 {
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    view.handle_mouse(MouseEvent {
                        kind,
                        column: x,
                        row: y,
                        modifiers: KeyModifiers::NONE,
                    });
                }
            }
            assert!(view.log_background_return.is_some());
            view.mouse_focus = Panel::LogList;
            key(&mut view, KeyCode::Char('t'));
            if update_before_return {
                view.set_log(events.clone());
            }
            for kind in [
                MouseEventKind::Down(MouseButton::Right),
                MouseEventKind::Up(MouseButton::Right),
            ] {
                view.handle_mouse(MouseEvent {
                    kind,
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                });
            }
            assert_eq!(
                view.visible_log_event(),
                Some(&events[0]),
                "duration_first={duration_first}, update={update_before_return}"
            );
        }
    }
}

#[test]
fn duration_empty_toggle_and_top_navigation_do_not_page() {
    let (_dir, mut view) = fixture();
    view.set_log(Vec::new());
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('t'));
    assert_eq!(view.log_sort, LogSort::Duration);
    assert!(view.log_order.is_empty());
    assert_eq!(view.log_selected, None);
    key(&mut view, KeyCode::Char('t'));
    assert_eq!(view.log_sort, LogSort::Time);
    let (_other, mut view) = fixture();
    view.mouse_focus = Panel::LogList;
    key(&mut view, KeyCode::Char('t'));
    view.select_log_event(Some(0));
    view.move_log_selection(false);
    assert_eq!(view.log_pending_older, 0);
}
