#[path = "codex_subagents_tests.rs"]
mod codex_subagents_tests;

#[path = "panel_focus_tests.rs"]
mod panel_focus_tests;

#[path = "prompt_mouse_tests.rs"]
mod prompt_mouse_tests;

#[path = "log_center_tests.rs"]
mod log_center_tests;

#[path = "task_cursor_tests.rs"]
mod task_cursor;

#[path = "heading_tests.rs"]
mod heading_tests;

fn session_key(id: &str) -> SessionKey {
    SessionKey {
        provider: crate::providers::ProviderId::Claude,
        id: id.into(),
    }
}

#[path = "details_project_tests.rs"]
mod details_project;

#[path = "details_footer_tests.rs"]
mod details_footer;

#[path = "thread_clock_tests.rs"]
mod thread_clock_tests;

use super::*;

#[path = "ask_question_tests.rs"]
mod ask_question_tests;
use chrono::{Duration, Utc};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn click_mouse(view: &mut View, press: MouseEvent) -> Option<Action> {
    assert!(view.handle_mouse(press).is_none(), "press must not act");
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..press
    })
}

#[test]
fn task_tabs_have_no_filter_hint() {
    for width in [60, 160] {
        let view = fixture();
        let rendered = screen(&view, width, 22);
        let y = view.viewport.borrow().tab_bar.y;
        let row = rendered.lines().nth(usize::from(y)).unwrap();
        assert!(!row.contains("tab/⇧tab"), "{row}");
        assert!(!row.contains("filter"), "{row}");
    }
}

#[test]
fn task_tabs_justify_labels_and_click_targets() {
    for width in [30, 60, 160] {
        let mut view = fixture();
        let rendered = screen(&view, width, 22);
        let area = Rect::new(0, 0, width, 22);
        let content = view.center_layout(area).header.inner(Margin::new(1, 0));
        let tabs = view.viewport.borrow().tabs.clone();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].1.x, content.x);
        assert_eq!(tabs[1].1.right(), content.right());
        let row = rendered.lines().nth(usize::from(tabs[0].1.y)).unwrap();
        assert_eq!(
            row.chars()
                .skip(usize::from(content.x))
                .take(10)
                .collect::<String>(),
            " Active 5 "
        );
        assert!(row.trim_end().ends_with("Inactive 1"), "{row}");
        for idx in [1, 0] {
            let rect = tabs[idx].1;
            for x in [rect.x, rect.right() - 1] {
                view.status_filter = 1 - idx;
                click_mouse(
                    &mut view,
                    mouse(MouseEventKind::Down(MouseButton::Left), x, rect.y),
                );
                assert_eq!(view.status_filter, idx);
                assert!(
                    view.visible_indices()
                        .iter()
                        .all(|&i| (view.rows[i].status == SessionStatus::Inactive) == (idx == 1))
                );
            }
        }
    }
}

#[test]
fn task_tabs_count_loaded_inactive_pages() {
    use crate::providers::{Provider, claude::ClaudeProvider};
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    for index in 0..21 {
        std::fs::write(
            project.join(format!("00000000-0000-0000-0000-{index:012}.jsonl")),
            include_str!("../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
    }
    let mut provider = ClaudeProvider::new(dir.path().into());
    let mut view = test_view(Grouping::Project, false);
    for (pages, loaded) in [(1, 10), (2, 20), (3, 21)] {
        let snapshot = provider.refresh(pages);
        assert_eq!(snapshot.sessions.len(), loaded);
        assert_eq!(snapshot.has_more, loaded < 21);
        view.set_sessions(snapshot.sessions, snapshot.has_more);
        let rendered = screen(&view, 100, 22);
        let y = view.viewport.borrow().tab_bar.y;
        let row = rendered.lines().nth(usize::from(y)).unwrap();
        assert!(row.contains(&format!("Inactive {loaded}")), "{row}");
    }
}

#[test]
fn mouse_tab_rects_match_visible_tabs_after_render() {
    let view = fixture();
    let rendered = screen(&view, 120, 40);
    let viewport = view.viewport.borrow();
    assert_eq!(viewport.tabs.len(), TASK_FILTERS.len());
    for (expected, &(idx, rect)) in viewport.tabs.iter().enumerate() {
        assert_eq!(idx, expected);
        assert_eq!(rect.height, 1);
        let row = rendered.lines().nth(usize::from(rect.y)).unwrap();
        let label = row
            .chars()
            .skip(usize::from(rect.x))
            .take(usize::from(rect.width))
            .collect::<String>();
        assert!(label.trim().starts_with(TASK_FILTERS[idx].0));
    }
}

#[test]
fn mouse_left_click_activates_tab_and_resets_scroll() {
    let mut view = fixture();
    screen(&view, 120, 40);
    let (idx, rect) = view.viewport.borrow().tabs[1];
    view.viewport.borrow_mut().scroll = 7;
    assert!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            rect.x,
            rect.y
        ))
        .is_none()
    );
    assert_eq!(view.status_filter, 0);
    assert_eq!(view.viewport.borrow().scroll, 7);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 0, 0))
            .is_none()
    );
    assert_eq!(view.status_filter, idx);
    assert_eq!(view.viewport.borrow().scroll, 0);
    assert_eq!(view.selected_row().unwrap().status, SessionStatus::Inactive);
}

#[test]
fn mouse_click_outside_tabs_changes_nothing() {
    let mut view = fixture();
    screen(&view, 120, 40);
    view.viewport.borrow_mut().scroll = 7;
    let selected = view.selected;
    assert!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0)
        )
        .is_none()
    );
    assert_eq!(view.status_filter, 0);
    assert_eq!(view.selected, selected);
    assert_eq!(view.viewport.borrow().scroll, 7);
}

#[test]
fn mouse_tabs_ignore_overlays() {
    for overlay in 0..4 {
        let mut view = fixture();
        screen(&view, 120, 40);
        let (_, rect) = view.viewport.borrow().tabs[1];
        match overlay {
            0 => {
                key(&mut view, KeyCode::Char('?'));
            }
            1 => {
                key(&mut view, KeyCode::Char('/'));
            }
            2 => {
                key(&mut view, KeyCode::Char('r'));
            }
            3 => {
                key(&mut view, KeyCode::Backspace);
            }
            _ => unreachable!(),
        }
        view.viewport.borrow_mut().scroll = 7;
        let selected = view.selected;
        assert!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y)
            )
            .is_none()
        );
        assert_eq!(view.status_filter, 0);
        assert_eq!(view.selected, selected);
        assert_eq!(view.viewport.borrow().scroll, 7);
    }
}

#[test]
fn mouse_tabs_ignore_other_events() {
    let mut view = fixture();
    screen(&view, 120, 40);
    let (_, rect) = view.viewport.borrow().tabs[1];
    view.viewport.borrow_mut().scroll = 7;
    let selected = view.selected;
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Down(MouseButton::Middle),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Moved,
        MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight,
    ] {
        assert!(view.handle_mouse(mouse(kind, rect.x, rect.y)).is_none());
        assert_eq!(view.status_filter, 0);
        assert_eq!(view.selected, selected);
        assert_eq!(view.viewport.borrow().scroll, 7);
    }
}

fn rendered_cell(rendered: &str, text: &str) -> (u16, u16) {
    rendered
        .lines()
        .enumerate()
        .find_map(|(y, line)| {
            let x = line.find(text)?;
            Some((line[..x].chars().count() as u16, y as u16))
        })
        .unwrap_or_else(|| panic!("missing rendered text: {text}"))
}

fn overflowing_details() -> View {
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.status = SessionStatus::Ready;
    selected.prompt = "A long prompt that wraps across several rows. ".repeat(16);
    selected.last_message = Some("First reply row\n\nSecond reply row".into());
    selected.transcript = Some(selected.cwd.join("session.jsonl"));
    view
}

fn details_panel(view: &View) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    view.render_at(buf.area, &mut buf, view.rows[view.selected].updated_at);
    let area = view.viewport.borrow().details;
    let mut panel = Buffer::empty(Rect::new(0, 0, area.width, area.height));
    for y in 0..area.height {
        for x in 0..area.width {
            panel[(x, y)] = buf[(area.x + x, area.y + y)].clone();
        }
    }
    panel
}

#[test]
fn mouse_log_wheel_shifts_rendered_rows_and_hides_live_time() {
    let (mut view, now) = log_fixture();
    let mut older = view.log[0].clone();
    older.body = "Older event".into();
    view.log.insert(0, older);
    view.set_log(view.log.clone());
    let before = log_list_screen(&view, 100, 32, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 32)).log;
    assert!(
        before
            .lines()
            .any(|line| line.contains("run tests") && line.ends_with("35s")),
        "{before}"
    );
    for _ in 0..view.log.len() {
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
        if view.log_scroll() == 0 {
            break;
        }
    }
    assert_eq!(view.log_scroll(), 0);
    let after = log_list_screen(&view, 100, 32, now);
    assert!(after.contains("Older event"), "{after}");
    assert!(!after.contains("run tests"));
    for y in rect.y + super::log::LOG_TOP..rect.bottom() - 3 {
        assert_eq!(
            before.lines().nth(y as usize),
            after.lines().nth(y as usize + 1)
        );
    }
    // A tool at the visible edge must use its recorded duration, even if unfinished.
    view.log[7] = view.log[8].clone();
    view.log[7].body = "Earlier unfinished tool".into();
    let after = log_list_screen(&view, 100, 32, now);
    assert!(
        after
            .lines()
            .any(|line| line.contains("Earlier unfinished tool") && line.ends_with("5s")),
        "{after}"
    );
    assert!(
        !after
            .lines()
            .any(|line| line.contains("Tool") && line.ends_with("35s"))
    );
    view.select_fixture_event(None);
    view.viewport.borrow_mut().log_scroll = 0;
    assert!(
        log_screen(&view, 100, 32, now)
            .lines()
            .any(|line| line.contains("run tests") && line.ends_with("35s"))
    );
}

#[test]
fn mouse_log_wheel_keeps_body_column_when_clock_leaves_window() {
    use crate::log::Kind;
    let (mut view, now) = log_fixture();
    let mut event = view.log.last().unwrap().clone();
    event.kind = Kind::Tool;
    event.ended_at = Some(event.started_at);
    event.previous_end = Some(event.started_at - Duration::seconds(12));
    view.log = (0..12)
        .map(|index| {
            let mut event = event.clone();
            event.body = format!("Event {index:02}");
            event.started_at -= Duration::seconds(11 - index);
            event.ended_at = Some(event.started_at);
            event.previous_end = Some(event.started_at - Duration::seconds(12));
            event
        })
        .collect();
    view.log.last_mut().unwrap().previous_end = None;
    view.set_log(view.log.clone());
    let before = log_list_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    assert!(
        before
            .lines()
            .any(|line| line.contains("Event 11") && line.ends_with("11:59:30")),
        "{before}"
    );
    assert!(
        before
            .lines()
            .any(|line| line.contains("Event 10") && line.ends_with("12s")),
        "{before}"
    );
    let initial_scroll = view.log_scroll();
    let (before_x, before_y) = rendered_cell(&before, "Event 10");
    for _ in 0..view.log.len() {
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
        if view.log_scroll() < initial_scroll {
            break;
        }
    }
    assert_eq!(view.log_scroll(), initial_scroll - 1);
    let after = log_list_screen(&view, 100, 25, now);
    assert!(!after.contains("Event 11"), "{after}");
    let (after_x, after_y) = rendered_cell(&after, "Event 10");
    assert_eq!(after_y, before_y + 1);
    assert_eq!(after_x, before_x);
}

#[test]
fn mouse_log_wheel_selects_one_event_per_report() {
    let (view, now) = log_fixture();
    log_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    let mut view = view;
    view.handle_mouse(mouse(
        MouseEventKind::ScrollUp,
        rect.x + 2,
        rect.y + super::log::LOG_TOP,
    ));
    assert_eq!(view.visible_log_event(), view.log.get(view.log.len() - 2));
    assert_eq!(view.log_scroll(), view.log_max_scroll());
    view.handle_mouse(mouse(
        MouseEventKind::ScrollDown,
        rect.x + 2,
        rect.y + super::log::LOG_TOP,
    ));
    assert_eq!(view.log_scroll(), view.log_max_scroll());
    assert_eq!(view.visible_log_event(), view.log.last());
}

#[test]
fn mouse_log_wheel_down_at_newest_stays_at_newest() {
    let (mut view, now) = log_fixture();
    let before = log_screen(&view, 100, 25, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 25)).log;
    view.handle_mouse(mouse(
        MouseEventKind::ScrollDown,
        rect.x + 2,
        rect.y + super::log::LOG_TOP,
    ));
    assert_eq!(before, log_screen(&view, 100, 25, now));
}

fn log_with_newer_event(view: &View, now: chrono::DateTime<Utc>) -> Vec<crate::log::Event> {
    let mut events = view.log.clone();
    let mut event = events.last().unwrap().clone();
    event.body = "brand new event".into();
    event.started_at = now - Duration::seconds(5);
    event.previous_end = Some(now - Duration::seconds(10));
    events.push(event);
    events
}

#[test]
fn new_log_events_keep_rows_still_while_mouse_is_held() {
    let (mut view, now) = log_fixture();
    let before = log_list_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 20,
        rect.y + 2,
    ));
    view.set_log(log_with_newer_event(&view, now));
    let after = log_list_screen(&view, 100, 25, now);
    assert!(!after.contains("brand new event"), "{after}");
    for body in ["run tests", "run a background job"] {
        assert_eq!(rendered_cell(&after, body), rendered_cell(&before, body));
    }
}

#[test]
fn new_log_events_while_dragging_copy_the_rows_on_screen() {
    let (mut view, now) = log_fixture();
    let before = log_screen(&view, 100, 25, now);
    let end_x =
        rendered_cell(&before, "run a background job").0 + "run a background job".len() as u16;
    let (x, y) = rendered_cell(&before, "failed check");
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    view.set_log(log_with_newer_event(&view, now));
    log_screen(&view, 100, 25, now);
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end_x, y + 2));
    log_screen(&view, 100, 25, now);
    let Some(Action::Copy { text, .. }) =
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end_x, y + 2))
    else {
        panic!("no copy");
    };
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    let before_row = before.lines().nth(usize::from(y)).unwrap();
    assert_eq!(lines[0], &before_row[usize::from(x)..]);
    assert!(lines[1].contains("Also inspect the tests"), "{text}");
    assert!(lines[2].contains("run a background job"), "{text}");
}

#[test]
fn new_log_events_keep_rows_still_off_the_newest_row() {
    let (mut view, now) = log_fixture();
    log_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
    let before = log_list_screen(&view, 100, 25, now);
    assert!(!before.contains("↓ Latest"), "{before}");
    view.set_log(log_with_newer_event(&view, now));
    let after = log_list_screen(&view, 100, 25, now);
    assert!(!after.contains("brand new event"), "{after}");
    let body = rendered_cell(&after, "run a background job");
    assert_eq!(body, rendered_cell(&before, "run a background job"));
    let jump = rendered_cell(&after, "↓ 1 New");
    let label = " ↓ 1 New ".width() as u16;
    assert_eq!(
        (jump.0, jump.1),
        (
            super::centering::centered_x(rect, super::log::log_row_areas(rect).0, label) + 1,
            rect.bottom()
        )
    );
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(Rect::new(0, 0, 100, 25), &mut buf, now);
    assert_eq!(buf[jump].fg, Color::Reset);
    assert_eq!(buf[jump].modifier, ratatui::style::Modifier::DIM);
}

/// The log fixture with a second task, and the row index of each.
fn two_task_log_fixture() -> (View, chrono::DateTime<Utc>, usize, usize) {
    let (mut view, now) = log_fixture();
    let mut other = view.rows[0].clone();
    other.id = session_key("other");
    other.updated_at = now - Duration::seconds(1);
    view.set_sessions(vec![view.rows[0].clone(), other], false);
    let index = |view: &View, id: &str| {
        view.rows
            .iter()
            .position(|row| row.id == session_key(id))
            .unwrap()
    };
    let (log, other) = (index(&view, "log"), index(&view, "other"));
    view.set_selected(log);
    (view, now, log, other)
}

/// Selects the task at `index` and loads `events` as its log.
fn show_task_log(view: &mut View, index: usize, events: Vec<crate::log::Event>) {
    view.set_selected(index);
    view.set_log(events);
    view.log_session = view.selected_id().cloned();
}

#[test]
fn switching_tasks_resets_log_to_newest_and_clears_unseen() {
    let (mut view, now, log, other) = two_task_log_fixture();
    log_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
    assert_eq!(view.visible_log_event(), view.log.get(view.log.len() - 2));
    let newer = log_with_newer_event(&view, now);
    show_task_log(&mut view, other, Vec::new());
    assert_eq!(view.log_scroll(), 0);
    assert!(view.log_selected.is_none());
    show_task_log(&mut view, log, newer);
    assert_eq!(view.log_scroll(), view.log_max_scroll());
    assert_eq!(view.visible_log_event(), view.log.last());
    let after = log_screen(&view, 100, 25, now);
    assert!(after.contains("brand new event"), "{after}");
    assert!(!after.contains("↓ 1 New"), "{after}");
    assert!(!after.contains("↓ Latest"), "{after}");
}

#[test]
fn task_search_returns_but_selected_detail_resets_to_newest() {
    let (mut view, now, log, other) = two_task_log_fixture();
    view.log[2].body = (1..=40)
        .map(|i| format!("Reply line {i}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let events = view.log.clone();
    {
        view.mouse_focus = Panel::LogList;
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
    };
    for letter in "tool".chars() {
        key(&mut view, KeyCode::Char(letter));
    }
    view.select_fixture_event(Some(events[2].clone()));
    view.viewport.borrow_mut().log_detail_scroll = 5;
    log_screen(&view, 100, 25, now);
    show_task_log(&mut view, other, Vec::new());
    assert!(!view.log_search.active());
    assert!(view.log_selected.is_none());
    show_task_log(&mut view, log, events);
    assert_eq!(view.log_search.query.text(), "tool");
    assert_eq!(view.visible_log_event(), view.log.last());
    assert_eq!(view.viewport.borrow().log_detail_scroll, 0);
    assert_eq!(view.log_scroll(), 0);
    assert!(log_screen(&view, 100, 25, now).contains("run tests"));
}

#[test]
fn jump_button_returns_to_the_newest_log_row() {
    let (mut view, now) = log_fixture();
    log_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(
        MouseEventKind::ScrollUp,
        rect.x + 2,
        rect.y + super::log::LOG_TOP,
    ));
    view.set_log(log_with_newer_event(&view, now));
    let scrolled = log_screen(&view, 100, 25, now);
    let (x, y) = rendered_cell(&scrolled, "↓ 1 New");
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y));
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 2, y));
    assert_eq!(view.log_scroll(), view.log_max_scroll());
    let after = log_screen(&view, 100, 25, now);
    assert!(after.contains("brand new event"), "{after}");
    assert!(
        !after.lines().nth(usize::from(y)).unwrap().contains('↑'),
        "{after}"
    );
}

#[test]
fn mouse_log_selection_tracks_selected_session_identity() {
    let (mut view, now) = log_fixture();
    log_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    let selected = view.log_selected;
    assert_eq!(view.visible_log_event(), view.log.get(view.log.len() - 2));
    let id = view.selected_id().unwrap().to_owned();
    let mut rows = view.rows.clone();
    rows.push(session(
        "recent",
        "Recent",
        SessionStatus::NeedsInput,
        -7200,
    ));
    view.set_sessions(rows.clone(), false);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(view.log_selected, selected);
    rows.retain(|row| row.id != id);
    for row in &mut rows {
        row.transcript = Some(row.cwd.join("session.jsonl"));
    }
    view.set_sessions(rows, false);
    assert_eq!(view.log_scroll(), 0);
    assert!(view.log_selected.is_none());
    view.set_log(view.log.clone());
    assert_eq!(view.visible_log_event(), view.log.last());
}

#[test]
fn mouse_details_wheel_scrolls_three_rows_per_report() {
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    let revealed = |view: &View| {
        view.viewport
            .borrow()
            .details_scroll
            .reveal
            .iter()
            .sum::<usize>()
    };
    assert_eq!(revealed(&view), 3);
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    assert_eq!(revealed(&view), 0);
}

#[test]
fn mouse_details_scroll_resets_with_selection_and_stays_across_tabs() {
    for code in [KeyCode::Down, KeyCode::End, KeyCode::PageDown, KeyCode::Tab] {
        let mut view = overflowing_details();
        details_panel(&view);
        let rect = view.viewport.borrow().details;
        view.scroll_content_row(true, false, rect.y);
        assert_eq!(
            view.viewport
                .borrow()
                .details_scroll
                .reveal
                .iter()
                .sum::<usize>(),
            1
        );
        key(&mut view, code);
        assert_eq!(
            view.viewport
                .borrow()
                .details_scroll
                .reveal
                .iter()
                .sum::<usize>(),
            0,
            "{code:?}"
        );
    }
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    let id = view.selected_id().unwrap().to_owned();
    view.select(view.selected);
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    let (_, tab) = view.viewport.borrow().tabs[1];
    click_mouse(
        &mut view,
        mouse(MouseEventKind::Down(MouseButton::Left), tab.x, tab.y),
    );
    assert_ne!(view.selected_id(), Some(&id));
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(
        view.viewport
            .borrow()
            .details_scroll
            .reveal
            .iter()
            .sum::<usize>(),
        3
    );
}

fn details_reveal(view: &View) -> Vec<usize> {
    view.viewport.borrow().details_scroll.reveal.clone()
}

#[test]
fn details_scroll_returns_with_its_task() {
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    let scrolled = details_reveal(&view);
    assert_eq!(scrolled.iter().sum::<usize>(), 3);
    let before = details_panel(&view);
    key(&mut view, KeyCode::Down);
    details_panel(&view);
    assert_eq!(details_reveal(&view).iter().sum::<usize>(), 0);
    key(&mut view, KeyCode::Up);
    assert_eq!(details_panel(&view), before);
    assert_eq!(details_reveal(&view), scrolled);
}

#[test]
fn details_body_changed_while_away_opens_folded() {
    let mut view = overflowing_details();
    view.rows[view.selected].last_message = Some(
        (1..=12)
            .map(|i| format!("Reply row {i}"))
            .collect::<Vec<_>>()
            .join("\n\n"),
    );
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    for _ in 0..8 {
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
        details_panel(&view);
    }
    let scrolled = details_reveal(&view);
    let prompt = 0;
    assert!(scrolled[prompt] > 0 && scrolled[1] > 0, "{scrolled:?}");
    let index = view.selected;
    key(&mut view, KeyCode::Down);
    details_panel(&view);
    view.rows[index].prompt = "A changed prompt that wraps across several rows. ".repeat(8);
    key(&mut view, KeyCode::Up);
    details_panel(&view);
    let mut expected = scrolled;
    expected[prompt] = 0;
    assert_eq!(details_reveal(&view), expected);
}

#[test]
fn details_scroll_preserves_background_cells_and_hyperlinks() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "Background output").unwrap();
    let mut view = overflowing_details();
    let selected = &mut view.rows[view.selected];
    selected.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: output.path().into(),
        started_at: selected.updated_at,
    });
    selected.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://example.com/pull/833".into(),
    });
    let now = selected.updated_at;
    let area = Rect::new(7, 4, 120, 40);
    let mut before = Buffer::empty(area);
    view.render_at(area, &mut before, now);
    let viewport = view.viewport.borrow();
    let body = viewport.details;
    let output_y = (body.y..body.bottom())
        .find(|&y| {
            (body.x..body.right())
                .map(|x| before[(x, y)].symbol())
                .collect::<String>()
                .contains("Background output")
        })
        .unwrap();
    let (pr, _) = viewport
        .details_links
        .iter()
        .find(|(_, path)| path == &PathBuf::from("https://example.com/pull/833"))
        .unwrap();
    let pr = *pr;
    assert!(
        before[(pr.x, pr.y)]
            .symbol()
            .contains("https://example.com/pull/833")
    );
    assert!(matches!(
        before[(pr.x, pr.y)].diff_option,
        ratatui::buffer::CellDiffOption::ForcedWidth(_)
    ));
    let prompt = viewport.details_scroll.bodies[0].start as u16 + body.y;
    assert!(viewport.details_scroll.hidden[0] > 0);
    drop(viewport);
    view.scroll_content_row(true, false, prompt);
    let mut after = Buffer::empty(area);
    view.render_at(area, &mut after, now);
    assert!(view.viewport.borrow().details_scroll.reveal[0] > 0);
    let after_y = (body.y..body.bottom())
        .find(|&y| {
            (body.x..body.right())
                .map(|x| after[(x, y)].symbol())
                .collect::<String>()
                .contains("Background output")
        })
        .unwrap();
    for offset in 0..=1 {
        for x in body.x..body.right() {
            assert_eq!(after[(x, after_y - offset)], before[(x, output_y - offset)]);
        }
    }
    for x in pr.x..pr.right() {
        assert_eq!(after[(x, pr.y)], before[(x, pr.y)]);
    }
}

#[test]
fn details_scroll_tracks_session_identity_across_refreshes() {
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    view.scroll_content_row(true, false, rect.y);
    let id = view.selected_id().unwrap().to_owned();
    let mut rows = view.rows.clone();
    rows.push(session(
        "recent",
        "Recent",
        SessionStatus::NeedsInput,
        -7200,
    ));
    view.set_sessions(rows.clone(), false);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(
        view.viewport
            .borrow()
            .details_scroll
            .reveal
            .iter()
            .sum::<usize>(),
        1
    );
    rows.retain(|row| row.id != id);
    view.set_sessions(rows, false);
    assert_eq!(
        view.viewport
            .borrow()
            .details_scroll
            .reveal
            .iter()
            .sum::<usize>(),
        0
    );
}

#[test]
fn mouse_list_click_and_double_click_keep_focus_on_second_task() {
    let mut view = fixture();
    let rendered = screen(&view, 160, 40);
    let second = view.selectable_indices()[1];
    let id = view.rows[second].id.clone();
    let (x, y) = rendered_cell(
        &rendered,
        &view.rows[second]
            .display_title()
            .chars()
            .take(12)
            .collect::<String>(),
    );
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(click_mouse(&mut view, click).is_none());
    assert_eq!(view.selected, second);
    assert_eq!(click_mouse(&mut view, click), None);
    assert!(!view.input_box.focused);
    assert_eq!(view.selected_id(), Some(&id));
}

#[test]
fn mouse_list_slow_or_split_clicks_only_select() {
    let mut view = fixture();
    let rendered = screen(&view, 160, 40);
    let [first, second] = [0, 1].map(|i| view.selectable_indices()[i]);
    let [first_click, second_click] = [first, second].map(|row| {
        let (x, y) = rendered_cell(
            &rendered,
            &view.rows[row]
                .display_title()
                .chars()
                .take(12)
                .collect::<String>(),
        );
        mouse(MouseEventKind::Down(MouseButton::Left), x, y)
    });
    assert!(click_mouse(&mut view, first_click).is_none());
    assert!(click_mouse(&mut view, second_click).is_none());
    assert_eq!(view.selected, second);
    view.last_click.as_mut().unwrap().1 -= DOUBLE_CLICK_INTERVAL;
    assert!(click_mouse(&mut view, second_click).is_none());
    assert_eq!(view.selected, second);
}

#[test]
fn mouse_list_group_and_gap_clicks_do_nothing() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let selected = view.selected;
    let (x, y) = rendered_cell(&rendered, "Pinned");
    let list = view.center_layout(Rect::new(0, 0, 120, 40)).list;
    let next_group = rendered
        .lines()
        .position(|line| {
            line.chars()
                .take(usize::from(list.right()))
                .collect::<String>()
                .contains("/tmp/project")
        })
        .unwrap() as u16;
    for row in [y, next_group - 1] {
        assert!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, row)
            )
            .is_none()
        );
        assert_eq!(view.selected, selected);
    }
}

#[test]
fn mouse_list_wheel_uses_keyboard_selection() {
    let mut view = fixture();
    screen(&view, 120, 40);
    let list = view.center_layout(Rect::new(0, 0, 120, 40)).list;
    let (x, y) = (list.x, list.y);
    let initial = view.selected;
    key(&mut view, KeyCode::Down);
    let next = view.selected;
    key(&mut view, KeyCode::Up);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, x, y))
            .is_none()
    );
    assert_eq!(view.selected, next);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, x, y))
            .is_none()
    );
    assert_eq!(view.selected, initial);
}

#[test]
fn mouse_tab_wheel_switches_filters_without_wrapping() {
    let mut view = fixture();
    screen(&view, 120, 40);
    let (_, rect) = view.viewport.borrow().tabs[0];
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    assert_eq!(view.status_filter, 0);
    view.viewport.borrow_mut().scroll = 7;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    assert_eq!(view.status_filter, 1);
    assert_eq!(view.viewport.borrow().scroll, 0);
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    assert_eq!(view.status_filter, 0);
    for _ in 0..TASK_FILTERS.len() + 1 {
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    }
    assert_eq!(view.status_filter, TASK_FILTERS.len() - 1);
}

#[test]
fn mouse_list_show_more_click_activates_once() {
    let mut view = fixture();
    view.status_filter = TASK_FILTERS.len() - 1;
    view.has_more = true;
    view.reconcile_selection();
    let rendered = screen(&view, 120, 40);
    let (x, y) = rendered_cell(&rendered, "Show more");
    assert!(matches!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y)
        ),
        Some(Action::ShowMore)
    ));
    assert_eq!(view.selected, usize::MAX);
    assert!(view.loading);
    assert!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y)
        )
        .is_none()
    );
}

#[test]
fn mouse_tab_wheel_accepts_space_between_tabs() {
    let mut view = fixture();
    screen(&view, 120, 40);
    let (_, rect) = view.viewport.borrow().tabs[0];
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.right(), rect.y));
    assert_eq!(view.status_filter, 1);
}

#[test]
fn mouse_list_click_uses_last_render_scroll_offset() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        (0..50)
            .map(|i| {
                session(
                    &format!("task-{i}"),
                    &format!("Task {i:02}"),
                    SessionStatus::Ready,
                    i,
                )
            })
            .collect(),
        false,
    );
    key(&mut view, KeyCode::End);
    let rendered = screen(&view, 120, 40);
    let target = view.selectable_indices()[48];
    let (x, y) = rendered_cell(
        &rendered,
        &view.rows[target]
            .display_title()
            .chars()
            .take(12)
            .collect::<String>(),
    );
    assert!(view.viewport.borrow().scroll > 0);
    view.viewport.borrow_mut().scroll = 0;
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(click_mouse(&mut view, click).is_none());
    assert_eq!(view.selected, target);
    assert_eq!(click_mouse(&mut view, click), None);
    assert!(!view.input_box.focused);
    assert_eq!(view.selected_id(), Some(&view.rows[target].id));
}

#[test]
fn mouse_list_ignores_overlays() {
    for overlay in [
        KeyCode::Char('?'),
        KeyCode::Char('/'),
        KeyCode::Char('r'),
        KeyCode::Backspace,
    ] {
        let mut view = fixture();
        let rendered = screen(&view, 160, 40);
        let second = view.selectable_indices()[1];
        let (x, y) = rendered_cell(
            &rendered,
            &view.rows[second]
                .display_title()
                .chars()
                .take(12)
                .collect::<String>(),
        );
        key(&mut view, overlay);
        let selected = view.selected;
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::ScrollDown,
            MouseEventKind::ScrollUp,
        ] {
            assert!(view.handle_mouse(mouse(kind, x, y)).is_none());
            assert_eq!(view.selected, selected);
        }
    }
}

fn session(id: &str, title: &str, status: SessionStatus, age: i64) -> Session {
    Session {
        id: session_key(id),
        parent: None,
        cwd: "/tmp/project".into(),
        name: None,
        title: title.into(),
        prompt: title.into(),
        detail_sources: Default::default(),
        prompt_images: Vec::new(),
        queued: Vec::new(),
        prompt_at: None,
        turn_messages: 0,
        last_record_at: None,
        model_label: None,
        model: None,
        effort: None,
        git_branch: None,
        pr: None,
        status,
        process: None,
        updated_at: Utc::now() - Duration::seconds(age),
        started_at: None,
        last_message: None,
        last_message_at: None,
        activity: None,
        ask: None,
        pending_questions: Vec::new(),
        background_job: None,
        background: Vec::new(),
        tasks: Vec::new(),
        last_prompt: None,
        transcript: None,
        archived: false,
    }
}
fn screen(view: &View, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    buf.content
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn key(view: &mut View, code: KeyCode) -> Option<Action> {
    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn assert_empty_task_list(view: &View) {
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);
    view.render_center_rows(area, &mut buf, Utc::now());
    assert!(buf.content.iter().all(|cell| cell.symbol() == " "));
    let rendered = screen(view, 80, 12);
    for message in ["Loading tasks…", "No tasks yet", "No matching tasks"] {
        assert!(!rendered.contains(message), "{message}");
    }
}
fn fixture() -> View {
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(
        vec![
            session("42", "Current task", SessionStatus::Ready, -3600),
            session(
                "43",
                "A longer task title that uses the available column",
                SessionStatus::Ready,
                330,
            ),
            session("44", "Needs an answer", SessionStatus::NeedsInput, 7230),
            session("45", "Unloaded task", SessionStatus::Inactive, 172830),
            session("46", "Working task", SessionStatus::Working, 86430),
            session("47", "Failed task", SessionStatus::NeedsInput, 3630),
        ],
        false,
    );
    view.set_pinned(&[session_key("42"), session_key("44")]);
    let selected = view
        .rows
        .iter()
        .position(|s| s.id == session_key("42"))
        .unwrap();
    view.set_selected(selected);
    view
}
fn simple_fixture() -> View {
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(
        vec![
            session("42", "Current task", SessionStatus::Ready, 120),
            session("43", "Needs an answer", SessionStatus::NeedsInput, 120),
            session("44", "Unloaded task", SessionStatus::Inactive, 120),
        ],
        false,
    );
    view
}
#[test]
fn live_center_columns_without_voice_and_with_needs_input_error_delta() {
    assert_eq!(
        screen(&fixture(), 160, 22),
        include_str!("../../tests/ui_snapshots/live_center_columns.txt")
    );
}
#[test]
fn live_center_help_two_columns_with_pin_and_narrow_tabs_delta() {
    let mut view = simple_fixture();
    key(&mut view, KeyCode::Char('?'));
    assert_eq!(
        screen(&view, 60, 24),
        include_str!("../../tests/ui_snapshots/live_center_help_two_columns.txt")
    );
    // Both widths fit the task tabs and shortcut columns.
    let narrow = screen(&view, 56, 24);
    let wide = screen(&view, 60, 24);
    assert_eq!(
        narrow.lines().skip(3).collect::<Vec<_>>(),
        wide.lines().skip(3).collect::<Vec<_>>()
    );
}
#[test]
fn active_tab_lists_all_non_inactive_sessions() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![
            session("working", "Working session", SessionStatus::Working, 1),
            session("ready", "Ready session", SessionStatus::Ready, 1),
            session(
                "needs-input",
                "Needs input session",
                SessionStatus::NeedsInput,
                1,
            ),
            session("inactive", "Inactive session", SessionStatus::Inactive, 1),
        ],
        false,
    );
    let rendered = screen(&view, 100, 16);
    assert!(rendered.contains("Working session"));
    assert!(rendered.contains("Ready session"));
    assert!(rendered.contains("Needs input session"));
    assert!(!rendered.contains("Inactive session"));
    assert_eq!(view.visible_indices().len(), 3);
    assert!(rendered.contains("Active 3"));
    view.set_sessions(
        vec![session(
            "inactive",
            "Inactive session",
            SessionStatus::Inactive,
            1,
        )],
        false,
    );
    let rendered = screen(&view, 100, 16);
    assert!(rendered.contains("Active 0"));
    assert_empty_task_list(&view);
}

#[test]
fn tabs_cycle_in_display_order_and_filter_by_label() {
    let mut view = fixture();
    assert_eq!(TASK_FILTERS.len(), 2);
    assert_eq!(view.visible_indices().len(), 5);
    for code in [
        KeyCode::Tab,
        KeyCode::BackTab,
        KeyCode::BackTab,
        KeyCode::Tab,
    ] {
        key(&mut view, code);
        let inactive = view.status_filter == 1;
        assert_eq!(view.visible_indices().len(), if inactive { 1 } else { 5 });
        assert!(
            view.visible_indices()
                .iter()
                .all(|&i| (view.rows[i].status == SessionStatus::Inactive) == inactive)
        );
    }
}

#[test]
fn refresh_follows_selected_working_session_to_inactive() {
    let mut view = fixture();
    view.select(
        view.rows
            .iter()
            .position(|s| s.id == session_key("46"))
            .unwrap(),
    );
    assert_eq!(view.selected_id(), Some(&session_key("46")));
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("46"))
        .unwrap()
        .status = SessionStatus::Inactive;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    assert_eq!(view.selected_id(), Some(&session_key("46")));
    assert_eq!(view.tab_selections[1], Some(session_key("46")));
}

#[test]
fn refresh_returns_to_active_when_selected_working_session_disappears() {
    let mut view = fixture();
    view.select(
        view.rows
            .iter()
            .position(|s| s.id == session_key("46"))
            .unwrap(),
    );
    assert_eq!(view.selected_id(), Some(&session_key("46")));
    let rows = view
        .rows
        .iter()
        .filter(|s| s.id != session_key("46"))
        .cloned()
        .collect();

    view.set_sessions(rows, true);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some(&session_key("44")));
}

#[test]
fn refresh_keeps_selected_session_on_active_when_ready_becomes_working() {
    let mut view = fixture();
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Working;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some(&session_key("42")));
}

#[test]
fn refresh_follows_selected_ready_session_to_inactive() {
    let mut view = fixture();
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Inactive;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    assert_eq!(view.selected_id(), Some(&session_key("42")));
    assert_eq!(view.tab_selections[1], Some(session_key("42")));
    key(&mut view, KeyCode::Tab);
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_id(), Some(&session_key("42")));
}

#[test]
fn refresh_child_leaving_tab_selects_visible_parent_or_falls_back() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        for tab in [0, 1] {
            for removed in [false, true] {
                for parent_visible in [false, true] {
                    let mut view = test_view(Grouping::Project, true);
                    let status = if tab == 0 {
                        SessionStatus::Ready
                    } else {
                        SessionStatus::Inactive
                    };
                    let other_status = if tab == 0 {
                        SessionStatus::Inactive
                    } else {
                        SessionStatus::Ready
                    };
                    let mut parent = session("parent", "Parent", status, 10);
                    parent.id.provider = provider;
                    let mut child = session("child", "Child", status, 20);
                    child.id.provider = provider;
                    child.parent = Some(parent.id.clone());
                    let other = session("other", "Unrelated task", status, 120);
                    view.status_filter = tab;
                    view.set_sessions(vec![parent.clone(), child.clone(), other.clone()], false);
                    let index = view.rows.iter().position(|s| s.id == child.id).unwrap();
                    view.select(index);
                    child.status = other_status;
                    if !parent_visible {
                        parent.status = other_status;
                    }
                    let expected = if parent_visible && !removed {
                        child.id.clone()
                    } else if parent_visible {
                        parent.id.clone()
                    } else {
                        other.id.clone()
                    };
                    let mut rows = vec![parent, other];
                    if !removed {
                        rows.push(child);
                    }

                    view.set_sessions(rows, false);

                    assert_eq!(
                        view.status_filter, tab,
                        "{provider:?}, removed={removed}, parent_visible={parent_visible}"
                    );
                    assert_eq!(
                        view.selected_id(),
                        Some(&expected),
                        "{provider:?}, tab={tab}, removed={removed}, parent_visible={parent_visible}"
                    );
                }
            }
        }
    }
}

#[test]
fn refresh_keeps_tab_when_selected_session_is_hidden_only_by_search() {
    let mut view = fixture();
    key(&mut view, KeyCode::Char('/'));
    for c in "Current".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    assert_eq!(view.selected_id(), Some(&session_key("42")));
    let mut rows = view.rows.clone();
    let selected = rows.iter_mut().find(|s| s.id == session_key("42")).unwrap();
    selected.title = "Other task".into();
    selected.prompt = "Other task".into();

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.search.text(), "Current");
    assert_eq!(view.selected_id(), None);
}

#[test]
fn live_center_status_filter() {
    let mut view = simple_fixture();
    key(&mut view, KeyCode::BackTab);
    assert_eq!(
        screen(&view, 40, 12),
        include_str!("../../tests/ui_snapshots/live_center_status_filter.txt")
    );
}
#[test]
fn live_center_search_active() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![session("42", "Task to find", SessionStatus::Ready, 120)],
        false,
    );
    key(&mut view, KeyCode::Char('/'));
    for c in "find".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    let bin = crate::usage::bin_of(Utc::now());
    view.set_usage(
        &session_key("42"),
        (bin - 100..=bin + 1)
            .map(|bin| {
                (
                    bin,
                    crate::usage::Bin {
                        input: 100,
                        output: 0,
                    },
                )
            })
            .collect(),
    );
    assert_eq!(
        screen(&view, 88, 16),
        include_str!("../../tests/ui_snapshots/live_center_search_active.txt")
    );
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 88, 16)), Some((14, 13)));
}
#[test]
fn show_more_is_only_listed_on_inactive() {
    let mut view = fixture();
    view.set_sessions(view.rows.clone(), true);
    for (tab, (_, statuses)) in TASK_FILTERS.iter().enumerate() {
        view.status_filter = tab;
        view.reconcile_selection();
        let inactive = *statuses == [SessionStatus::Inactive];
        assert_eq!(view.selectable_indices().contains(&usize::MAX), inactive);
        assert_eq!(screen(&view, 160, 22).contains("Show more"), inactive);
    }
}

#[test]
fn leaving_inactive_reconciles_show_more_selection() {
    let mut view = fixture();
    view.set_sessions(view.rows.clone(), true);
    key(&mut view, KeyCode::BackTab);
    key(&mut view, KeyCode::End);
    assert_eq!(view.selected, usize::MAX);

    key(&mut view, KeyCode::Tab);

    assert_eq!(view.selected_id(), Some(&session_key("46")));
}

#[test]
fn a_tab_without_its_chosen_task_selects_the_longest_waiting_task() {
    let mut view = fixture();

    key(&mut view, KeyCode::Tab);
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    assert_eq!(view.selected_id(), Some(&session_key("45")));
    key(&mut view, KeyCode::BackTab);

    assert_eq!(view.selected_id(), Some(&session_key("46")));
}

#[test]
fn the_first_load_selects_the_longest_waiting_task() {
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(fixture().rows, true);

    assert_eq!(view.selected_id(), Some(&session_key("46")));
}

#[test]
fn inactive_without_its_chosen_task_selects_show_more() {
    let mut view = fixture();
    view.set_sessions(view.rows.clone(), true);

    key(&mut view, KeyCode::BackTab);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    assert_eq!(view.selected, usize::MAX);
}

#[test]
fn returning_to_a_tab_restores_its_selection() {
    let mut view = fixture();
    while view.selected_id() != Some(&session_key("46")) {
        key(&mut view, KeyCode::Down);
    }

    key(&mut view, KeyCode::Tab);
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    key(&mut view, KeyCode::BackTab);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some(&session_key("46")));
}

#[test]
fn returning_to_inactive_through_an_empty_tab_restores_its_selection() {
    let mut view = fixture();
    let mut rows = view.rows.clone();
    for row in &mut rows {
        row.status = SessionStatus::Inactive;
    }
    view.set_sessions(rows, true);
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    while view.selected_id() != Some(&session_key("45")) {
        key(&mut view, KeyCode::Down);
    }
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_id(), Some(&session_key("45")));
}

#[test]
fn empty_active_tab_does_not_page_inactive_sessions() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![session("42", "Recent task", SessionStatus::Inactive, 120)],
        true,
    );
    assert_empty_task_list(&view);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(!view.loading);
}

#[test]
fn live_center_show_more() {
    let mut view = View {
        status_filter: 1,
        ..test_view(Grouping::Project, true)
    };
    view.set_sessions(
        vec![session("42", "Recent task", SessionStatus::Inactive, 120)],
        true,
    );
    key(&mut view, KeyCode::Home);
    key(&mut view, KeyCode::Down);
    assert_eq!(
        screen(&view, 80, 12),
        include_str!("../../tests/ui_snapshots/live_center_show_more.txt")
    );
    assert_eq!(key(&mut view, KeyCode::Enter), Some(Action::ShowMore));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    let rendered = screen(&view, 80, 12);
    assert!(rendered.contains("Show more"));
    assert!(!rendered.contains("Loading more…"));
}
#[test]
fn group_headers_have_no_count() {
    for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
        for pinned in [false, true] {
            for filter in [0] {
                let mut view = test_view(grouping, false);
                view.set_sessions(
                    vec![
                        session("ready", "Ready task", SessionStatus::Ready, 0),
                        session("working", "Working task", SessionStatus::Working, 0),
                    ],
                    false,
                );
                if pinned {
                    view.set_pinned(&[session_key("ready"), session_key("working")]);
                }
                view.status_filter = filter;
                let area = Rect::new(0, 0, 80, 12);
                let mut buf = Buffer::empty(area);
                view.render_center_rows(area, &mut buf, Utc::now());
                let entries = view.center_rows(&view.selectable_indices(), grouping);
                let mut headers = 0;
                for (y, entry) in entries.iter().enumerate() {
                    let navigation::CenterRow::Group(index) = entry else {
                        continue;
                    };
                    headers += 1;
                    let expected = if pinned {
                        "Pinned"
                    } else {
                        match grouping {
                            Grouping::Project => "/tmp/project",
                            Grouping::Status => view.rows[*index].status.label(),
                            Grouping::Model => view.rows[*index].model_name(),
                        }
                    };
                    let heading = (0..area.width)
                        .map(|x| buf[(x, y as u16)].symbol())
                        .collect::<String>();
                    assert_eq!(
                        heading.trim(),
                        expected,
                        "{grouping:?}, pinned={pinned}, filter={filter}"
                    );
                    assert!(
                        buf[((area.width - expected.len() as u16) / 2, y as u16)]
                            .modifier
                            .contains(ratatui::style::Modifier::DIM)
                    );
                }
                assert!(headers > 0);
            }
        }
    }
}

#[test]
fn agents_overview_model_grouping() {
    let mut view = test_view(Grouping::Model, false);
    let rows = [
        ("Older task", Some("model-a"), 4),
        ("Other model", Some("model-b"), 3),
        ("Recent task", Some("model-a"), 2),
        ("Legacy task", None, 1),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (title, model, age))| {
        let mut row = session(&i.to_string(), title, SessionStatus::Ready, age);
        row.model = model.map(str::to_owned);
        row
    })
    .collect();
    view.set_sessions(rows, false);
    key(&mut view, KeyCode::Home);
    assert_eq!(
        screen(&view, 100, 24),
        include_str!("../../tests/ui_snapshots/agents_overview_model_grouping.txt")
    );
}
#[test]
fn agents_overview_recent_sessions_twelve_rows_delta() {
    let mut view = View {
        status_filter: 1,
        ..test_view(Grouping::Project, true)
    };
    view.set_sessions(
        (1..=12)
            .map(|i| {
                let mut row = session(
                    &i.to_string(),
                    &format!("Task {i}"),
                    SessionStatus::Inactive,
                    (13 - i) * 60,
                );
                row.cwd = "/".into();
                row
            })
            .collect(),
        true,
    );
    key(&mut view, KeyCode::Home);
    let rendered = screen(&view, 80, 60);
    assert!(rendered.contains("Active 0"));
    assert!(rendered.contains("Inactive 12"));
    assert_eq!(
        rendered
            .lines()
            .nth(2)
            .unwrap()
            .split('│')
            .next()
            .unwrap()
            .trim(),
        "/"
    );
    let expected = include_str!("../../tests/ui_snapshots/agents_overview_recent_sessions.txt");
    // The reference loads ten rows. Twelve rows change only counts and append Task 2 / Task 1.
    for (actual, expected) in rendered
        .lines()
        .skip(3)
        .take(10)
        .zip(expected.lines().skip(4).take(10))
    {
        assert_eq!(
            actual
                .split('│')
                .next()
                .unwrap()
                .trim_end()
                .split("Inactive")
                .next()
                .unwrap()
                .trim_end(),
            expected
                .split('│')
                .next()
                .unwrap()
                .trim_end()
                .split("Inactive")
                .next()
                .unwrap()
                .trim_end()
        );
    }
    assert!(rendered.contains("Task 2 "));
    assert!(rendered.contains("Task 1 "));
    assert!(rendered.contains("Show more"));
}
#[test]
fn search_text_wins_and_refresh_preserves_selection_and_state() {
    let mut view = fixture();
    key(&mut view, KeyCode::Char('/'));
    for c in "task".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    key(&mut view, KeyCode::Char('j'));
    assert_eq!(view.search.text(), "taskj");
    key(&mut view, KeyCode::Backspace);
    assert_eq!(view.search.text(), "task");
    let id = view.selected_id().unwrap().to_owned();
    view.set_sessions(view.rows.clone(), false);
    assert_eq!(view.selected_id(), Some(&id));
    assert!(view.searching);
    assert_eq!(view.search.text(), "task");
    key(&mut view, KeyCode::Esc);
    assert!(!view.searching);
    assert_eq!(key(&mut view, KeyCode::Esc), Some(Action::Back));
}
#[test]
fn actions_and_hide_move_to_next_visible_row() {
    let mut view = fixture();
    assert_eq!(
        key(&mut view, KeyCode::Char('o')),
        Some(Action::OpenTerminal(session_key("42")))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('f')),
        Some(Action::Fork(session_key("42")))
    );
    assert_eq!(
        key(&mut view, KeyCode::Right),
        Some(Action::Open(session_key("42")))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('x')),
        Some(Action::Unsupported("Stop"))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin {
            id: session_key("42"),
            pinned: false
        })
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('h')),
        Some(Action::Hide(session_key("42")))
    );
    assert_eq!(view.selected_id(), Some(&session_key("44")));
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert!(screen(&view, 88, 16).contains("Rename ❯"));
    assert_eq!(view.rename.text(), "Needs an answer");
    key(&mut view, KeyCode::Esc);

    assert_eq!(
        key(&mut view, KeyCode::Char('g')),
        Some(Action::PersistGrouping(Grouping::Status))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('n')),
        Some(Action::NewDraft {
            cwd: "/tmp/project".into()
        })
    );
}
#[test]
fn paging_tracks_headings_and_never_wraps() {
    let mut view = View {
        status_filter: 1,
        ..test_view(Grouping::Project, true)
    };
    view.set_sessions(
        (0..30)
            .map(|i| {
                let mut row = session(
                    &i.to_string(),
                    &format!("Task {i}"),
                    SessionStatus::Inactive,
                    i,
                );
                if i >= 10 {
                    row.cwd = "/tmp/second-project".into();
                }
                row
            })
            .collect(),
        true,
    );
    key(&mut view, KeyCode::Home);
    screen(&view, 110, 18);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), Some(&session_key("11")));
    key(&mut view, KeyCode::PageUp);
    assert_eq!(view.selected_id(), Some(&session_key("0")));
    for _ in 0..40 {
        key(&mut view, KeyCode::PageDown);
        screen(&view, 110, 18);
    }
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::Down);
    assert_eq!(view.selected_id(), Some(&session_key("0")));
}
#[test]
fn styles_and_unicode_clipping() {
    use super::style::selection_style_for;
    assert_eq!(
        selection_style_for(false, true, true),
        Style::default()
            .bold()
            .fg(Color::Indexed(18))
            .bg(Color::Indexed(111))
    );
    assert_eq!(
        selection_style_for(false, true, false),
        Style::default()
            .bold()
            .fg(Color::Indexed(24))
            .bg(Color::Indexed(233))
    );
    assert_eq!(
        selection_style_for(true, true, false),
        Style::default().bold().reversed()
    );
    assert_eq!(
        selection_style_for(false, false, false),
        Style::default().bold().reversed()
    );
    assert_eq!(
        super::line_truncation::truncate_line_with_ellipsis_if_overflow(
            Line::from("你好 world"),
            4
        )
        .to_string(),
        "你…"
    );
    let view = fixture();
    for width in 0..100 {
        for height in 0..7 {
            let area = Rect::new(5, 8, width, height);
            view.render(area, &mut Buffer::empty(area));
        }
    }
}

#[test]
fn details_include_attention_and_clip_messages() {
    let mut row = session("details", "Details task", SessionStatus::NeedsInput, 120);
    row.model = Some("claude-opus".into());
    row.effort = Some("high".into());
    row.git_branch = Some("main".into());
    row.process = Some(crate::model::Process {
        pid: 123,
        label: None,
        started_at: None,
    });
    row.prompt = "prompt ".repeat(40);
    row.last_message = Some("message ".repeat(60));
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 0;
    view.set_sessions(vec![row], false);
    let rendered = screen(&view, 160, 50);
    for label in [
        "claude-opus",
        "project",
        "Needs attention",
        "Open task to review.",
        "Waiting for your response.",
        "Update",
    ] {
        assert!(rendered.contains(label), "{label}");
    }
    let position = |label: &str| {
        rendered
            .lines()
            .position(|line| !line.contains("Group:") && line.contains(label))
    };
    assert!(position("Prompt") < position("Update"));
    assert!(position("Prompt") < position("Needs attention"));
    let viewport = view.viewport.borrow();
    let folder = viewport
        .details_links
        .iter()
        .find(|(_, path)| path == &PathBuf::from("/tmp/project"))
        .unwrap()
        .0;
    assert_eq!(folder.y, viewport.details.bottom());
    assert!(
        viewport
            .field_targets
            .iter()
            .any(|(_, field)| *field == Field::Model)
    );
    assert!(!rendered.contains("main"));
    assert!(!rendered.contains("Reasoning"));
}

#[test]
fn empty_states_notices_and_help_only_handle_close_keys() {
    let mut view = test_view(Grouping::Project, true);
    assert_empty_task_list(&view);
    view.set_loading(true);
    assert_empty_task_list(&view);
    view.set_sessions(vec![session("a", "Alpha", SessionStatus::Ready, 1)], false);
    key(&mut view, KeyCode::Char('/'));
    key(&mut view, KeyCode::Char('z'));
    assert_empty_task_list(&view);
    key(&mut view, KeyCode::Esc);
    view.set_notice(Some("Notice".into()));
    let lines = screen(&view, 80, 12);
    let lines = lines.lines().collect::<Vec<_>>();
    let notice_row = view.center_layout(Rect::new(0, 0, 80, 12)).list.bottom() - 1;
    assert_eq!(
        lines[usize::from(notice_row)]
            .split(['│', '├'])
            .next()
            .unwrap()
            .trim_end(),
        " Notice"
    );
    key(&mut view, KeyCode::Left);
    assert!(view.notice.is_none());
    key(&mut view, KeyCode::Char('?'));
    assert_eq!(key(&mut view, KeyCode::Char('n')), None);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        None
    );
    assert!(view.help);
    key(&mut view, KeyCode::Esc);
    assert!(!view.help);
}

#[test]
fn marker_pos_follows_the_selected_row_and_clears_on_help() {
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(
        vec![
            session("a", "First", SessionStatus::Ready, 10),
            session("b", "Second", SessionStatus::Ready, 20),
        ],
        false,
    );
    key(&mut view, KeyCode::Home);
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    let (x, y) = view.marker_pos().unwrap();
    assert_eq!(buf[(x, y)].symbol(), "○");
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    view.render(area, &mut buf);
    let (x2, y2) = view.marker_pos().unwrap();
    assert_eq!((x2, y2), (x, y + 1));
    assert_eq!(buf[(x2, y2)].symbol(), "○");
    view.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    view.render(area, &mut buf);
    assert_eq!(view.marker_pos(), None);
}

#[test]
fn details_name_a_missing_prompt() {
    let mut row = session("blank", "Blank task", SessionStatus::Ready, 10);
    row.prompt = String::new();
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    assert!(screen(&view, 160, 24).contains("No prompt available."));
}

#[test]
fn rename_edits_title_and_commits_trimmed_text() {
    let mut view = fixture();
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert!(screen(&view, 88, 16).contains("Rename ❯ Current task"));
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 88, 16)), Some((22, 2)));
    for c in " updated  ".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Rename {
            id: session_key("42"),
            name: "Current task updated".into()
        })
    );
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
}

#[test]
fn rename_locks_navigation_cancels_and_ignores_empty_enter() {
    let mut view = fixture();
    let hints = view.center_footer_hints().to_string();
    key(&mut view, KeyCode::Char('r'));
    assert_eq!(
        view.center_footer_hints().to_string(),
        "esc back  enter rename"
    );
    for code in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Tab,
        KeyCode::Right,
        KeyCode::F(3),
    ] {
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.selected_id(), Some(&session_key("42")));
    }
    for _ in 0..12 {
        key(&mut view, KeyCode::Backspace);
    }
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    for c in " e\u{301}".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    key(&mut view, KeyCode::Backspace);
    assert!(screen(&view, 88, 16).contains("Rename ❯"));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    key(&mut view, KeyCode::Esc);
    assert_eq!(view.center_footer_hints().to_string(), hints);
    assert_eq!(view.rows[view.selected].display_title(), "Current task");
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
}

#[test]
fn rename_survives_status_refresh_but_drops_removed_or_hidden_target() {
    let mut view = fixture();
    view.status_filter = TASK_FILTERS
        .iter()
        .position(|(label, _)| *label == "Active")
        .unwrap();
    key(&mut view, KeyCode::Char('r'));
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some(&session_key("42")));
    assert!(screen(&view, 88, 16).contains("Rename ❯ Current task"));
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    view.hide(&session_key("42"));
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
    let mut view = fixture();
    key(&mut view, KeyCode::Char('r'));
    view.set_sessions(vec![], true);
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert!(view.cursor_pos(Rect::new(0, 0, 88, 16)).is_none());
}

#[test]
fn live_center_rename_active() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![session("42", "Task to find", SessionStatus::Ready, 120)],
        false,
    );
    key(&mut view, KeyCode::Char('r'));
    assert_eq!(
        screen(&view, 88, 16),
        include_str!("../../tests/ui_snapshots/live_center_rename_active.txt")
    );
    let area = Rect::new(0, 0, 88, 16);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    assert_eq!(buf[(2, 2)].fg, Color::Cyan);
    assert!(
        buf[(2, 2)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert_eq!(
        buf[(view.center_layout(area).search.right() - 1, 2)].style(),
        Style::default()
            .fg(Color::Reset)
            .bg(Color::Reset)
            .underline_color(Color::Reset)
            .patch(super::style::active_tab_style())
    );
}

#[test]
fn delete_dialog_defaults_to_cancel_and_requires_enter() {
    let mut view = fixture();
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(!view.confirm_dialog.as_ref().unwrap().confirm);
    assert_eq!(
        view.center_footer_hints().to_string(),
        "↑/↓ move  enter select  esc back"
    );
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(view.confirm_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete(session_key("42")))
    );
    assert!(view.confirm_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    key(&mut view, KeyCode::Esc);
    assert!(view.confirm_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::End), None);
    assert!(view.confirm_dialog.as_ref().unwrap().confirm);
    for code in [
        KeyCode::Char('o'),
        KeyCode::Char('n'),
        KeyCode::Right,
        KeyCode::Char(' '),
    ] {
        assert_eq!(key(&mut view, code), None);
        assert!(view.confirm_dialog.is_some());
    }
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete(session_key("42")))
    );
}

#[test]
fn delete_dialog_locks_list_and_supports_movement_keys() {
    let mut view = fixture();
    key(&mut view, KeyCode::Backspace);
    for code in [
        KeyCode::Down,
        KeyCode::Up,
        KeyCode::Char('j'),
        KeyCode::Char('k'),
        KeyCode::PageDown,
        KeyCode::PageUp,
        KeyCode::End,
        KeyCode::Home,
        KeyCode::Char('o'),
        KeyCode::Char('n'),
    ] {
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.selected_id(), Some(&session_key("42")));
    }
    for c in ['n', 'p', 'j', 'k'] {
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(view.selected_id(), Some(&session_key("42")));
    }
}

#[test]
fn delete_live_rows_open_the_dialog_and_ignore_show_more() {
    let mut view = fixture();
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: 123,
        label: None,
        started_at: None,
    });
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.confirm_dialog.is_some());
    assert!(screen(&view, 120, 24).contains("This stops any running work in this task"));
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete(session_key("42")))
    );
    view.has_more = true;
    view.selected = usize::MAX;
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.confirm_dialog.is_none());
}

fn lifecycle_fixture() -> View {
    let mut view = fixture();
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            stop: true,
            archive: true,
            ..Capabilities::default()
        },
    )]));
    view
}

#[test]
fn archive_dialog_shows_codex_copy_and_two_accepts_at_once() {
    let mut view = lifecycle_fixture();
    assert_eq!(key(&mut view, KeyCode::Char('a')), None);
    let rendered = screen(&view, 120, 48);
    assert!(rendered.contains("Archive “Current task”?"));
    let dialog = rendered
        .lines()
        .filter_map(|line| line.split('│').nth(1))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(dialog.contains("then archives them. Their history can be restored from the"));
    assert!(rendered.contains("2. Archive task and child agents"));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    key(&mut view, KeyCode::Char('a'));
    assert_eq!(
        key(&mut view, KeyCode::Char('2')),
        Some(Action::Archive(session_key("42")))
    );
    key(&mut view, KeyCode::Char('a'));
    key(&mut view, KeyCode::Down);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Archive(session_key("42")))
    );
}

#[test]
fn archive_and_stop_follow_capabilities() {
    let mut view = lifecycle_fixture();
    assert_eq!(
        key(&mut view, KeyCode::Char('x')),
        Some(Action::Stop(session_key("42")))
    );
    let mut view = fixture();
    assert_eq!(
        key(&mut view, KeyCode::Char('a')),
        Some(Action::Unsupported("Archive"))
    );
}

#[test]
fn live_center_delete_dialog() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 1;
    view.set_sessions(
        vec![session("44", "Unloaded task", SessionStatus::Inactive, 120)],
        false,
    );
    key(&mut view, KeyCode::Backspace);
    assert_eq!(
        screen(&view, 88, 16),
        include_str!("../../tests/ui_snapshots/live_center_delete_dialog.txt")
    );
    let area = Rect::new(0, 0, 88, 16);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    assert!(
        buf[(6, 5)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(buf[(6, 7)].modifier.contains(ratatui::style::Modifier::DIM));
    // A painted cell reports unset colours as Reset, so compare against a painted default cell.
    let mut expected = ratatui::buffer::Cell::default();
    expected.set_style(super::style::selection_style());
    assert_eq!(buf[(6, 9)].style(), expected.style());
    for width in 0..100 {
        for height in 0..16 {
            let area = Rect::new(5, 8, width, height);
            view.render(area, &mut Buffer::empty(area));
        }
    }
}

#[test]
fn delete_dialog_number_shortcuts_accept_at_once() {
    let mut view = fixture();
    key(&mut view, KeyCode::Backspace);
    assert!(view.confirm_dialog.is_some());
    assert_eq!(key(&mut view, KeyCode::Char('1')), None);
    assert!(view.confirm_dialog.is_none(), "1 accepts Cancel at once");
    key(&mut view, KeyCode::Backspace);
    assert_eq!(
        key(&mut view, KeyCode::Char('2')),
        Some(Action::Delete(session_key("42")))
    );
    assert!(view.confirm_dialog.is_none());
}

#[test]
fn removal_dialog_centers_on_the_panels_above_the_prompt() {
    let mut view = fixture();
    key(&mut view, KeyCode::Backspace);
    for width in [120, 200] {
        let area = Rect::new(0, 0, width, 48);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        let layout = view.center_layout(area);
        let right = if width >= super::render::WIDE_LAYOUT_COLUMNS {
            layout.log.right()
        } else {
            layout.details.right()
        };
        let (left, top) = (0..area.height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == "┌")
            .expect("dialog border is drawn");
        let dialog_right = (left..width)
            .find(|&x| buf[(x, top)].symbol() == "┐")
            .unwrap()
            + 1;
        let bottom = (top..area.height)
            .find(|&y| buf[(left, y)].symbol() == "└")
            .unwrap()
            + 1;
        assert!(
            (left - layout.list.x).abs_diff(right - dialog_right) <= 1,
            "width {width}: {left}..{dialog_right} in {}..{right}",
            layout.list.x
        );
        assert!(
            (top - layout.list.y).abs_diff(layout.list.bottom() - bottom) <= 1,
            "width {width}: {top}..{bottom} in {}..{}",
            layout.list.y,
            layout.list.bottom()
        );
    }
}

#[test]
fn task_titles_take_the_next_accent_when_first_shown_and_keep_it() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let mut view = test_view(Grouping::Project, true);
    let first = vec![
        session("a", "Alpha task", SessionStatus::Ready, 10),
        session("b", "Bravo task", SessionStatus::Ready, 20),
        session("c", "Charlie task", SessionStatus::Ready, 30),
    ];
    view.set_sessions(first.clone(), false);
    key(&mut view, KeyCode::Home);
    let title_color = |view: &View, title: &str| {
        let area = Rect::new(0, 0, 80, 12);
        let mut buf = Buffer::empty(area);
        view.render_center_rows(area, &mut buf, Utc::now());
        (0..area.height)
            .find_map(|y| {
                let line: String = (0..area.width).map(|x| buf[(x, y)].symbol()).collect();
                let x = line.find(title)?;
                Some(buf[(line[..x].chars().count() as u16, y)].fg)
            })
            .expect(title)
    };
    assert_eq!(title_color(&view, "Bravo task"), style::thread_color(1));
    assert_eq!(title_color(&view, "Charlie task"), style::thread_color(2));
    let mut second = first;
    second.push(session("d", "Delta task", SessionStatus::Ready, 0));
    view.set_sessions(second, false);
    assert_eq!(title_color(&view, "Delta task"), style::thread_color(3));
    assert_eq!(title_color(&view, "Bravo task"), style::thread_color(1));
    assert_eq!(title_color(&view, "Charlie task"), style::thread_color(2));
}

#[test]
fn details_folder_and_pr_use_rule_styles_and_keep_links() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let mut row = session("pr", "PR task", SessionStatus::Inactive, 120);
    row.git_branch = Some("hidden-branch".into());
    row.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    let mut view = test_view(Grouping::Project, true);
    view.status_filter = 1;
    view.set_sessions(vec![row], false);
    for width in [120, 200] {
        let area = Rect::new(0, 0, width, 40);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        let viewport = view.viewport.borrow();
        let links = viewport.details_links.clone();
        let y = viewport.details.bottom();
        drop(viewport);
        let (folder, _) = links
            .iter()
            .find(|(_, path)| path == &PathBuf::from("/tmp/project"))
            .unwrap();
        let (pr, url) = links
            .iter()
            .find(|(_, path)| path != &PathBuf::from("/tmp/project"))
            .unwrap();
        assert_eq!(folder.y, y);
        assert_eq!(pr.y, y);
        assert!(pr.right() < folder.x);
        for (offset, symbol) in "project".chars().enumerate() {
            let cell = &buf[(folder.x + offset as u16, y)];
            assert_eq!(cell.symbol(), symbol.to_string());
            assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
            assert!(!cell.modifier.contains(ratatui::style::Modifier::BOLD));
        }
        for x in pr.x..pr.right() {
            let cell = &buf[(x, y)];
            let mut reference = ratatui::buffer::Cell::default();
            reference.set_style(view.title_style(&session_key("pr")));
            assert_eq!(cell.style(), reference.style());
            if x == pr.x {
                assert!(cell.symbol().contains("\x1b]8;;"));
                assert!(cell.symbol().contains(url.to_str().unwrap()));
            }
        }
        view.underline_links(&mut buf);
        assert!(buf[(folder.x, y)].symbol().contains("\x1b[4:4m"));
        assert!(
            buf[(folder.x, y)]
                .modifier
                .contains(ratatui::style::Modifier::DIM)
        );
        assert_eq!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), folder.x, y)
            ),
            Some(Action::OpenPath("/tmp/project".into()))
        );
        assert_eq!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), pr.x, y)
            ),
            Some(Action::OpenPath(url.clone()))
        );
    }
}

#[test]
fn log_model_and_effort_use_dim_style_and_picker_targets() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    for use_colors in [true, false] {
        for width in [120, 200] {
            for human_name in [None, Some("Opus 5.5")] {
                let mut row = session("model", "Model task", SessionStatus::Inactive, 120);
                row.model = Some("claude-opus-5-5".into());
                row.model_label = human_name.map(str::to_owned);
                row.effort = Some("medium".into());
                row.transcript = Some("/tmp/model.jsonl".into());
                let mut view = test_view(Grouping::Project, use_colors);
                view.status_filter = 1;
                view.set_sessions(vec![row], false);
                let area = Rect::new(0, 0, width, 40);
                let mut buf = Buffer::empty(area);
                view.render(area, &mut buf);
                let layout = view.center_layout(area);
                let y = if width >= 150 {
                    layout.log.bottom()
                } else {
                    layout.log.y - 1
                };
                let targets = view.viewport.borrow().field_targets.clone();
                for (field, text) in [
                    (Field::Model, human_name.unwrap_or("claude-opus-5-5")),
                    (Field::Effort, "medium"),
                ] {
                    let rect = targets.iter().find(|(_, value)| *value == field).unwrap().0;
                    assert_eq!(rect.y, y);
                    assert_eq!(rect.width as usize, text.len());
                    assert_eq!(
                        rect.x,
                        if field == Field::Model {
                            (if width >= 150 {
                                layout.log.x - 2
                            } else {
                                area.x
                            }) + 2
                        } else {
                            area.right() - 2 - rect.width
                        }
                    );
                    for (offset, symbol) in text.chars().enumerate() {
                        let cell = &buf[(rect.x + offset as u16, y)];
                        assert_eq!(cell.symbol(), symbol.to_string());
                        assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
                        assert!(!cell.modifier.contains(ratatui::style::Modifier::BOLD));
                    }
                }
                if use_colors && width == 120 && human_name.is_none() {
                    let text: String = (layout.log.x..layout.log.right())
                        .map(|x| buf[(x, y)].symbol())
                        .collect();
                    let styles: String = (layout.log.x..layout.log.right())
                        .map(|x| {
                            if buf[(x, y)].modifier.contains(ratatui::style::Modifier::DIM) {
                                'd'
                            } else {
                                '.'
                            }
                        })
                        .collect();
                    assert_eq!(
                        format!("{text}\n{styles}\n"),
                        include_str!("../../tests/ui_snapshots/details_model_style.txt")
                    );
                }
            }
        }
    }
}

#[test]
fn details_status_folder_and_optional_pr_snapshot() {
    let mut row = session("pr", "PR task", SessionStatus::Inactive, 120);
    row.git_branch = Some("hidden-branch".into());
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 1;
    view.set_sessions(vec![row.clone()], false);
    let render = |view: &View| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        view.render(buf.area, &mut buf);
        let viewport = view.viewport.borrow();
        let y = viewport.details.bottom();
        let layout = view.center_layout(buf.area);
        let mut text = String::new();
        for x in layout.details.x..layout.details.right() {
            let symbol = buf[(x, y)].symbol();
            let symbol = symbol
                .rsplit("\x1b\\")
                .find(|part| !part.is_empty() && !part.starts_with("\x1b]8"))
                .unwrap_or("");
            text.push_str(symbol.split("\x1b]8").next().unwrap());
        }
        text + "\n"
    };
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_branch.txt")
    );
    assert_eq!(view.viewport.borrow().details_links.len(), 1);
    row.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    view.set_sessions(vec![row.clone()], false);
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_branch_pr.txt")
    );
    for width in [80, 120, 160, 200] {
        let mut buf = Buffer::empty(Rect::new(5, 8, width, 30));
        view.render(buf.area, &mut buf);
        let viewport = view.viewport.borrow();
        for (rect, _) in &viewport.details_links {
            assert_eq!(rect.y, viewport.details.bottom());
            assert!(rect.right() <= buf.area.right());
        }
    }
    row.pr = None;
    view.set_sessions(vec![row], false);
    let text = render(&view);
    assert!(!text.contains("#833"));
    assert!(!text.contains("hidden-branch"));
    assert_eq!(view.viewport.borrow().details_links.len(), 1);
}

#[test]
fn details_activity_and_message_are_independent() {
    let now = Utc::now();
    let mut row = session("activity", "Activity task", SessionStatus::Working, 7);
    row.prompt_at = Some(now - Duration::seconds(4200));
    row.turn_messages = 42;
    row.last_record_at = Some(now - Duration::seconds(3595));
    row.activity = Some(crate::model::Activity {
        kind: crate::log::Kind::Tool,
        label: "Run checks".into(),
        at: now - Duration::seconds(7),
    });
    row.last_message = Some("Checking the project".into());
    row.last_message_at = Some(now - Duration::hours(1));
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row.clone()], false);
    let render = |view: &View| {
        let area = Rect::new(0, 0, 40, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        buf.content
            .chunks(40)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end_matches('\n')
            .to_owned()
    };
    let rendered = render(&view);
    assert!(rendered.contains("             Last activity        7s ago"));
    assert!(rendered.contains("                 Update           1h ago"));
    assert!(rendered.find("Update").unwrap() < rendered.find("Run checks").unwrap());
    assert!(rendered.contains("Checking the project"));
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_activity_working.txt")
    );
    row.activity.as_mut().unwrap().label =
        "Run checks on every changed surface in the project".into();
    view.set_sessions(vec![row.clone()], false);
    let clipped = render(&view);
    assert!(clipped.contains("Run checks on every changed surface in…"));
    assert!(!clipped.contains("surface in the project"));
    // Both sections remain visible regardless of status or timestamp order.
    row.activity.as_mut().unwrap().label = "Run checks".into();
    row.status = SessionStatus::Ready;
    view.set_sessions(vec![row.clone()], false);
    assert!(render(&view).contains("             Last activity        7s ago"));
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_activity_ready.txt")
    );
    row.status = SessionStatus::Working;
    row.last_message_at = Some(now - Duration::seconds(3));
    view.set_sessions(vec![row.clone()], false);
    let replied = render(&view);
    assert!(replied.contains("             Last activity        7s ago"));
    assert!(replied.contains("                 Update           3s ago"));
    assert_eq!(
        replied,
        include_str!("../../tests/ui_snapshots/details_activity_replied.txt")
    );
}

#[test]
fn details_activity_and_message_keep_copy_links_and_short_panel_budgets() {
    let now = Utc::now();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("reports")).unwrap();
    for name in [
        "reports/activity.md",
        "reports/reply.md",
        "reports/prompt.md",
        "reports/queued.md",
    ] {
        std::fs::write(dir.path().join(name), "report").unwrap();
    }
    let mut row = session("activity", "Activity task", SessionStatus::Ready, 0);
    row.cwd = dir.path().into();
    row.prompt = "Request reports/prompt.md".into();
    row.queued = vec!["Queued reports/queued.md".into()];
    let activity = "Check reports/activity.md";
    let message = "Reply reports/reply.md";
    row.activity = Some(crate::model::Activity {
        kind: crate::log::Kind::Tool,
        label: activity.into(),
        at: now,
    });
    row.last_message = Some(message.into());
    row.last_message_at = Some(now);
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let rendered = log_screen(&view, 120, 40, now);
    for (body, label, path) in [
        (activity, "Copied latest activity", "reports/activity.md"),
        (message, "Copied update", "reports/reply.md"),
        (
            "Request reports/prompt.md",
            "Copied prompt",
            "reports/prompt.md",
        ),
    ] {
        view.last_copy_click = None;
        assert_eq!(
            double_click_text(&mut view, &rendered, body.split_once(' ').unwrap().0),
            Some(Action::Copy {
                text: body.into(),
                label: label.into()
            })
        );
        assert_eq!(
            click_text(&mut view, &rendered, path),
            Some(Action::OpenPath(dir.path().join(path)))
        );
    }
    assert_eq!(
        click_text(&mut view, &rendered, "reports/queued.md"),
        Some(Action::OpenPath(dir.path().join("reports/queued.md")))
    );
    view.rows[view.selected].queued.clear();
    view.rows[view.selected].prompt = "Prompt row  \n".repeat(20);
    view.rows[view.selected].last_message = Some("Reply row  \n".repeat(20));
    for height in [16, 4] {
        let area = Rect::new(0, 0, 40, height);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        let viewport = view.viewport.borrow();
        if height == 16 {
            assert_eq!(viewport.details_scroll.fallback_max, None);
            assert_eq!(viewport.details_copy.len(), 3);
            assert!(viewport.details_scroll.hidden[0] > 0);
            assert!(viewport.details_scroll.hidden[1] > 0);
            assert_eq!(viewport.details_scroll.hidden[2], 0);
        } else {
            assert!(viewport.details_scroll.fallback_max.unwrap() > 0);
        }
    }
}

#[test]
fn details_activity_uses_tool_color() {
    use ratatui::style::Modifier;
    let now = Utc::now();
    let mut row = session("activity", "Activity task", SessionStatus::Ready, 0);
    row.activity = Some(crate::model::Activity {
        kind: crate::log::Kind::Tool,
        label: "Run checks".into(),
        at: now - Duration::minutes(2),
    });
    row.last_message = Some("Checking the project".into());
    row.last_message_at = Some(now - Duration::minutes(5));
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 60, 24);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, now);
    let header = "                       Last activity                  2m ago";
    let y = (0..area.height)
        .find(|y| {
            header
                .chars()
                .enumerate()
                .all(|(x, ch)| buf[(x as u16, *y)].symbol() == ch.to_string())
        })
        .expect("activity header");
    for (offset, character) in header.chars().enumerate() {
        let cell = &buf[(offset as u16, y)];
        assert_eq!(cell.symbol(), character.to_string());
        assert_eq!(cell.fg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::DIM));
    }
    for (offset, character) in "Run checks".chars().enumerate() {
        let cell = &buf[(offset as u16, y + 1)];
        assert_eq!(cell.symbol(), character.to_string());
        assert_eq!(cell.fg, crate::theme::text_accent_color());
        assert!(!cell.modifier.contains(Modifier::DIM));
    }
}

#[test]
fn details_message_uses_model_color_under_markdown() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    use ratatui::style::Modifier;
    let now = Utc::now();
    let mut row = session("message", "Message task", SessionStatus::Ready, 0);
    row.activity = Some(crate::model::Activity {
        kind: crate::log::Kind::Tool,
        label: "Run checks".into(),
        at: now - Duration::minutes(5),
    });
    row.last_message = Some("Plain `code` **bold** *italic*\n\nSecond paragraph".into());
    row.last_message_at = Some(now - Duration::minutes(2));
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 60, 24);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, now);
    let header = "                           Update                     2m ago";
    let y = (0..area.height)
        .find(|y| {
            header
                .chars()
                .enumerate()
                .all(|(x, ch)| buf[(x as u16, *y)].symbol() == ch.to_string())
        })
        .expect("message header");
    for (offset, character) in header.chars().enumerate() {
        let cell = &buf[(offset as u16, y)];
        assert_eq!(cell.symbol(), character.to_string());
        assert_eq!(cell.fg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::DIM));
    }
    for (offset, character) in "Plain code bold italic".chars().enumerate() {
        let cell = &buf[(offset as u16, y + 1)];
        assert_eq!(cell.symbol(), character.to_string());
        assert_eq!(
            cell.fg,
            if (6..10).contains(&offset) {
                crate::theme::inline_code_color()
            } else {
                Color::Yellow
            }
        );
        assert!(!cell.modifier.contains(Modifier::DIM));
        if (11..15).contains(&offset) {
            assert!(cell.modifier.contains(Modifier::BOLD));
        }
        if offset >= 16 {
            assert!(cell.modifier.contains(Modifier::ITALIC));
        }
    }
    for (offset, character) in "Second paragraph".chars().enumerate() {
        let cell = &buf[(offset as u16, y + 3)];
        assert_eq!(cell.symbol(), character.to_string());
        assert_eq!(cell.fg, Color::Yellow);
    }
}

#[test]
fn prompt_header_omits_message_counts_and_clamps_elapsed_time() {
    let now: chrono::DateTime<Utc> = "2026-10-12T12:00:00Z".parse().unwrap();
    for (status, prompt_at, last_record_at, count, expected) in [
        (
            SessionStatus::Working,
            Some(now - Duration::seconds(45)),
            None,
            1,
            "                 Prompt              45s",
        ),
        (
            SessionStatus::Ready,
            Some(now),
            Some(now - Duration::seconds(1)),
            0,
            "                 Prompt               0s",
        ),
        (
            SessionStatus::Working,
            Some(now + Duration::seconds(1)),
            None,
            0,
            "                 Prompt               0s",
        ),
        (
            SessionStatus::Ready,
            None,
            Some(now),
            0,
            "                 Prompt",
        ),
    ] {
        let mut row = session("turn", "Turn task", status, 0);
        row.prompt_at = prompt_at;
        row.last_record_at = last_record_at;
        row.turn_messages = count;
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        let area = Rect::new(0, 0, 40, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        let header = buf
            .content
            .chunks(40)
            .find(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_start()
                    .starts_with("Prompt")
            })
            .unwrap();
        assert_eq!(
            header
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end(),
            expected
        );
        for cell in &header[..expected.chars().count()] {
            assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
        }
    }
}

mod details_header {
    use super::*;
    use ratatui::style::Modifier;

    fn rule(view: &View, now: chrono::DateTime<Utc>) -> (Buffer, Rect) {
        rule_at_width(view, now, 112)
    }

    fn rule_at_width(view: &View, now: chrono::DateTime<Utc>, width: u16) -> (Buffer, Rect) {
        let area = Rect::new(0, 0, width, 25);
        let layout = view.center_layout(area);
        let y = layout.header.y + 1;
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        (buf, Rect::new(layout.details.x, y, layout.details.width, 1))
    }

    fn label(buf: &Buffer, rule: Rect) -> String {
        (rule.x..rule.right())
            .map(|x| buf[(x, rule.y)].symbol())
            .collect::<String>()
            .trim_matches(['─', '╶', '╴'])
            .trim()
            .to_owned()
    }

    #[test]
    fn elapsed_wall_time_follows_redraw_for_every_row_kind() {
        let now = "2026-10-13T12:00:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        for provider in [ProviderId::Claude, ProviderId::Codex] {
            for child in [false, true] {
                for (age, expected) in [
                    (-1, "0s"),
                    (0, "0s"),
                    (45, "45s"),
                    (60, "1m 0s"),
                    (725, "12m 5s"),
                    (3599, "59m 59s"),
                    (3600, "1h 0m"),
                    (11520, "3h 12m"),
                    (86399, "23h 59m"),
                    (86400, "1d 0h"),
                    (183600, "2d 3h"),
                ] {
                    let mut row = session("header", "Fix login bug", SessionStatus::Working, 0);
                    row.id.provider = provider;
                    row.parent = child.then(|| SessionKey {
                        provider: ProviderId::Claude,
                        id: "parent".into(),
                    });
                    row.started_at = Some(now - Duration::seconds(age));
                    let mut view = test_view(Grouping::Project, true);
                    let id = row.id.clone();
                    let parent = session("parent", "Parent", SessionStatus::Ready, 0);
                    view.set_sessions(vec![parent, row], false);
                    view.selected = view.rows.iter().position(|row| row.id == id).unwrap();
                    for advance in [0, 86401] {
                        let (buf, rule) = rule(&view, now + Duration::seconds(advance));
                        let layout = view.center_layout(buf.area);
                        let bottom = Rect::new(
                            rule.x,
                            view.viewport.borrow().details.bottom(),
                            rule.width,
                            1,
                        );
                        let header = label(&buf, rule);
                        assert!(label(&buf, bottom).contains("Working"));
                        assert!(header.contains("Fix login bug"));
                        assert!(bottom.y > layout.header.bottom());
                        if advance == 0 {
                            assert!(header.ends_with(expected), "{header}");
                        } else {
                            assert!(header.contains("d "), "{header}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn title_alone_is_on_top_in_wide_and_narrow_layouts() {
        let row = session("header", "Header task", SessionStatus::Working, 0);
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        for width in [112, 200] {
            let (buf, rule) = rule_at_width(&view, Utc::now(), width);
            assert_eq!(label(&buf, rule), "Header task");
        }
    }

    #[test]
    fn narrow_and_wide_truncate_the_title_on_top() {
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let mut row = session(
            "header",
            &"升级命令中心".repeat(20),
            SessionStatus::Working,
            0,
        );
        row.started_at = Some(now - Duration::seconds(11520));
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        for width in [112, 200] {
            let (buf, rule) = rule_at_width(&view, now, width);
            let header = label(&buf, rule);
            assert!(header.starts_with('升'), "{header}");
            assert!(header.contains("…"), "{header}");
        }
    }

    #[test]
    fn title_and_status_keep_their_styles() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        for status in [
            SessionStatus::Working,
            SessionStatus::Ready,
            SessionStatus::NeedsInput,
            SessionStatus::Inactive,
        ] {
            let mut view = test_view(Grouping::Project, true);
            view.status_filter = match status {
                SessionStatus::NeedsInput => 0,
                SessionStatus::Inactive => 1,
                _ => 0,
            };
            view.set_sessions(vec![session("header", "Header task", status, 0)], false);
            let (buf, rule) = rule(&view, Utc::now());
            let header = label(&buf, rule);
            assert_eq!(header, "Header task");
            let start = rule.x + (rule.width - (header.width() as u16 + 2)) / 2 + 1;
            let title = &buf[(start, rule.y)];
            assert_eq!(title.fg, view.title_color(&session_key("header")));
            assert!(title.modifier.contains(Modifier::BOLD));
            assert!(!title.modifier.contains(Modifier::DIM));
            let bottom = view.viewport.borrow().details.bottom();
            let start = rule.x - 2 + (rule.width + 3 - (status.label().len() as u16 + 2)) / 2 + 1;
            for (offset, symbol) in status.label().chars().enumerate() {
                let cell = &buf[(start + offset as u16, bottom)];
                assert_eq!(cell.symbol(), symbol.to_string());
                assert_eq!(cell.fg, ratatui::style::Color::Reset);
                assert!(cell.modifier.contains(Modifier::BOLD));
                assert!(!cell.modifier.contains(Modifier::DIM));
            }
        }
    }

    #[test]
    fn truncated_title_keeps_full_width() {
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let mut row = session(
            "header",
            "A very long session title that exceeds the panel width",
            SessionStatus::Working,
            0,
        );
        row.prompt = "Current prompt".into();
        row.started_at = Some(now - Duration::seconds(11520));
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row.clone()], false);
        let (buf, rule) = rule(&view, now);
        let header = label(&buf, rule);
        assert!(header.starts_with("A very long"), "{header}");
        assert!(header.contains("…"), "{header}");
        assert_eq!(
            details_at(row, 15, now),
            include_str!("../../tests/ui_snapshots/details_header_elapsed.txt")
        );
    }

    #[test]
    fn truncates_title_on_top_border() {
        let mut row = session(
            "header",
            "A very long session title that exceeds the panel width",
            SessionStatus::Working,
            0,
        );
        row.prompt = "Current prompt".into();
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row.clone()], false);
        let (buf, rule) = rule(&view, Utc::now());
        let header = label(&buf, rule);
        let rendered = queued_details(row, 15);
        assert!(header.ends_with('…'), "{header}");
        assert_eq!(
            rendered,
            include_str!("../../tests/ui_snapshots/details_header_truncated.txt")
        );
    }
    #[test]
    fn borders_keep_title_status_elapsed_activity_and_log_jump_separate() {
        let (mut view, now) = log_fixture();
        view.select_fixture_event(Some(view.log[6].clone()));
        view.rows[view.selected].started_at = Some(now - Duration::seconds(11520));
        view.viewport.borrow_mut().log_unseen = 1;
        view.set_activity([crate::usage::bin_of(now)].into());
        for width in [112, 200] {
            for with_log in [true, false] {
                view.rows[view.selected].transcript =
                    with_log.then(|| "/tmp/task-log.jsonl".into());
                let (buf, top) = rule_at_width(&view, now, width);
                let layout = view.center_layout(buf.area);
                let bottom = view.viewport.borrow().details.bottom();
                let expected = " Task log example ";
                // The title centers on the whole segment between the separator's joint and the
                // log's joint or the screen's edge, not the space left of the time.
                let segment = top.width + if width >= 150 { 4 } else { 3 };
                let start = top.x + (segment - expected.width() as u16) / 2 - 2;
                for (offset, symbol) in expected.chars().enumerate() {
                    assert_eq!(
                        buf[(start + offset as u16, top.y)].symbol(),
                        symbol.to_string()
                    );
                }
                assert!((top.x..start).all(|x| buf[(x, top.y)].symbol() == "─"));
                // The time ends a space before the log's joint or the screen's edge.
                let time_end = top.right() - u16::from(width < 150);
                assert!(
                    (start + expected.width() as u16..time_end - " 3h 12m ".len() as u16)
                        .all(|x| buf[(x, top.y)].symbol() == "─")
                );
                let bottom_text = (top.x..top.right())
                    .map(|x| buf[(x, bottom)].symbol())
                    .collect::<String>();
                let status_start = (usize::from(top.width + 3) - " Working ".len()) / 2 - 2;
                assert_eq!(
                    bottom_text
                        .chars()
                        .skip(status_start)
                        .take(" Working ".len())
                        .collect::<String>(),
                    " Working "
                );
                assert!(bottom_text.contains("project"));
                for x in time_end - 6..time_end {
                    let cell = &buf[(x, top.y)];
                    assert!(cell.modifier.contains(Modifier::DIM));
                    assert!(!cell.modifier.contains(Modifier::BOLD));
                }
                assert!(label(&buf, top).ends_with(" 3h 12m"));
                assert!(!bottom_text.contains("3h 12m"));
                assert!(!bottom_text.contains("Task log example"));
                assert!(!label(&buf, top).contains("Working"));
                if !layout.log.is_empty() {
                    let strip = &buf[(layout.list.right() - 1, bottom)];
                    assert!(
                        strip
                            .symbol()
                            .chars()
                            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                    );
                    if width >= super::super::render::WIDE_LAYOUT_COLUMNS {
                        assert_eq!(view.viewport.borrow().log_jump.y, layout.log.bottom());
                        assert!(view.viewport.borrow().log_jump.x >= top.right());
                    }
                }
            }
        }
    }

    #[test]
    fn centered_title_truncates_before_elapsed_is_dropped() {
        let (mut view, now) = log_fixture();
        view.rows[view.selected].started_at = Some(now - Duration::seconds(11520));
        for columns in 10..=100 {
            let (buf, top) = rule_at_width(&view, now, columns);
            let rule = Rect::new(top.x, view.viewport.borrow().details.bottom(), top.width, 1);
            let width = rule.width;
            let text = (rule.x..rule.right())
                .map(|x| buf[(x, rule.y)].symbol())
                .collect::<String>();
            assert!(!text.contains("3h 12m"));
            let top_text = (top.x..top.right())
                .map(|x| buf[(x, top.y)].symbol())
                .collect::<String>();
            // The centered title keeps elapsed's width and its spaces clear on both sides, plus
            // a title cell, two spaces and two rule cells.
            let elapsed_width = " 3h 12m ".len() as u16;
            let reserved = if width + 3 >= 2 * elapsed_width + 7 {
                assert!(top_text.ends_with(" 3h 12m╶"), "width {width}: {top_text}");
                2 * elapsed_width
            } else {
                assert!(!top_text.contains("3h 12m"), "width {width}: {top_text}");
                0
            };
            // The rule runs from the separator's joint to the screen's edge or the log's joint,
            // a cell beyond the details panel on each side.
            let available = usize::from(width + 3).saturating_sub(6 + usize::from(reserved));
            if available >= 1 {
                let title = "Task log example";
                let expected = if title.len() > available {
                    format!("{}…", &title[..available - 1])
                } else {
                    title.to_owned()
                };
                let expected = format!(" {expected} ");
                let start = (usize::from(width) + 3 - expected.chars().count()) / 2 - 2;
                let chars: Vec<_> = top_text.chars().collect();
                assert_eq!(
                    chars[start..start + expected.chars().count()]
                        .iter()
                        .collect::<String>(),
                    expected,
                    "width {width}"
                );
            }
        }
    }

    #[test]
    fn tight_bottom_border_keeps_centered_status() {
        let (mut view, now) = log_fixture();
        view.rows[view.selected].started_at = Some(now - Duration::seconds(11520));
        let area = Rect::new(0, 0, 28, 25);
        let layout = view.center_layout(area);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let bottom = Rect::new(
            layout.details.x,
            view.viewport.borrow().details.bottom(),
            layout.details.width,
            1,
        );
        assert!(label(&buf, bottom).contains("Working"));
    }
}

#[test]
fn details_prompt_shows_queued_entries_in_order() {
    let mut row = session("queue", "Queue task", SessionStatus::Working, 0);
    row.prompt = "Current prompt".into();
    row.queued = vec!["First queued prompt".into(), "Second queued prompt".into()];
    assert_eq!(
        queued_details(row, 15),
        include_str!("../../tests/ui_snapshots/details_queued.txt")
    );
}

#[test]
fn details_prompt_renders_all_wrapped_lines() {
    let mut row = session("prompt", "Prompt task", SessionStatus::Working, 0);
    row.prompt = format!("{} {} {}", "a".repeat(40), "b".repeat(40), "c".repeat(40));
    let rendered = queued_details(row, 15);
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(lines[1], "a".repeat(40));
    assert_eq!(lines[2], "b".repeat(40));
    assert_eq!(lines[3], "c".repeat(40));
    assert_eq!(lines.len(), 4);
    assert!(!rendered.contains('…'));
}

#[test]
fn details_prompt_overflow_marks_panel_and_scrolls_to_all_lines() {
    let mut row = session("prompt", "Prompt task", SessionStatus::Working, 0);
    let wrapped = ('a'..='h')
        .map(|letter| letter.to_string().repeat(40))
        .collect::<Vec<_>>();
    row.prompt = wrapped.join(" ");
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, 6);
    let now = Utc::now();
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, now);
    let line = |buf: &Buffer, y: u16| {
        (0..area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    };
    assert_eq!(line(&buf, 1), wrapped[0]);
    assert_eq!(line(&buf, 2), wrapped[1]);
    assert!(line(&buf, 5).ends_with('…'));
    let hidden = view.viewport.borrow().details_scroll.hidden[0];
    assert!(hidden > 0);
    for _ in 0..hidden {
        view.scroll_content_row(true, false, area.y + 1);
        buf.reset();
        view.render_details(area, &mut buf, now);
    }
    assert_eq!(line(&buf, 5), wrapped.last().unwrap().as_str());
    assert!(!buf.content.iter().any(|cell| cell.symbol() == "…"));
}

#[test]
fn details_prompt_truncates_long_queued_entry() {
    let mut row = session("queue", "Queue task", SessionStatus::Working, 0);
    row.prompt = "Current prompt ".repeat(12);
    row.queued = vec!["Queued prompt that extends beyond the panel width".into()];
    assert_eq!(
        queued_details(row, 15),
        include_str!("../../tests/ui_snapshots/details_queued_truncated.txt")
    );
}

fn queued_details(row: Session, height: u16) -> String {
    details_at(row, height, Utc::now())
}

fn details_at(row: Session, height: u16, now: chrono::DateTime<Utc>) -> String {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, height);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, now);
    buf.content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end_matches('\n')
        .to_owned()
}

#[test]
fn details_sections_run_prompt_tasks_background_update_activity() {
    let mut row = session("order", "Order task", SessionStatus::Ready, 120);
    row.activity = Some(crate::model::Activity {
        kind: crate::log::Kind::Tool,
        label: "Run checks".into(),
        at: Utc::now(),
    });
    row.last_message = Some("Checking the project".into());
    row.background = vec![crate::model::BackgroundTask {
        description: "Waiting for output".into(),
        output: std::path::PathBuf::from("/nonexistent/output"),
        started_at: Utc::now(),
    }];
    row.tasks = vec![crate::model::SessionTask {
        id: "1".into(),
        subject: "Move the prompt".into(),
        status: crate::model::TaskStatus::InProgress,
    }];
    let rendered = queued_details(row, 40);
    let positions = [
        "Prompt\n",
        "Tasks",
        "Background\n",
        "Update",
        "Last activity",
    ]
    .map(|label| rendered.find(label).expect(label));
    assert!(positions.is_sorted(), "{rendered}");
}

#[test]
fn details_background_boxes_tail_and_clip_output() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "discard\nfirst\nlast\n{}", "x".repeat(80)).unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.last_message = Some("Checking the project".into());
    row.background = vec![
        crate::model::BackgroundTask {
            description: "Waiting for output".into(),
            output: output.path().with_extension("missing"),
            started_at: Utc::now(),
        },
        crate::model::BackgroundTask {
            description: "Run checks".into(),
            output: output.path().into(),
            started_at: Utc::now(),
        },
    ];
    let rendered = queued_details(row.clone(), 22);
    assert!(rendered.find("Prompt\n").unwrap() < rendered.find("Background\n").unwrap());
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_background.txt")
    );
    row.background[1].description = "A very long description ".repeat(5);
    assert_eq!(
        queued_details(row.clone(), 22),
        include_str!("../../tests/ui_snapshots/details_background_long_title.txt")
    );
    let clipped = queued_details(row, 9);
    assert!(
        clipped
            .lines()
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n")
            .starts_with(
                "Prompt\nBackground task\n\nBackground\n…\n\nUpdate\nChecking the project"
            ),
        "{clipped}"
    );
    assert!(clipped.contains("Update"));
    assert!(!clipped.contains("Project"));
    assert!(!clipped.contains("Unknown"));
    assert!(!clipped.contains("xxxxxxxx"));
}

#[test]
fn details_background_includes_only_working_subagents() {
    let now = Utc::now();
    let mut parent = session("parent", "Parent", SessionStatus::Ready, 0);
    let output = tempfile::NamedTempFile::new().unwrap();
    parent.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: output.path().into(),
        started_at: now - Duration::seconds(10),
    });
    let mut worker = session("agent-worker", "Check files", SessionStatus::Working, 0);
    worker.parent = Some(parent.id.clone());
    worker.started_at = Some(now - Duration::seconds(65));
    worker.activity = Some(crate::model::Activity {
        kind: crate::log::Kind::Tool,
        label: "Read files\nextra line".into(),
        at: now,
    });
    let mut idle = session("agent-idle", "Review files", SessionStatus::Ready, 0);
    idle.parent = Some(parent.id.clone());
    idle.started_at = Some(now - Duration::seconds(125));
    idle.last_message = Some("Review complete\nextra line".into());
    // Ready teammates can retain activity from their previous turn.
    idle.activity = worker.activity.clone();
    let mut other = worker.clone();
    other.id = session_key("agent-other");
    other.parent = Some(session_key("other"));
    other.title = "Unrelated agent".into();
    let mut nested = worker.clone();
    nested.id = session_key("agent-nested");
    nested.parent = Some(worker.id.clone());
    nested.title = "Nested agent".into();
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![parent, worker, idle, other, nested], false);
    view.selected = view
        .rows
        .iter()
        .position(|s| s.id == session_key("parent"))
        .unwrap();
    assert!(view.has_background());
    let area = Rect::new(0, 0, 40, 24);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, now);
    let rendered = buf
        .content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        rendered.trim_end_matches('\n'),
        include_str!("../../tests/ui_snapshots/details_subagents.txt")
    );
    let (x, y) = rendered_cell(&rendered, "Read files");
    assert_eq!(
        buf[(x, y)].fg,
        style::log_styles(&crate::log::Kind::Tool).1.fg.unwrap()
    );
    view.selected = view
        .rows
        .iter()
        .position(|s| s.id == session_key("agent-worker"))
        .unwrap();
    assert!(view.has_background());
    buf.reset();
    view.render_details(area, &mut buf, now);
    assert!(buf.content.chunks(40).any(|row| {
        row.iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("Background")
    }));
    view.rows[view.selected]
        .background
        .push(crate::model::BackgroundTask {
            description: "Worker command".into(),
            output: output.path().into(),
            started_at: now,
        });
    assert!(view.has_background());
    buf.reset();
    view.render_details(area, &mut buf, now);
    let text = buf
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("──────────── Worker command ───────── 0s"));
    assert!(text.contains("Nested agent"));
    assert!(!text.contains("Review files"));
}

#[test]
fn background_box_dims_title_rule_but_not_output() {
    use ratatui::style::Modifier;
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: output.path().into(),
        started_at: Utc::now(),
    });
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, 20);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    let top = (0..20)
        .find(|y| buf[(0, *y)].symbol() == "─")
        .expect("box top border");
    assert!(buf[(0, top)].modifier.contains(Modifier::DIM));
    assert!(buf[(30, top)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(15, top)].symbol(), "R");
    assert!(buf[(15, top)].modifier.contains(Modifier::DIM));
    let (tool_view, _) = open_tool_fixture();
    let mut event = tool_view.log[7].clone();
    event.calls[0].title = "Run checks".into();
    event.calls[0].detail_title = "Run checks".into();
    let detail = super::tool_detail::detail(&event, String::new(), Style::new().dim());
    let mut title_buf = Buffer::empty(Rect::new(0, 0, 40, 1));
    let columns = super::detail_model::detail_columns(title_buf.area, title_buf.area);
    (&detail.sections[0].title.line(columns).0).render(title_buf.area, &mut title_buf);
    let title_x = (0..40)
        .find(|x| title_buf[(*x, 0)].symbol() == "R")
        .unwrap();
    assert_eq!(title_buf[(0, 0)].modifier, buf[(0, top)].modifier);
    assert_eq!(
        title_buf[(title_x, 0)].fg,
        detail.sections[0].title.title_style.fg.unwrap()
    );
    assert_eq!(buf[(0, top + 1)].symbol(), "t");
    assert!(!buf[(0, top + 1)].modifier.contains(Modifier::DIM));
}

#[test]
fn background_task_whose_output_ended_is_hidden() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick\n\n[exited with code 0]").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: output.path().into(),
        started_at: Utc::now(),
    });
    let rendered = queued_details(row, 19);
    assert!(!rendered.contains("\nBackground\n"));
    assert!(!rendered.contains("Run checks"));
}

#[test]
fn background_redraw_follows_selection() {
    let mut first = session("first", "First", SessionStatus::Ready, 0);
    first.background.push(crate::model::BackgroundTask {
        description: "Running".into(),
        output: "/tmp/unused.output".into(),
        started_at: Utc::now(),
    });
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![
            first,
            session("second", "Second", SessionStatus::Ready, 120),
        ],
        false,
    );
    key(&mut view, KeyCode::Home);
    assert!(view.has_background());
    key(&mut view, KeyCode::Down);
    assert!(!view.has_background());
    view.set_sessions(Vec::new(), false);
    assert!(!view.has_background());
}

#[test]
fn details_markdown_snapshot() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let mut row = session("markdown", "Markdown task", SessionStatus::Working, 0);
    row.prompt = "Use `cargo` and **tests**".into();
    row.queued = vec!["- *queued* prompt".into()];
    row.last_message =
        Some("## Result\n\nBuilt [agents](https://example.com).\n\n```\ncargo test\n```".into());
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 21)).unwrap();
    terminal
        .draw(|frame| view.render_details(frame.area(), frame.buffer_mut(), Utc::now()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = buffer
        .content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end_matches('\n')
        .to_owned();
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_markdown.txt")
    );
    assert_eq!(buffer[(4, 1)].fg, crate::theme::inline_code_color());
    assert!(
        buffer[(14, 1)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(
        buffer[(2, 2)]
            .modifier
            .contains(ratatui::style::Modifier::ITALIC)
    );
    assert_eq!(buffer[(6, 7)].fg, Color::Yellow);
    assert_eq!(buffer[(0, 9)].fg, Color::Cyan);
}

#[test]
fn compaction_summary_renders_compaction_tag_and_keeps_visible_body() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-compaction.jsonl");
    let events = crate::providers::claude::log::tail(&path, 10).unwrap();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(super::log::kind_label(&event.kind), "Compaction");
    assert_eq!(super::log::copied_label(event), "Copied compaction message");
    let (mut view, now) = log_fixture();
    view.log = events.clone();
    view.select_fixture_event(Some(event.clone()));
    let rendered = log_screen(&view, 120, 60, now);
    assert!(rendered.contains("Compaction"), "{rendered}");
    assert!(
        rendered.matches("This session is being continued").count() >= 2,
        "summary must remain visible in both list and detail: {rendered}"
    );
    assert!(!rendered.contains("User"), "{rendered}");
}

fn log_fixture() -> (View, chrono::DateTime<Utc>) {
    use crate::log::{Event, Kind};
    use chrono::TimeZone;
    let now = chrono::Local
        .with_ymd_and_hms(2026, 10, 12, 12, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let mut row = session("log", "Task log example", SessionStatus::Working, 0);
    row.updated_at = now;
    row.transcript = Some("/tmp/task-log.jsonl".into());
    row.last_record_at = Some(now - Duration::seconds(10));
    row.process = Some(crate::model::Process {
        pid: 1,
        label: None,
        started_at: None,
    });
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    view.log_session = Some(session_key("log"));
    view.log = [
        (Kind::User, "Please\n  inspect the task", 120, None),
        (Kind::Assistant, "I will check the files", 110, None),
        (Kind::Tool, "Read src/main.rs，Read src/app.rs", 100, Some(90)),
        (Kind::Error, "failed check", 80, Some(70)),
        (Kind::QueuedUser, "Also inspect the tests", 60, None),
        (Kind::Tool, "run a background job *", 50, None),
        (Kind::Assistant, "A long response whose whitespace collapses and whose body must be truncated before the right aligned timestamp", 40, None),
        (Kind::Tool, "run tests", 30, None),
    ].into_iter().map(|(kind, body, age, end)| Event {
        source: None,
        completed_time: false,
        background: false,
        source_records: Vec::new(),
        sender: None,
        kind, body: body.into(), paths: Vec::new(), started_at: now - Duration::seconds(age),
        ended_at: end.map(|age| now - Duration::seconds(age)),
        duration: None,
        previous_end: Some(now - Duration::seconds(age + 5)),
        images: Vec::new(),
        calls: Vec::new(),
    }).collect();
    let mut previous_end = None;
    for event in &mut view.log {
        if event.kind == Kind::Assistant {
            event.previous_end = previous_end;
        }
        previous_end = if matches!(event.kind, crate::log::Kind::Tool | crate::log::Kind::Error) {
            event.ended_at
        } else {
            Some(event.started_at)
        };
    }
    view.set_log(view.log.clone());
    (view, now)
}

fn log_screen(view: &View, width: u16, height: u16, now: chrono::DateTime<Utc>) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    buf.content
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn log_list_screen(view: &View, width: u16, height: u16, now: chrono::DateTime<Utc>) -> String {
    let rendered = log_screen(view, width, height, now);
    let detail = view
        .center_layout(Rect::new(0, 0, width, height))
        .log_detail;
    rendered
        .lines()
        .enumerate()
        .map(|(y, line)| {
            if (detail.y + 1..detail.bottom()).contains(&(y as u16)) {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn column_padding_fixture() -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = log_fixture();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    view.set_log(crate::providers::claude::log::tail_session(&path, 200, true).unwrap());
    let row = &mut view.rows[0];
    row.last_message = Some("A message that wraps across the details column. ".repeat(4));
    row.last_message_at = Some(now - Duration::minutes(2));
    row.prompt = "Inspect the files".into();
    row.prompt_at = Some(now - Duration::seconds(45));
    row.turn_messages = 3;
    row.git_branch = Some("agents-panel".into());
    row.pr = Some(crate::model::PullRequest {
        number: 13,
        url: "https://github.com/blockchainian/claude/pull/13".into(),
    });
    row.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: path,
        started_at: now - Duration::seconds(25),
    });
    (view, now)
}

fn assert_task_metadata(width: u16, show_status: bool, show_updated: bool) {
    let (mut view, now) = column_padding_fixture();
    view.rows[0].title = "A task title that is deliberately longer than the column".into();
    view.rows[0].updated_at = now - Duration::minutes(2);
    let area = Rect::new(0, 0, width, 60);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let list = view.center_layout(area).list;
    let y = view.viewport.borrow().marker.unwrap().1;
    let text: String = (list.x..list.right())
        .map(|x| buf[(x, y)].symbol())
        .collect();
    let status = View::status(&view.rows[0]).0;
    assert_eq!(
        text.contains(status),
        show_status,
        "status at {width}, list width {}: {text}",
        list.width
    );
    assert_eq!(
        text.contains("2m ago"),
        show_updated,
        "updated at {width}, list width {}: {text}",
        list.width
    );
    println!("{width} cols: list={}, {text}", list.width);
}

#[test]
fn task_rows_start_at_left_padding_without_marker_and_click_correctly() {
    for width in [100, 150, 200] {
        super::style::with_selection_style(
            super::style::selection_style_for(true, false, false),
            || {
                let mut view = fixture();
                let area = Rect::new(0, 0, width, 40);
                let mut buf = Buffer::empty(area);
                view.render(area, &mut buf);
                let list = view.center_layout(area).list;
                let second = view.selectable_indices()[1];
                let title = view.rows[second]
                    .display_title()
                    .chars()
                    .take(12)
                    .collect::<String>();
                let rendered = screen(&view, width, 40);
                let (_, y) = rendered_cell(&rendered, &title);
                click_mouse(
                    &mut view,
                    mouse(MouseEventKind::Down(MouseButton::Left), list.x, y),
                );
                assert_eq!(view.selected, second);
                view.render(area, &mut buf);
                assert_eq!(view.marker_pos(), Some((list.x, y)));
                assert_eq!(
                    buf[(list.x, y)].symbol(),
                    View::status(&view.rows[second]).1.content
                );
                assert_eq!(buf[(list.x + 2, y)].symbol(), &title[..1]);
                for x in list.x..list.right() {
                    assert_ne!(buf[(x, y)].symbol(), "❯");
                    assert!(
                        buf[(x, y)]
                            .modifier
                            .contains(ratatui::style::Modifier::REVERSED)
                    );
                }
                assert_eq!(buf[(list.x - 1, y)].symbol(), " ");
            },
        );
    }
}

#[test]
fn task_metadata_narrow_100_columns() {
    assert_task_metadata(100, true, true);
}

#[test]
fn task_metadata_wide_150_columns() {
    assert_task_metadata(150, true, false);
}

#[test]
fn task_metadata_wide_200_columns() {
    assert_task_metadata(200, true, true);
}

#[test]
fn task_metadata_drops_rightmost_columns_at_title_limit() {
    for (width, title_width, show_status, show_updated) in [
        (32, 30, false, false),
        (34, 32, false, false),
        (35, 20, true, false),
        (45, 30, true, false),
        (46, 20, true, true),
        (47, 21, true, true),
    ] {
        let (mut view, now) = column_padding_fixture();
        view.rows[0].title = "T".repeat(80);
        view.rows[0].updated_at = now - Duration::minutes(2);
        let area = Rect::new(1, 1, width, 4);
        let mut buf = Buffer::empty(Rect::new(0, 0, width + 2, 6));
        view.render_center_rows(area, &mut buf, now);
        let y = view.viewport.borrow().marker.unwrap().1;
        let text: String = (area.x..area.right())
            .map(|x| buf[(x, y)].symbol())
            .collect();
        assert_eq!(
            text.contains(View::status(&view.rows[0]).0),
            show_status,
            "status at {width}: {text}"
        );
        assert_eq!(
            text.contains("2m ago"),
            show_updated,
            "updated at {width}: {text}"
        );
        assert_eq!(
            buf[(area.x + 2 + title_width - 1, y)].symbol(),
            "…",
            "title at {width}: {text}"
        );
    }
}

#[test]
fn list_row_highlights_include_column_padding() {
    for width in [100, 149, 150, 200] {
        for kind in ["task", "more", "picker"] {
            let (mut view, now) = column_padding_fixture();
            match kind {
                "more" => {
                    view.status_filter = TASK_FILTERS
                        .iter()
                        .position(|(_, statuses)| *statuses == [SessionStatus::Inactive])
                        .unwrap();
                    view.has_more = true;
                    view.selected = usize::MAX;
                }
                "picker" => {
                    view.selector = Some(super::draft_controls::Selector::new(
                        super::draft_controls::SelectorKind::Model {
                            target: super::draft_controls::ControlTarget::Session(
                                view.rows[0].id.clone(),
                            ),
                            values: vec!["claude-sonnet-5-5".into(), "claude-haiku-5-5".into()],
                        },
                        0,
                    ));
                }
                _ => {}
            }
            let area = Rect::new(0, 0, width, 60);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let layout = view.center_layout(area);
            let viewport = view.viewport.borrow();
            let (content, y) = if kind == "picker" {
                let row = viewport.picker_rows[0].0;
                (layout.details, row.y)
            } else {
                (layout.list, viewport.marker.expect("selected row").1)
            };
            let marker_x = content.x;
            if kind == "picker" {
                assert_eq!(buf[(marker_x, y)].symbol(), "c");
            } else {
                assert!(!(content.x..content.right()).any(|x| buf[(x, y)].symbol() == "❯"));
            }
            if kind == "task" {
                assert_eq!(buf[(marker_x, y)].symbol(), "●");
                assert_eq!(
                    buf[(marker_x + 2, y)].symbol(),
                    &view.rows[0].display_title()[..1]
                );
            }
            let bg = super::style::selection_style().bg.unwrap();
            for x in content.x - 1..=content.right() {
                assert_eq!(
                    buf[(x, y)].bg,
                    bg,
                    "{kind} padding at ({x}, {y}), width {width}"
                );
            }
            for x in [content.x - 1, content.right()] {
                assert_eq!(buf[(x, y)].symbol(), " ");
            }
            for x in 0..width {
                if buf[(x, y)].symbol() == "│" {
                    assert_eq!(buf[(x, y)].bg, Color::Reset, "separator at {width}");
                }
            }
        }
    }
}

#[test]
fn dashboard_task_rows_use_the_header_row_and_keep_click_offsets() {
    for width in [100, 200] {
        let (mut view, now) = column_padding_fixture();
        let template = view.rows[0].clone();
        view.set_sessions(
            (0..40)
                .map(|index| {
                    let mut row = template.clone();
                    row.id.id = format!("task-{index}");
                    row.updated_at = now - Duration::seconds(index);
                    row
                })
                .collect(),
            false,
        );
        let area = Rect::new(0, 0, width, 60);
        key(&mut view, KeyCode::Home);
        for end in [false, true] {
            if end {
                key(&mut view, KeyCode::End);
            }
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let list = view.center_layout(area).list;
            let viewport = view.viewport.borrow();
            assert_eq!(viewport.list_rows.y, list.y);
            assert_eq!(viewport.list_rows.height, list.height - 1);
            assert_eq!(viewport.page_height, usize::from(list.height - 1));
            let entries = view.center_rows(&view.selectable_indices(), view.grouping);
            let offset = usize::from(!end);
            let super::navigation::CenterRow::Task(index) = entries[viewport.first_entry + offset]
            else {
                panic!("expected task row");
            };
            let expected = view.rows[index].id.clone();
            let y = list.y + offset as u16;
            assert_ne!(buf[(list.x, list.y)].symbol(), "↑");
            drop(viewport);
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), list.x + 4, y),
            );
            assert_eq!(view.selected_id(), Some(&expected));
        }
    }
}

#[test]
fn dashboard_log_bodies_center_on_the_panel() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = log_fixture();
        for event in view.log.clone() {
            view.set_log(vec![event.clone()]);
            let area = Rect::new(0, 0, width, 60);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let log = view.viewport.borrow().log;
            let y = log.y;
            let body =
                super::log::body_line(&event, usize::from(log.width - 10), &view.rows[0].cwd)
                    .to_string();
            for (offset, ch) in body.chars().take(12).enumerate() {
                let x = super::centering::centered_x(
                    log,
                    super::log::log_row_areas(log).0,
                    body.width() as u16,
                );
                let cell = &buf[(x + offset as u16, y)];
                assert_eq!(
                    cell.symbol(),
                    ch.to_string(),
                    "kind {:?} at {width}",
                    event.kind
                );
                assert_eq!(
                    cell.fg,
                    super::style::log_styles(&event.kind)
                        .1
                        .fg
                        .unwrap_or(Color::Reset)
                );
            }
            if width >= 150 && event.kind == crate::log::Kind::Assistant {
                let detail = view.center_layout(area).log_detail;
                let header: String = (detail.x..detail.right())
                    .map(|x| buf[(x, detail.y + 1)].symbol())
                    .collect();
                assert_eq!(header.trim().split("  ").next(), Some("Assistant"));
            }
        }
    }
}

#[test]
fn dashboard_log_list_fills_rows_next_to_rules() {
    for width in [100, 149, 150, 200] {
        for search in [false, true] {
            let (mut view, now) = column_padding_fixture();
            view.rows[0].model = Some("opus".into());
            let event = view
                .log
                .iter()
                .find(|event| event.kind == crate::log::Kind::Tool)
                .unwrap()
                .clone();
            view.log_search.editing = search;
            let body_event = event.clone();
            view.set_log(vec![event; 100]);
            let area = Rect::new(0, 0, width, 60);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let layout = view.center_layout(area);
            let log = view.viewport.borrow().log;
            let body =
                super::log::body_line(&body_event, usize::from(log.width - 10), &view.rows[0].cwd);
            let x = super::centering::centered_x(
                log,
                super::log::log_row_areas(log).0,
                body.width() as u16,
            );
            let first_symbol = body.to_string().chars().next().unwrap().to_string();
            let first = log.y;
            let last = log.bottom() - 1 - u16::from(search);
            assert_eq!(
                buf[(x, first)].symbol(),
                first_symbol,
                "first row at {width}, search={search}"
            );
            assert_eq!(
                buf[(x, last)].symbol(),
                first_symbol,
                "last row at {width}, search={search}"
            );
            assert_eq!(
                view.viewport.borrow().log_rows.len(),
                usize::from(last - first + 1)
            );
            assert_eq!(view.log_event_count(area), usize::from(last - first + 1));
            if width >= 150 {
                let rule_y = layout.prompt.y - 1;
                assert_eq!(layout.list.bottom(), rule_y);
                assert_eq!(layout.details.bottom(), rule_y);
                assert_eq!(log.bottom(), rule_y);
                assert_eq!(buf[(log.x - 2, rule_y)].symbol(), "┴");
                assert_eq!(buf[(log.x - 1, rule_y)].symbol(), "╴");
                assert_ne!(buf[(log.x, rule_y)].symbol(), " ");
            }
        }
    }
}

#[test]
fn dashboard_log_detail_starts_directly_below_rule() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = column_padding_fixture();
        view.select_fixture_event(Some(view.log.last().unwrap().clone()));
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        let detail = layout.log_detail;
        assert!(
            (detail.x..detail.right()).any(|x| buf[(x, detail.y + 1)].symbol() != " "),
            "detail header at {width}"
        );
    }
}

#[test]
fn dashboard_screen_padding_is_one_cell() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = column_padding_fixture();
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.input_box.focused = true;
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        assert_eq!(layout.list.x, 1);
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_eq!(buf[(2, 0)].symbol(), "A");
        assert_eq!(view.viewport.borrow().tab_bar.x, 1);
        for x in 1..width - 1 {
            if x < layout.details.x || x >= layout.details.right() {
                let joint = x == layout.gap.x + 1 || (width >= 150 && x == layout.log.x - 2);
                assert_eq!(buf[(x, 1)].symbol(), if joint { "┬" } else { "─" });
            }
        }
        for y in 0..area.height {
            // Rules run edge to edge; everything else keeps the padding cell.
            let rule = buf[(0, y)].symbol() == "─";
            assert_eq!(buf[(0, y)].symbol() == "─", rule, "row {y}");
            assert_eq!(buf[(width - 1, y)].symbol() == "─", rule, "row {y}");
            if !rule {
                assert_eq!(buf[(0, y)].symbol(), " ", "row {y}");
                assert_eq!(buf[(width - 1, y)].symbol(), " ", "row {y}");
            }
        }
        let prompt_y = layout.prompt.y;
        assert_ne!(buf[(1, prompt_y)].symbol(), " ");
        assert_eq!(
            view.cursor_pos(area).unwrap().0,
            4 + view.input_label().width() as u16
        );
        assert_ne!(buf[(1, layout.footer.y)].symbol(), " ");
        println!("{width} cols: screen/header/tabs/rules/input/footer padding=1/1");
    }
}

#[test]
fn dashboard_column_padding_matches_separators_in_buffer() {
    for width in [100, 149, 150, 200, 240] {
        let (view, now) = column_padding_fixture();
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let text = |y, start, end| {
            (start..end)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        };
        let separators: Vec<_> = (0..width)
            .filter(|&x| buf[(x, 2)].symbol() == "│")
            .collect();
        let left = separators[0];
        let right = separators.get(1).copied().unwrap_or(width);
        let updated = text(3, 0, left);
        if view.center_layout(area).list.width >= 4 + 20 + 24 {
            assert_eq!(updated.trim_end().chars().count() as u16, left - 1);
        } else {
            assert!(!updated.contains("ago"));
        }
        for label in ["Background", "Update", "Prompt"] {
            let y = (2..35)
                .find(|&y| text(y, left + 1, right).contains(label))
                .unwrap_or_else(|| panic!("missing {label} at width {width}"));
            let cells = text(y, left + 1, right);
            assert_eq!(
                cells.find(label),
                Some(1 + (usize::from(right - left - 3) - label.len()) / 2),
                "{label} center at {width}"
            );
            if label == "Update" || label == "Prompt" {
                assert_eq!(
                    (left + 1..right).rfind(|&x| buf[(x, y)].symbol() != " "),
                    Some(right - 2),
                    "{label} right at {width}"
                );
            }
        }
        let y = (2..35)
            .find(|&y| text(y, left + 1, right).contains("Run checks"))
            .unwrap();
        assert!(text(y, left + 1, right).trim_end().ends_with("25s"));
        assert_eq!(
            text(y, left + 1, right).trim_end().chars().count() as u16,
            right - left - 2,
            "background time at {width}"
        );
        // Wrapped bodies, selections and links stay inside the same content bounds.
        for y in 3..view.center_layout(area).details.bottom() {
            for x in [left + 1, right - 1] {
                assert_eq!(buf[(x, y)].symbol(), " ", "padding ({x}, {y}) at {width}");
            }
        }
        let viewport = view.viewport.borrow();
        if matches!(width, 100 | 200) {
            println!(
                "{width} cols: separators={separators:?}, task Updated end={}, details={}..{}, log={}..{}",
                updated.trim_end().chars().count(),
                viewport.details_text.area.x,
                viewport.details_text.area.right(),
                viewport.log.x,
                viewport.log.right()
            );
        }
        assert_eq!(viewport.details_text.area.x, left + 2);
        assert_eq!(viewport.details_text.area.right(), right - 1);
    }
}

#[test]
fn dashboard_log_padding_and_edge_clicks_use_rendered_rows() {
    for width in [100, 149, 150, 200] {
        for edge in [false, true] {
            let (mut view, now) = column_padding_fixture();
            let area = Rect::new(0, 0, width, 60);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let wide = width >= 150;
            let separators: Vec<_> = (0..width)
                .filter(|&x| buf[(x, 2)].symbol() == "│")
                .collect();
            let left = if wide { separators[1] + 2 } else { 1 };
            let y = view.viewport.borrow().log.y
                + view
                    .viewport
                    .borrow()
                    .log_rows
                    .iter()
                    .position(|&index| index == view.log.len() - 2)
                    .unwrap() as u16;
            let row: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
            let log: String = (left..width).map(|x| buf[(x, y)].symbol()).collect();
            assert!(
                log.trim_end().chars().count() as u16 + left <= width - 11,
                "log body right at {width}: {row}"
            );
            assert!((width - 11..width - 1).all(|x| buf[(x, y)].symbol() == " "));
            assert!(!log.trim().is_empty(), "log body at {width}");
            if wide {
                assert_eq!(buf[(left - 1, y)].symbol(), " ");
            }
            let selected = {
                let viewport = view.viewport.borrow();
                let index = viewport.log_rows[usize::from(y - viewport.log.y)];
                assert_eq!(viewport.log_list_text.area.x, left);
                assert_eq!(viewport.log_list_text.area.right(), width - 1);
                view.log[index].clone()
            };
            let x = if edge { width - 2 } else { left };
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, y),
            );
            view.flush_pending_click(Instant::now() + view.double_click_interval);
            assert_eq!(
                view.visible_log_event().cloned(),
                Some(selected),
                "edge click at {width}"
            );
            if wide {
                let detail = view.viewport.borrow().log_text.area;
                let columns = super::detail_model::detail_columns(
                    view.viewport.borrow().log_detail,
                    view.center_layout(buf.area).log,
                );
                assert_eq!(detail.x, columns.text.x);
                assert_eq!(detail.right(), columns.text.right());
            }
        }
    }
}

#[test]
fn dashboard_log_search_uses_the_row_padding() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = column_padding_fixture();
        view.log_search.editing = true;
        view.log_search.query.set_text("Tool");
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let left = if width >= 150 {
            2 * ((width - 8) / 4) + 7
        } else {
            1
        };
        let y = view.viewport.borrow().log.bottom() - 1;
        let input: String = (left..left + 13).map(|x| buf[(x, y)].symbol()).collect();
        assert_eq!(input, "Search ❯ Tool");
        assert_eq!(view.cursor_pos(area), Some((left + 13, y)));
        view.log_search.editing = false;
        view.render_at(area, &mut buf, now);
        let query: String = (width - 5..width - 1)
            .map(|x| buf[(x, y)].symbol())
            .collect();
        assert_eq!(query, "Tool");
        assert_eq!(buf[(width - 1, y)].symbol(), " ");
    }
}

#[test]
fn task_log_time_column_and_ellipses_align_in_both_layouts() {
    use crate::log::Kind;
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let source = crate::providers::claude::log::tail_session(&path, 200, true)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == Kind::Tool)
        .unwrap();
    for width in [100, 149, 150, 200] {
        let (mut view, now) = log_fixture();
        view.log = [2, 590, 0]
            .into_iter()
            .map(|seconds| {
                let mut event = source.clone();
                event.body = "long body ".repeat(40);
                event.calls.clear();
                event.paths.clear();
                event.started_at = now - Duration::seconds(10);
                event.ended_at = Some(event.started_at);
                event.previous_end =
                    (seconds > 0).then_some(event.started_at - Duration::seconds(seconds));
                event
            })
            .collect();
        let area = Rect::new(0, 0, width, 40);
        let mut buffer = Buffer::empty(area);
        view.render_at(area, &mut buffer, now);
        let log = view.viewport.borrow().log;
        let top = log.y;
        for (offset, time) in ["2s", "9m 50s", "11:59:50"].into_iter().enumerate() {
            let y = top + offset as u16;
            let text = |start, end| {
                (start..end)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            };
            assert_eq!(text(log.x, log.x + 10), "long body ", "width {width}");
            assert_eq!(text(log.right() - 8, log.right()), format!("{time:>8}"));
            assert_eq!(text(log.right() - 10, log.right() - 8), "  ");
            assert_eq!(buffer[(log.right() - 11, y)].symbol(), "…");
            assert_eq!(buffer[(log.x, y)].symbol(), "l");
        }
        view.scroll_content_row(true, true, top);
        view.render_at(area, &mut buffer, now);
        assert!(view.viewport.borrow().log_jump.is_empty());
    }
}

#[test]
fn task_log_body_centers_on_its_panel() {
    let (mut view, now) = log_fixture();
    view.log.pop();
    let rendered = log_screen(&view, 100, 32, now);
    assert!(
        rendered
            .lines()
            .nth(view.viewport.borrow().log.y as usize)
            .unwrap()
            .starts_with(&format!("{}Please", " ".repeat(39)))
    );
}

#[test]
fn task_log_denied_snapshot() {
    use crate::log::Kind;
    use crate::providers::claude::log::tail;
    let (mut view, now) = log_fixture();
    view.log.truncate(1);
    for (fixture, age, end) in [
        ("automode_blocked.jsonl", 110, 105),
        ("safety_check.jsonl", 100, 95),
        ("automode_blocked_dangerous.jsonl", 90, 85),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let mut event = tail(&path, 1).unwrap().remove(0);
        event.started_at = now - Duration::seconds(age);
        event.ended_at = Some(now - Duration::seconds(end));
        event.previous_end = view
            .log
            .last()
            .map(|previous| previous.ended_at.unwrap_or(previous.started_at));
        assert_eq!(
            super::style::log_styles(&event.kind),
            super::style::log_styles(&Kind::Error)
        );
        view.log.push(event);
    }
    view.set_log(view.log.clone());
    view.select_log_event(None);
    assert_eq!(
        log_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_log_denied.txt")
    );
}

#[test]
fn codex_inner_task_log_snapshot() {
    use crate::providers::Provider;
    let (mut view, now) = log_fixture();
    view.rows[0].id.provider = crate::providers::ProviderId::Codex;
    view.log_session = Some(view.rows[0].id.clone());
    let dir = tempfile::tempdir().unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        dir.path().to_owned(),
        dir.path().join("manager"),
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-inner");
    view.log.clear();
    for (index, fixture) in [
        "command.jsonl",
        "file-change.jsonl",
        "mcp.jsonl",
        "extension.jsonl",
        "image-view.jsonl",
    ]
    .into_iter()
    .enumerate()
    {
        view.rows[0].transcript = Some(root.join(fixture));
        let events: Vec<_> = provider
            .log(&view.rows[0], usize::MAX, false)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.kind, crate::log::Kind::Tool | crate::log::Kind::Error))
            .collect();
        assert_eq!(events.len(), 1, "{fixture}: {events:?}");
        let mut event = events.into_iter().next().unwrap();
        let duration = event.ended_at.unwrap() - event.started_at;
        event.started_at = now - Duration::seconds(100 - index as i64 * 15);
        event.ended_at = Some(event.started_at + duration);
        event.previous_end = view.log.last().map(|previous| previous.ended_at.unwrap());
        view.log.push(event);
    }
    view.set_log(view.log.clone());
    view.select_log_event(None);
    assert_eq!(
        log_screen(&view, 160, 25, now),
        include_str!("../../tests/ui_snapshots/codex_inner_log.txt")
    );
    view.prepare_log_search();
    view.log_search.query.set_text("ImageView");
    view.rematch_log();
    assert_eq!(view.matching_log_count(), 1);
    assert!(view.log_matches(4));
    view.log_search.query.set_text("exec_command");
    view.rematch_log();
    assert_eq!(view.matching_log_count(), 1);
    assert!(view.log_matches(0));
}

#[test]
fn task_log_tall_running_snapshot() {
    let (view, now) = log_fixture();
    assert_eq!(
        log_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_log_running.txt")
    );
}

#[test]
fn task_log_tall_finished_snapshot() {
    let (mut view, now) = log_fixture();
    view.log.last_mut().unwrap().ended_at = Some(now - Duration::seconds(5));
    view.set_log(view.log.clone());
    assert_eq!(
        log_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_log_finished.txt")
    );
}

#[test]
fn task_log_short_terminal_splits_content_equally() {
    let mut view = fixture();
    view.rows[view.selected].transcript = Some("/tmp/task-log.jsonl".into());
    let (log, _) = log_fixture();
    view.set_log(log.log);
    view.log_session = view.selected_id().cloned();
    let layout = view.center_layout(Rect::new(0, 0, 140, 22));
    assert_eq!(layout.log.height, 5);
    assert_eq!(layout.log_detail.height, 6);
    assert_eq!(layout.list.height, 5);
    let rendered = screen(&view, 140, 22);
    assert!(rendered.contains("Also inspect the tests"));
    assert!(!rendered.contains("Please inspect"));
}

#[test]
fn task_log_requires_room_for_header_and_six_body_rows() {
    let (mut view, now) = log_fixture();
    let hidden = log_screen(&view, 100, 18, now);
    assert!(!hidden.contains("Please inspect"));
    view.rows[view.selected].transcript = None;
    assert_eq!(hidden, log_screen(&view, 100, 18, now));
}

#[test]
fn task_log_rules_match_header_and_footer_gap_is_empty() {
    use ratatui::style::Modifier;
    let (mut view, now) = log_fixture();
    for loaded in [true, false] {
        if !loaded {
            view.log_session = None;
        }
        let area = Rect::new(0, 0, 100, 25);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        let middle = layout.prompt.y - 1;
        for x in 0..100 {
            for y in [middle, layout.log_detail.y] {
                if y != middle || x < layout.details.x || x >= layout.details.right() {
                    let rule = if (layout.details.x..layout.details.right()).contains(&x) {
                        &buf[(1, 1)]
                    } else {
                        &buf[(x, 1)]
                    };
                    let expected = if y == middle && x == layout.gap.x + 1 {
                        "┴"
                    } else if y == layout.log_detail.y && x == layout.gap.x + 1 {
                        "─"
                    } else {
                        rule.symbol()
                    };
                    assert_eq!(buf[(x, y)].symbol(), expected);
                    assert_eq!(buf[(x, y)].modifier, rule.modifier);
                }
            }
            if !loaded {
                for y in layout.log.y..layout.log.bottom() {
                    assert_eq!(buf[(x, y)].symbol(), " ");
                    assert_eq!(buf[(x, y)].modifier, Modifier::empty());
                }
                for y in layout.log_detail.y + 1..layout.footer.y {
                    assert_eq!(buf[(x, y)].symbol(), " ");
                    assert_eq!(buf[(x, y)].modifier, Modifier::empty());
                }
            }
            assert_eq!(buf[(x, 23)].symbol(), " ");
            assert_eq!(buf[(x, 23)].modifier, Modifier::empty());
        }
        assert!(buf[(2, middle)].modifier.contains(Modifier::DIM));
    }
}

#[test]
fn task_log_last_tool_clock_freezes_for_inactive_and_background_sessions() {
    let (mut view, now) = log_fixture();
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("35s"))
    );
    assert!(
        log_screen(&view, 100, 25, now + Duration::seconds(1))
            .lines()
            .any(|line| line.ends_with("36s"))
    );
    view.rows[view.selected].process = None;
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("25s"))
    );
    view.log.last_mut().unwrap().body.push_str(" *");
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("5s"))
    );
}

#[test]
fn task_log_last_message_uses_relative_time_and_missing_transcript_keeps_panel() {
    let (mut view, now) = log_fixture();
    view.rows[view.selected].model = Some("opus".into());
    view.log.pop();
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("40s ago"))
    );
    view.rows[view.selected].transcript = None;
    let rendered = log_screen(&view, 100, 25, now);
    assert!(!rendered.contains("Please inspect"));
    assert!(rendered.lines().nth(10).unwrap().starts_with("─╴"));
}

#[test]
fn non_current_log_time_matches_unselected_task_and_details_times() {
    let (mut view, now) = log_fixture();
    view.log.truncate(5);
    view.select_log_event(None);
    let mut other = view.rows[0].clone();
    other.id = session_key("other");
    other.title = "Other task".into();
    other.updated_at = now - Duration::minutes(1);
    let current = view.rows[0].clone();
    view.set_sessions(vec![current, other], false);

    let area = Rect::new(0, 0, 100, 25);
    let mut tasks = Buffer::empty(area);
    view.render_center_rows(area, &mut tasks, now);
    let task_y = (0..area.height)
        .find(|&y| {
            (0..area.width)
                .map(|x| tasks[(x, y)].symbol())
                .collect::<String>()
                .contains("Other task")
        })
        .expect("unselected task row");
    let task_time = &tasks[(99, task_y)];
    assert_ne!(task_time.symbol(), " ");
    assert!(task_time.modifier.contains(ratatui::style::Modifier::DIM));

    let mut log = Buffer::empty(area);
    view.render_at(area, &mut log, now);
    let y = view.center_layout(area).log.y;
    assert_ne!(log[(98, y)].symbol(), " ");
    assert_eq!(log[(98, y)].style(), task_time.style());

    view.rows[view.selected].last_message = Some("Reply".into());
    view.rows[view.selected].last_message_at = Some(now - Duration::minutes(1));
    let mut details = Buffer::empty(area);
    view.render_details(area, &mut details, now);
    let details_y = (0..area.height)
        .find(|&y| {
            (0..area.width)
                .map(|x| details[(x, y)].symbol())
                .collect::<String>()
                .contains("Update")
        })
        .expect("message header");
    assert_ne!(details[(99, details_y)].symbol(), " ");
    assert_eq!(details[(99, details_y)].style(), task_time.style());
}

#[test]
fn selected_log_row_preserves_foreground_and_adds_bold() {
    use crate::log::Kind;
    use ratatui::style::Modifier;
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let event = crate::providers::claude::log::tail_session(&source, 200, true)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == Kind::Tool)
        .unwrap();
    for light in [false, true] {
        let cursor = super::style::selection_style_for(false, true, light);
        super::style::with_selection_style(cursor, || {
            for width in [100, 149, 150, 200] {
                for kind in [
                    Kind::User,
                    Kind::Assistant,
                    Kind::Agent,
                    Kind::Session,
                    Kind::Tool,
                    Kind::Error,
                    Kind::Denied,
                    Kind::QueuedUser,
                    Kind::QueuedSystem,
                ] {
                    for search in [false, true] {
                        let (mut view, now) = log_fixture();
                        let mut event = event.clone();
                        event.kind = kind.clone();
                        // Include a visible link and a search match alongside plain body text.
                        event.body =
                            "Read `code` **bold** [link](https://example.com) and results".into();
                        event.calls.clear();
                        event.paths.clear();
                        let mut older = event.clone();
                        older.started_at -= Duration::seconds(1);
                        view.set_log(vec![older, event]);
                        if search {
                            view.log_search.query.set_text("Read");
                            view.rematch_log();
                        }
                        let area = Rect::new(0, 0, width, 60);
                        view.log_selected = None;
                        for index in 0..2 {
                            let mut baseline = Buffer::empty(area);
                            view.log_selected = None;
                            view.render_at(area, &mut baseline, now);
                            let log = view.viewport.borrow().log;
                            view.select_fixture_event(Some(view.log[index].clone()));
                            let mut selected = Buffer::empty(area);
                            view.render_at(area, &mut selected, now);
                            let y = log.y + index as u16;
                            for x in log.x - 1..=log.right() {
                                let before = &baseline[(x, y)];
                                let after = &selected[(x, y)];
                                assert_eq!(after.symbol(), before.symbol());
                                assert_eq!(
                                    after.fg, before.fg,
                                    "{kind:?}, width {width}, ({x}, {y})"
                                );
                                assert_eq!(
                                    after.modifier,
                                    before.modifier | Modifier::BOLD,
                                    "{kind:?}, width {width}, ({x}, {y})"
                                );
                                assert_eq!(
                                    after.bg,
                                    cursor.bg.unwrap(),
                                    "padding included at ({x}, {y})"
                                );
                            }
                        }
                    }
                }
            }
        });
    }
}

#[test]
fn text_and_tool_styles_follow_neovim_accent_in_both_modes() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    for (light, expected) in [
        (false, Color::Indexed(24)),
        (true, Color::Indexed(18)),
        (false, Color::Indexed(24)),
    ] {
        crate::theme::use_default_accents(light);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if light {
            "themes/light.tmTheme"
        } else {
            "themes/dark.tmTheme"
        });
        crate::theme::use_theme_file(&path, light).unwrap();
        for kind in [crate::log::Kind::Tool, crate::log::Kind::QueuedSystem] {
            let (label, body) = super::style::log_styles(&kind);
            assert_eq!(label.fg, Some(expected), "{kind:?}, light={light}");
            assert_eq!(body.fg, Some(expected), "{kind:?}, light={light}");
        }
        let lines = super::markdown::render("3. Run `cargo test`", 80);
        for text in ["3. ", "cargo test"] {
            let span = lines[0]
                .spans
                .iter()
                .find(|span| span.content == text)
                .unwrap();
            assert_eq!(span.style.fg, Some(expected), "{text}, light={light}");
        }
        assert_eq!(crate::theme::text_accent_color(), expected);
        assert_eq!(
            super::style::selection_style_for(false, true, light).fg,
            Some(expected)
        );
    }
}

#[test]
fn task_log_uses_cctail_colours_and_highlights_latest_time() {
    use crate::log::Kind;
    use ratatui::style::Modifier;
    let (mut view, now) = log_fixture();
    view.log.truncate(5);
    view.log_selected = None;
    let area = Rect::new(0, 0, 100, 25);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let top = view.center_layout(area).log.y;
    for (offset, colour) in [
        (0, Color::Reset),
        (1, Color::Yellow),
        (2, crate::theme::text_accent_color()),
        (3, Color::Red),
        (4, Color::Reset),
    ] {
        let y = top + offset;
        let x = (1..89).find(|&x| buf[(x, y)].symbol() != " ").unwrap();
        let label = &buf[(x, y)];
        let body = &buf[(x, y)];
        assert_eq!(label.fg, colour, "label row {y}");
        assert_eq!(body.fg, colour, "body row {y}");
        assert!(!label.modifier.contains(Modifier::BOLD), "row {y}");
        assert!(!label.modifier.contains(Modifier::DIM), "label row {y}");
        assert!(
            !body.modifier.intersects(Modifier::DIM | Modifier::BOLD),
            "body row {y}"
        );
    }
    for y in top..top + 4 {
        assert_eq!(buf[(98, y)].fg, Color::Reset, "time row {y}");
        assert_eq!(buf[(98, y)].modifier, Modifier::DIM, "time row {y}");
    }
    for (kind, colour) in [
        (Kind::User, Color::Reset),
        (Kind::Assistant, Color::Yellow),
        (Kind::Agent, Color::Yellow),
        (Kind::Session, Color::Yellow),
        (Kind::Tool, crate::theme::text_accent_color()),
        (Kind::Error, Color::Red),
        (Kind::Denied, Color::Red),
        (Kind::QueuedUser, Color::Reset),
        (Kind::QueuedSystem, crate::theme::text_accent_color()),
    ] {
        let (label_style, body_style) = super::style::log_styles(&kind);
        let foreground = (colour != Color::Reset).then_some(colour);
        assert_eq!(label_style.fg, foreground);
        assert_eq!(body_style.fg, foreground);
        view.log.first_mut().unwrap().kind = kind.clone();
        view.log.first_mut().unwrap().previous_end = None;
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let time = &buf[(98, top)];
        let time_style = super::style::log_time_style(&kind, false);
        assert_eq!(time_style, super::style::time_style());
        assert_eq!(time.fg, Color::Reset);
        assert_eq!(time.modifier, Modifier::DIM);
        view.log.last_mut().unwrap().kind = kind;
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let x = (1..89)
            .find(|&x| buf[(x, top + 4)].symbol() != " ")
            .unwrap();
        for x in [x, x + 10] {
            let cell = &buf[(x, top + 4)];
            assert_eq!(cell.fg, colour);
            assert!(!cell.modifier.contains(Modifier::BOLD));
            assert!(!cell.modifier.contains(Modifier::DIM));
        }
        let time = &buf[(98, top + 4)];
        assert_ne!(time.symbol(), " ");
        assert_eq!(time.fg, colour);
        assert!(time.modifier.contains(Modifier::BOLD));
        assert!(!time.modifier.contains(Modifier::DIM));
    }
}

#[test]
fn task_log_preserves_time_when_truncated() {
    use crate::log::Kind;
    use ratatui::style::Modifier;
    let (view, now) = log_fixture();
    let area = Rect::new(0, 0, 100, 25);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let y = view.center_layout(area).log.bottom() - 2;
    assert_eq!(buf[(88, y)].symbol(), "…");
    assert_eq!(buf[(98, y)].fg, Color::Reset);
    assert_eq!(buf[(98, y)].modifier, Modifier::DIM);
    for kind in [
        Kind::User,
        Kind::Assistant,
        Kind::Agent,
        Kind::Session,
        Kind::QueuedUser,
        Kind::QueuedSystem,
    ] {
        let mut view = log_fixture().0;
        let event = view.log.last_mut().unwrap();
        event.kind = kind;
        assert!(
            log_screen(&view, 100, 25, now)
                .lines()
                .any(|line| line.ends_with("30s ago"))
        );
    }
}

#[test]
fn task_log_right_aligns_mixed_width_times_and_centers_content() {
    use crate::log::Kind;
    let (mut view, now) = log_fixture();
    view.log.truncate(3);
    view.select_log_event(None);
    view.log[0].kind = Kind::Tool;
    view.log[0].previous_end = Some(now - Duration::seconds(123));
    view.log[0].ended_at = Some(now - Duration::seconds(120));
    view.log[1].started_at = now + Duration::seconds(225);
    view.log[1].previous_end = None;
    view.log[2].kind = Kind::Assistant;
    view.log[2].started_at = now - Duration::minutes(44);
    let rendered = log_screen(&view, 100, 25, now);
    assert_eq!(
        rendered
            .lines()
            .skip(view.center_layout(Rect::new(0, 0, 100, 25)).log.y as usize)
            .take(3)
            .map(|line| line.trim_start().chars().take(9).collect::<String>())
            .collect::<Vec<_>>(),
        ["Please in", "I will ch", "Read src/"]
    );
    for (index, (line, (time, label))) in rendered
        .lines()
        .skip(view.center_layout(Rect::new(0, 0, 100, 25)).log.y as usize)
        .take(3)
        .zip([
            ("3s", "Please in"),
            ("12:03:45", "I will ch"),
            ("44m ago", "Read src/"),
        ])
        .enumerate()
    {
        assert_eq!(
            line[..line.find(time).unwrap()].chars().count() + time.len(),
            99,
            "times end at the same column"
        );
        let body = super::log::body_line(&view.log[index], 88, &view.rows[0].cwd);
        assert_eq!(line.find(label), Some(50 - body.width() / 2));
    }
}

#[test]
fn task_log_shows_only_visible_events_and_hides_without_selection() {
    let (mut view, now) = log_fixture();
    let first = crate::log::Event {
        source: None,
        completed_time: false,
        background: false,
        source_records: Vec::new(),
        sender: None,
        kind: crate::log::Kind::User,
        body: "Discarded event".into(),
        paths: Vec::new(),
        started_at: now - Duration::seconds(180),
        ended_at: None,
        duration: None,
        previous_end: None,
        images: Vec::new(),
        calls: Vec::new(),
    };
    view.log.insert(0, first);
    view.log.extend(vec![view.log.last().unwrap().clone(); 2]);
    assert!(!log_screen(&view, 100, 25, now).contains("Discarded event"));
    view.selected = usize::MAX;
    let rendered = log_screen(&view, 100, 25, now);
    assert!(!rendered.contains("Please inspect"));
    assert!(!rendered.lines().nth(14).unwrap().contains('─'));
}

#[test]
fn center_layout_narrow_buffer_ranges_at_120_columns() {
    let (view, now) = log_fixture();
    let area = Rect::new(0, 0, 120, 60);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let layout = view.center_layout(area);
    assert_eq!(
        [layout.list, layout.details, layout.log].map(|rect| rect.x..rect.right()),
        [1..59, 62..119, 1..119],
    );
    for y in layout.list.y..layout.list.bottom() {
        let separators: Vec<_> = (0..120).filter(|&x| buf[(x, y)].symbol() == "│").collect();
        assert_eq!(separators, [60]);
    }
    assert!(layout.log.y >= layout.list.bottom());
}

#[test]
fn center_layout_wide_balances_inner_content_across_sizes() {
    for (width, height) in [(200, 60), (200, 61), (180, 40), (160, 50), (150, 50)] {
        assert_equal_content_buffer(width, height, "");
    }
}

#[test]
fn center_layout_wide_equal_content_buffer_at_200x45() {
    assert_equal_content_buffer(200, 45, "");
}

#[test]
fn center_layout_equal_content_buffer_with_growing_input() {
    for (width, height) in [
        (200, 60),
        (200, 61),
        (200, 45),
        (180, 40),
        (160, 50),
        (120, 60),
        (120, 61),
        (120, 45),
        (100, 40),
        (149, 50),
    ] {
        for lines in 2..=7 {
            assert_equal_content_buffer(width, height, &vec!["input"; lines].join("\n"));
        }
        assert_equal_content_buffer(width, height, &"wrapped input ".repeat(100));
    }
}

#[test]
fn center_layout_narrow_balances_inner_content_across_sizes() {
    for (width, height) in [(120, 60), (120, 61), (100, 40), (149, 50)] {
        assert_equal_content_buffer(width, height, "");
    }
}

#[test]
fn center_layout_narrow_equal_content_buffer_at_100x45() {
    assert_equal_content_buffer(100, 45, "");
}

fn assert_equal_content_buffer(width: u16, height: u16, input: &str) {
    let (mut view, now) = log_fixture();
    view.input_box.editor.set_text(input);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let layout = view.center_layout(area);
    let top = (0..height)
        .filter(|&y| buf[(layout.gap.x + 1, y)].symbol() == "│")
        .count() as u16;
    let wide = width >= super::render::WIDE_LAYOUT_COLUMNS;
    let available = height
        - layout.header.height
        - layout.footer.height
        - layout.prompt.height
        - 2
        - u16::from(!wide);
    let detail = layout.log_detail.height.saturating_sub(1);
    // Count content between the rendered detail rule and footer, excluding the rule.
    assert_eq!(detail, layout.footer.y - layout.log_detail.y - 1);
    let heights = if wide {
        vec![top, detail]
    } else {
        vec![top, layout.log.height, detail]
    };
    assert_eq!(heights.iter().sum::<u16>(), available);
    assert!(heights.windows(2).all(|pair| pair[0] >= pair[1]));
    assert!(heights[0] - heights[heights.len() - 1] <= 1);
    let expected = if wide {
        (available.div_ceil(2), available.div_ceil(2), available / 2)
    } else {
        (available.div_ceil(3), (available + 1) / 3, available / 3)
    };
    println!(
        "{width}x{height}, input {} rows: top {top}, log {}, detail {detail}",
        layout.prompt.height, layout.log.height
    );
    assert_eq!((top, layout.log.height, detail), expected);
    assert_eq!(layout.details.height, top);
    assert_eq!(layout.prompt.y, layout.details.bottom() + 1);
    let status_left = layout.gap.x + 1;
    let status_right = if wide { layout.log.x - 1 } else { area.right() };
    assert_eq!(
        buf[(
            status_left + (status_right - status_left - 9) / 2 + 1,
            layout.details.bottom(),
        )]
            .symbol(),
        "W"
    );
    assert_eq!(buf[(layout.details.x, layout.log_detail.y)].symbol(), "─");
    if wide {
        assert_eq!(layout.prompt.bottom(), layout.log_detail.y);
    } else {
        assert_eq!(layout.prompt.bottom() + 1, layout.log.y);
        assert_eq!(layout.log.bottom(), layout.log_detail.y);
        assert_eq!(layout.log_detail.bottom(), layout.footer.y);
        assert!(buf[(0, layout.log.y)].symbol() != "│");
    }
}

#[test]
fn center_layout_wide_buffer_ranges_at_200_columns() {
    assert_wide_buffer_ranges(200, [1..49, 52..100, 103..199]);
}

#[test]
fn center_layout_wide_buffer_ranges_at_151_columns() {
    assert_wide_buffer_ranges(151, [1..36, 39..74, 77..150]);
}

fn assert_wide_buffer_ranges(width: u16, expected: [std::ops::Range<u16>; 3]) {
    let (view, now) = log_fixture();
    let area = Rect::new(0, 0, width, 60);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let layout = view.center_layout(area);
    assert_eq!(
        [layout.list, layout.details, layout.log].map(|rect| rect.x..rect.right()),
        expected,
    );
    for y in layout.list.y..layout.list.bottom() {
        let separators: Vec<_> = (0..width)
            .filter(|&x| buf[(x, y)].symbol() == "│")
            .collect();
        assert_eq!(separators, [expected[0].end + 1, expected[1].end + 1]);
    }
}

#[test]
fn center_layout_golden_rects_follow_each_area() {
    let (view, _) = log_fixture();
    for (width, height, top, log_height, list_width, details_width, body_height, list_height) in [
        (80, 24, 17, 6, 38, 37, 6, 6),
        (120, 40, 27, 12, 58, 57, 11, 11),
        (200, 60, 31, 28, 48, 48, 27, 27),
        (80, 24, 17, 6, 38, 37, 6, 6),
    ] {
        let area = Rect::new(5, 7, width, height);
        let layout = view.center_layout(area);
        assert_eq!(layout.header, Rect::new(5, 7, width, 2));
        assert_eq!(layout.list, Rect::new(6, 9, list_width, body_height));
        let bottom = if width >= super::render::WIDE_LAYOUT_COLUMNS {
            assert_eq!(layout.log.y, layout.list.y);
            assert_eq!(layout.log.width, width - 8 - 2 * list_width);
            layout.log_detail
        } else {
            assert_eq!(
                layout.log,
                Rect::new(6, 9 + body_height + 3, width - 2, list_height)
            );
            layout.log_detail
        };
        assert_eq!(bottom, Rect::new(5, 7 + top, width, log_height));
        assert_eq!(layout.footer, Rect::new(5, 7 + height - 1, width, 1));
        if details_width == 0 {
            assert_eq!(layout.details, Rect::default());
            assert_eq!(layout.gap, Rect::default());
        } else {
            assert_eq!(layout.gap, Rect::new(6 + list_width, 9, 3, body_height));
            assert_eq!(
                layout.details,
                Rect::new(9 + list_width, 9, details_width, body_height)
            );
        }
    }
}

#[test]
fn center_layout_log_and_details_hide_thresholds() {
    let (mut view, _) = log_fixture();
    for height in 0..19 {
        let layout = view.center_layout(Rect::new(0, 0, 94, height));
        assert_eq!(layout.log.height, 0, "height {height}");
    }
    let layout = view.center_layout(Rect::new(0, 0, 94, 19));
    assert_eq!(layout.log.height, 4);
    assert_eq!(layout.list.height, 4);
    assert_eq!(layout.log_detail.height, 5);
    assert!(layout.details.width > 0);
    assert!(
        !view
            .center_layout(Rect::new(0, 0, 93, 20))
            .details
            .is_empty()
    );
    let panels = |view: &View, height| {
        let layout = view.center_layout(Rect::new(0, 0, 120, height));
        [
            layout.list,
            layout.details,
            layout.log,
            layout.prompt,
            layout.log_detail,
        ]
    };
    let with_transcript = [24, 40, 60].map(|height| panels(&view, height));
    view.rows[view.selected].transcript = None;
    for (height, expected) in [24, 40, 60].into_iter().zip(with_transcript) {
        assert_eq!(panels(&view, height), expected, "height {height}");
    }
}

#[test]
fn task_log_event_count_follows_height() {
    let (mut view, now) = log_fixture();
    let event = view.log[0].clone();
    view.log = (0..30)
        .map(|index| {
            let mut event = event.clone();
            event.body = format!("event-{index:02}");
            event
        })
        .collect();
    view.select_log_event(None);
    for (height, count) in [(24, 6), (40, 11), (60, 18), (18, 0), (24, 6)] {
        let rendered = log_list_screen(&view, 120, height, now);
        let events: Vec<_> = rendered
            .lines()
            .filter(|line| line.contains("event-"))
            .collect();
        assert_eq!(events.len(), count, "height {height}");
        if count > 0 {
            assert!(events[0].contains(&format!("event-{:02}", 30 - count)));
            assert!(events.last().unwrap().contains("event-29"));
            assert!(
                rendered
                    .lines()
                    .nth(usize::from(
                        view.center_layout(Rect::new(0, 0, 120, height))
                            .log
                            .bottom()
                            - 1
                    ))
                    .unwrap()
                    .contains("event-")
            );
        }
    }
}

#[path = "details_scroll_tests.rs"]
mod details_scroll;

#[test]
fn details_status_uses_only_the_last_folder_component() {
    let mut task = session("path", "Path task", SessionStatus::Ready, 0);
    task.cwd = "/Users/visitor/Code/.worktrees/agents-panel".into();
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![task], false);
    let rendered = screen(&view, 200, 30);
    let viewport = view.viewport.borrow();
    let (rect, path) = viewport
        .details_links
        .iter()
        .find(|(_, path)| path.ends_with("agents-panel"))
        .unwrap();
    assert_eq!(rect.y, viewport.details.bottom());
    assert_eq!(
        path,
        &PathBuf::from("/Users/visitor/Code/.worktrees/agents-panel")
    );
    let rule = rendered.lines().nth(rect.y as usize).unwrap();
    assert!(rule.contains("agents-panel"));
    assert!(!rule.contains("/Users/"));
}

#[test]
fn path_abbreviation_in_group_header_preserves_last_component() {
    let mut task = session("path", "Path task", SessionStatus::Ready, 0);
    task.cwd = "/Users/visitor/Code/.worktrees/agents-panel".into();
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![task], false);
    let area = Rect::new(0, 0, 30, 6);
    let mut buf = Buffer::empty(area);
    view.render_center_rows(area, &mut buf, Utc::now());
    let heading = buf
        .content
        .chunks(30)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .find(|row| row.trim_start().starts_with('/'))
        .unwrap();
    assert_eq!(heading.trim(), "/U…/v…/C…/.w…/agents-panel");
}

mod log_path_abbrev {
    use super::*;

    #[test]
    fn read_path_is_abbreviated_to_the_available_body_width() {
        let (mut view, now) = log_fixture();
        let mut event = view.log[2].clone();
        let home = std::env::var_os("HOME").expect("HOME");
        let path = std::path::Path::new(&home).join("Users/workspace/project/main.rs");
        event.body = format!("Read {}", path.display());
        event.paths.push(5..event.body.len());
        view.log = vec![event];
        view.select_log_event(None);
        let area = Rect::new(0, 0, 43, 4);
        let mut buf = Buffer::empty(area);
        view.render_log(area, &mut buf, now);
        let row = buf
            .content
            .chunks(43)
            .next()
            .unwrap()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(row.contains("Read ~/U…/w…/project/main.rs"), "{row}");
    }
}

mod log_help {
    use super::*;

    #[test]
    fn help_has_log_column_right_of_view() {
        let mut view = simple_fixture();
        key(&mut view, KeyCode::Char('?'));
        let rendered = screen(&view, 100, 24);
        let headings = rendered.lines().find(|line| line.contains("View")).unwrap();
        assert!(headings.find("Log").unwrap() > headings.find("View").unwrap());
        assert_eq!(
            rendered,
            include_str!("../../tests/ui_snapshots/live_center_help_log.txt")
        );
    }
}

mod log_search {
    use super::*;
    use ratatui::style::Modifier;

    fn open(view: &mut View) {
        {
            view.mouse_focus = Panel::LogList;
            view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        };
    }

    fn panel(view: &View, now: chrono::DateTime<Utc>) -> Buffer {
        let area = Rect::new(0, 0, 100, 12);
        let mut buf = Buffer::empty(area);
        view.render_log(area, &mut buf, now);
        buf
    }

    fn text(buf: &Buffer) -> String {
        buf.content.iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn leaving_list_search_preserves_event_and_detail_search() {
        for mouse_exit in [false, true] {
            let (mut view, now) = log_fixture();
            view.select_log_event(Some(1));
            let selected = view.visible_log_event().unwrap().clone();
            open(&mut view);
            for c in "files".chars() {
                key(&mut view, KeyCode::Char(c));
            }
            assert_eq!(view.visible_log_event(), Some(&selected));
            screen(&view, 120, 50);
            let detail = view.viewport.borrow().log_detail;
            if mouse_exit {
                click_mouse(
                    &mut view,
                    mouse(
                        MouseEventKind::Down(MouseButton::Left),
                        detail.x + 2,
                        detail.y + 3,
                    ),
                );
            } else {
                key(&mut view, KeyCode::Esc);
                view.mouse_focus = Panel::LogDetail;
            }
            assert!(!view.log_search.active());
            assert_eq!(view.visible_log_event(), Some(&selected));
            let area = Rect::new(0, 0, 100, 40);
            let mut buf = Buffer::empty(area);
            view.render_log_detail(area, &mut buf, now);
            assert!(text(&buf).contains("I will check the files"));
            key(&mut view, KeyCode::Char('/'));
            for c in "files".chars() {
                key(&mut view, KeyCode::Char(c));
            }
            assert!(view.log_detail_search.editing);
            let mut buf = Buffer::empty(area);
            view.render_log_detail(area, &mut buf, now);
            assert!(text(&buf).contains("I will check the files"));
            assert!(text(&buf).contains("Search ❯ files"));
        }
    }

    #[test]
    fn input_live_match_highlight_keep_and_escape() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let (mut view, now) = log_fixture();
        let template = view.log[0].clone();
        view.log = [
            "plain `timeout` **bold**",
            "unrelated",
            "t........i........m........e........o........u........t",
        ]
        .map(|body| {
            let mut event = template.clone();
            event.body = body.into();
            event
        })
        .to_vec();
        open(&mut view);
        assert!(text(&panel(&view, now)).contains("Search ❯ "));
        assert!(
            view.center_footer_hints()
                .to_string()
                .contains("enter keep")
        );
        for c in "time out".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        let buf = panel(&view, now);
        assert!(text(&buf).contains("plain timeout bold"));
        assert!(!text(&buf).contains("unrelated"));
        assert!(!text(&buf).contains("t........"));
        let start = text(&buf).find("plain timeout bold").unwrap() as u16;
        for x in start + 6..start + 13 {
            assert!(buf[(x, 0)].modifier.contains(Modifier::BOLD));
            assert_eq!(buf[(x, 0)].fg, crate::theme::inline_code_color());
        }
        key(&mut view, KeyCode::Enter);
        let buf = panel(&view, now);
        assert!(text(&buf).contains("timeout"));
        assert!(buf[(99, 11)].modifier.contains(Modifier::DIM));
        assert!(!text(&buf).contains("Search ❯ "));
        open(&mut view);
        assert!(text(&panel(&view, now)).contains("Search ❯ time out"));
        key(&mut view, KeyCode::Backspace);
        assert!(text(&panel(&view, now)).contains("Search ❯ time ou"));
        key(&mut view, KeyCode::Esc);
        assert!(text(&panel(&view, now)).contains("unrelated"));
    }

    #[test]
    fn atoms_filters_order_and_unicode_styles() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        use crate::log::Kind;
        let (mut view, now) = log_fixture();
        let template = view.log[0].clone();
        view.log = [
            (Kind::Assistant, "timeout connection out"),
            (Kind::Tool, "timeout alone"),
            (Kind::Error, "timeout connection"),
            (Kind::User, "plain `café` [link](url)"),
        ]
        .into_iter()
        .map(|(kind, body)| {
            let mut event = template.clone();
            event.kind = kind;
            event.body = body.into();
            event
        })
        .collect();
        for (query, expected) in [
            ("timeout conn", vec![0, 2]),
            ("conn timeout", vec![0, 2]),
            ("time out", vec![0, 1, 2]),
            ("tmo", vec![]),
            ("  out\t time  ", vec![0, 1, 2]),
            ("time missing", vec![]),
            ("Timeout", vec![]),
            ("'timeout", vec![]),
            ("^timeout", vec![]),
            ("connection$", vec![]),
            ("!timeout", vec![]),
            ("tool", vec![1]),
            ("assistant timeout", vec![0]),
            ("error conn", vec![2]),
            ("user café", vec![3]),
        ] {
            open(&mut view);
            for c in query.chars() {
                key(&mut view, KeyCode::Char(c));
            }
            let matches: Vec<_> = (0..view.log.len())
                .filter(|&i| view.log_matches(i))
                .collect();
            assert_eq!(matches, expected, "{query}");
            key(&mut view, KeyCode::Esc);
        }
        open(&mut view);
        for c in "cafe".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        let buf = panel(&view, now);
        assert!(text(&buf).contains("plain café link"));
        let start = text(&buf).find("plain café link").unwrap() as u16;
        for x in start + 6..start + 10 {
            assert_eq!(buf[(x, 0)].fg, crate::theme::inline_code_color());
            assert!(buf[(x, 0)].modifier.contains(Modifier::BOLD));
        }
        assert!(!buf[(start + 11, 0)].modifier.contains(Modifier::UNDERLINED));
        assert!(buf[(start + 11, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn selection_change_clears_and_wheel_still_scrolls() {
        let (mut view, now) = log_fixture();
        open(&mut view);
        key(&mut view, KeyCode::Char('a'));
        screen(&view, 120, 40);
        let rect = view.viewport.borrow().log;
        view.handle_mouse(mouse(
            MouseEventKind::ScrollUp,
            rect.x,
            rect.y + super::log::LOG_TOP,
        ));
        assert_eq!(view.visible_log_event(), view.log.get(5));
        let mut second = view.rows[0].clone();
        second.id = session_key("second");
        view.set_sessions(vec![view.rows[0].clone(), second], false);
        let index = view
            .rows
            .iter()
            .position(|row| row.id == session_key("second"))
            .unwrap();
        view.set_selected(index);
        view.log_session = Some(session_key("second"));
        let buf = panel(&view, now);
        assert!(!text(&buf).contains("Search ❯ "));
        assert_eq!(
            view.center_footer_hints().to_string(),
            super::super::hints::hint_line(&[
                ("?", "help"),
                ("esc", "back"),
                ("↑/↓", "move"),
                ("enter", "open"),
                ("n", "new"),
            ])
            .to_string()
        );
    }
}

mod line_edit {
    use super::*;

    #[test]
    fn task_search_movement_does_not_reach_list_handlers() {
        let mut view = fixture();
        key(&mut view, KeyCode::Char('/'));
        key(&mut view, KeyCode::Down);
        let selected = view.selected;
        for code in [KeyCode::Home, KeyCode::End, KeyCode::Right, KeyCode::Left] {
            assert_eq!(key(&mut view, code), None);
            assert_eq!(view.selected, selected);
        }
        for c in ['b', 'f', 'k'] {
            assert_eq!(
                view.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)),
                None
            );
            assert_eq!(view.selected, selected);
        }
    }

    #[test]
    fn task_search_kill_stays_on_selected_row() {
        let mut view = fixture();
        key(&mut view, KeyCode::Char('/'));
        for c in "task".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        key(&mut view, KeyCode::Down);
        let selected = view.selected;
        key(&mut view, KeyCode::Left);
        view.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(view.search.text(), "tas");
        assert_eq!(view.selected, selected);
    }

    #[test]
    fn log_search_caret_follows_cjk_inset_text() {
        let (mut view, now) = log_fixture();
        {
            view.mouse_focus = Panel::LogList;
            view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        };
        for c in "你好".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let y = view.center_layout(area).log.bottom() - 1;
        assert_eq!(buf[(10, y)].symbol(), "你");
        assert_eq!(buf[(12, y)].symbol(), "好");
        assert_eq!(view.cursor_pos(area), Some((14, y)));
    }

    #[test]
    fn rename_inserts_in_the_middle() {
        let mut view = fixture();
        key(&mut view, KeyCode::Char('r'));
        let title = view.rename.text().to_owned();
        key(&mut view, KeyCode::Left);
        key(&mut view, KeyCode::Char('X'));
        let mut expected = title;
        expected.insert(expected.len() - 1, 'X');
        assert!(
            matches!(key(&mut view, KeyCode::Enter), Some(Action::Rename { name, .. }) if name == expected)
        );
    }
}

mod input_row {
    use super::*;

    fn rendered_inputs(
        width: u16,
        query: &str,
        left: bool,
    ) -> (Buffer, Rect, Buffer, Rect, View, View) {
        let area = Rect::new(3, 2, width, 40);
        let (mut task, now) = log_fixture();
        key(&mut task, KeyCode::Char('/'));
        task.search.set_text(query);
        let (mut log, _) = log_fixture();
        {
            log.mouse_focus = Panel::LogList;
            log.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        };
        log.log_search.query.set_text(query);
        if left {
            task.search.left();
            log.log_search.query.left();
        }
        let mut task_buf = Buffer::empty(area);
        task.render_at(area, &mut task_buf, now);
        let mut log_buf = Buffer::empty(area);
        log.render_at(area, &mut log_buf, now);
        let task_row = task.center_layout(area).search;
        let log_row = super::super::log::search_input_rect(log.center_layout(area).log);
        (task_buf, task_row, log_buf, log_row, task, log)
    }

    #[test]
    fn search_prompt_cells_and_whole_row_style_match() {
        for (no_color, colors, light) in [
            (false, true, false),
            (false, true, true),
            (true, true, false),
            (false, false, false),
        ] {
            let style = super::super::style::selection_style_for(no_color, colors, light);
            super::super::style::with_selection_style(style, || {
                for width in [14, 24, 88] {
                    let (task, task_row, mut log, mut log_row, _, log_view) =
                        rendered_inputs(width, "你好 query", false);
                    Clear.render(log_row, &mut log);
                    log_row.width = task_row.width;
                    super::super::render::render_input_row(
                        "Search ❯ ",
                        &log_view.log_search.query,
                        log_row,
                        &mut log,
                    );
                    assert_eq!(task_row.x, log_row.x);
                    for x in 0..task_row.width {
                        let task_cell = &task[(task_row.x + x, task_row.y)];
                        let log_cell = &log[(log_row.x + x, log_row.y)];
                        assert_eq!(task_cell, log_cell, "width={width}, column={x}");
                        assert_eq!(
                            task_cell.bg,
                            super::super::style::active_tab_style()
                                .bg
                                .unwrap_or(Color::Reset)
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn search_prompt_keeps_cyan_and_light_background_for_wide_text() {
        use super::super::style::{selection_style_for, with_selection_style};
        let light = selection_style_for(false, true, true);
        with_selection_style(light, || {
            let (task, row, _, _, _, _) = rendered_inputs(24, "你好", false);
            assert_eq!(task[(row.x, row.y)].fg, Color::Cyan);
            for x in row.x..row.right() {
                assert_eq!(task[(x, row.y)].bg, Color::Indexed(111));
            }
        });
    }

    #[test]
    fn search_carets_follow_the_same_visible_window() {
        for width in [8, 14, 24, 88] {
            for query in ["", "abc", "你好 query that scrolls past the edge"] {
                for left in [false, true] {
                    let (_, task_row, _, log_row, task, log) = rendered_inputs(width, query, left);
                    let area = Rect::new(3, 2, width, 40);
                    let expected = (9 + task
                        .search
                        .window(usize::from(task_row.width.saturating_sub(9)))
                        .1)
                        .min(task_row.width.saturating_sub(1));
                    assert_eq!(
                        task.cursor_pos(area),
                        Some((task_row.x + expected, task_row.y))
                    );
                    let log_expected = (9 + log
                        .log_search
                        .query
                        .window(usize::from(log_row.width.saturating_sub(9)))
                        .1)
                        .min(log_row.width.saturating_sub(1));
                    assert_eq!(
                        log.cursor_pos(area),
                        Some((log_row.x + log_expected, log_row.y))
                    );
                }
            }
        }
    }
}

#[test]
fn task_log_search_active_snapshot() {
    let (mut view, now) = log_fixture();
    {
        view.mouse_focus = Panel::LogList;
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
    };
    for c in "tests".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    assert_eq!(
        log_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_log_search_active.txt")
    );
}

mod subagent_rows {
    use super::*;

    pub(super) fn fixture() -> View {
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let mut parent = session("parent", "Parent task", SessionStatus::Ready, 120);
        parent.updated_at = now - Duration::minutes(2);
        parent.model = Some("parent-model".into());
        let mut older = session("agent-older", "Check files", SessionStatus::Working, 60);
        older.parent = Some(parent.id.clone());
        older.updated_at = now - Duration::minutes(1);
        older.model = Some("child-model".into());
        older.name = Some("worker".into());
        older.cwd = "/tmp/child-project".into();
        let mut newer = session("agent-newer", "Review results", SessionStatus::Ready, 0);
        newer.parent = Some(parent.id.clone());
        newer.updated_at = now;
        let mut view = test_view(Grouping::Project, false);
        view.set_sessions(vec![newer, older, parent], false);
        view
    }

    pub(super) fn screen(view: &View, width: u16, height: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        view.render_at(buf.area, &mut buf, "2026-10-13T12:00:00Z".parse().unwrap());
        buf.content
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn set_filter(view: &mut View, tab: usize) {
        view.status_filter = tab;
        view.reconcile_selection();
    }

    fn ids(view: &View) -> Vec<&str> {
        view.visible_indices()
            .iter()
            .map(|&i| view.rows[i].id.id.as_str())
            .collect()
    }

    #[test]
    fn child_connectors_use_tees_until_the_last_visible_sibling() {
        for count in [1, 3] {
            let mut view = fixture();
            let parent = view
                .rows
                .iter()
                .find(|row| row.parent.is_none())
                .unwrap()
                .clone();
            let mut rows = vec![parent.clone()];
            for index in 0..count {
                let mut child = session(
                    &format!("child-{index}"),
                    &format!("Child {index}"),
                    SessionStatus::Working,
                    index,
                );
                child.parent = Some(parent.id.clone());
                rows.push(child);
            }
            view.set_sessions(rows, false);
            for width in [100, 200] {
                let area = Rect::new(0, 0, width, 30);
                let mut buf = Buffer::empty(area);
                view.render(area, &mut buf);
                let list = view.center_layout(area).list;
                for index in 0..count {
                    let y = (list.y..list.bottom())
                        .find(|&y| {
                            (list.x..list.right())
                                .map(|x| buf[(x, y)].symbol())
                                .collect::<String>()
                                .contains(&format!("Child {index}"))
                        })
                        .unwrap();
                    let prefix = (list.x..list.x + 3)
                        .map(|x| buf[(x, y)].symbol())
                        .collect::<String>();
                    assert_eq!(
                        prefix,
                        if index + 1 == count {
                            "└─ "
                        } else {
                            "├─ "
                        }
                    );
                    assert_eq!(buf[(list.x + 3, y)].symbol(), "●");
                    click_mouse(
                        &mut view,
                        mouse(MouseEventKind::Down(MouseButton::Left), list.x, y),
                    );
                    assert_eq!(
                        view.selected_id(),
                        Some(&session_key(&format!("child-{index}")))
                    );
                }
            }
        }
    }

    #[test]
    fn children_follow_same_newest_first_order_as_top_level_rows() {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        let mut middle = session("agent-middle", "Middle task", SessionStatus::Working, 30);
        middle.parent = Some(session_key("parent"));
        middle.updated_at = rows
            .iter()
            .find(|s| s.id == session_key("agent-newer"))
            .unwrap()
            .updated_at
            - Duration::seconds(30);
        rows.push(middle);
        view.set_sessions(rows, false);
        for grouping in [Grouping::Project, Grouping::Model] {
            view.grouping = grouping;
            assert_eq!(
                ids(&view),
                ["parent", "agent-newer", "agent-middle", "agent-older"]
            );
        }
    }

    #[test]
    fn top_level_rows_and_siblings_break_update_ties_by_id() {
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let mut rows = Vec::new();
        for (id, status) in [
            ("b", SessionStatus::Working),
            ("a", SessionStatus::Ready),
            ("c", SessionStatus::Working),
        ] {
            let mut row = session(id, id, status, 0);
            row.updated_at = now;
            rows.push(row);
        }
        let mut view = test_view(Grouping::Project, false);
        for grouping in [Grouping::Project, Grouping::Model] {
            view.grouping = grouping;
            view.set_sessions(rows.clone(), false);
            assert_eq!(ids(&view), ["a", "b", "c"]);
            let mut children = rows.clone();
            for child in &mut children {
                child.parent = Some(session_key("parent"));
            }
            children.push(session("parent", "Parent", SessionStatus::Ready, 0));
            view.set_sessions(children, false);
            assert_eq!(ids(&view), ["parent", "a", "b", "c"]);
        }
    }

    #[test]
    fn renders_children_newest_first_with_own_status() {
        let view = fixture();
        assert_eq!(ids(&view), ["parent", "agent-newer", "agent-older"]);
        let rendered = screen(&view, 120, 30);
        let parent = rendered
            .lines()
            .position(|l| l.contains("○ Parent task"))
            .unwrap();
        let lines: Vec<_> = rendered.lines().collect();
        assert!(lines[parent + 2].contains("└─ ● Check files"), "{rendered}");
        assert!(lines[parent + 2].contains("Working"));
        assert!(lines[parent + 1].contains("├─ ○ Review results"));
        assert!(lines[parent + 1].contains("Ready"));
    }

    #[test]
    fn tabs_filter_inactive_and_refresh_keeps_child_selected() {
        let mut view = fixture();
        set_filter(&mut view, 1);
        assert!(ids(&view).is_empty());
        set_filter(&mut view, 0);
        let child = view
            .rows
            .iter()
            .position(|s| s.id == session_key("agent-older"))
            .unwrap();
        view.select(child);
        let mut rows = view.rows.clone();
        rows[child].status = SessionStatus::NeedsInput;
        view.set_sessions(rows, false);
        assert_eq!(view.status_filter, 0);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        assert_eq!(ids(&view), ["parent", "agent-newer", "agent-older"]);
    }

    #[test]
    fn tab_counts_use_each_rows_status() {
        let view = fixture();
        let rendered = screen(&view, 160, 30);
        for label in ["Active 3", "Inactive 0"] {
            assert!(rendered.contains(label), "{rendered}");
        }
    }

    #[test]
    fn a_tab_without_its_chosen_task_counts_subagents_when_picking_the_longest_waiting() {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        let mut oldest = session("agent-oldest", "Old subagent", SessionStatus::Ready, 0);
        oldest.parent = Some(session_key("parent"));
        oldest.updated_at = rows
            .iter()
            .find(|s| s.id == session_key("parent"))
            .unwrap()
            .updated_at
            - Duration::hours(1);
        rows.push(oldest);
        view.set_sessions(rows, false);

        view.tab_selections[0] = None;
        key(&mut view, KeyCode::Tab);
        key(&mut view, KeyCode::Tab);

        assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
        assert_eq!(view.selected_id(), Some(&session_key("agent-oldest")));
    }

    #[test]
    fn inactive_root_hides_children_in_active_tab() {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        rows.iter_mut()
            .find(|row| row.id == session_key("parent"))
            .unwrap()
            .status = SessionStatus::Inactive;
        rows.push(session("other", "Other task", SessionStatus::Working, 0));
        view.set_sessions(rows, false);
        set_filter(&mut view, 0);
        assert_eq!(ids(&view), ["other"]);
        key(&mut view, KeyCode::Home);
        assert_eq!(view.selected_id(), Some(&session_key("other")));
        assert!(!screen(&view, 120, 30).contains("Parent task"));
        set_filter(&mut view, 1);
        assert_eq!(ids(&view), ["parent", "agent-newer", "agent-older"]);
    }

    #[test]
    fn status_grouping_uses_root_status() {
        let mut view = fixture();
        view.grouping = Grouping::Status;
        assert_eq!(ids(&view), ["parent", "agent-newer", "agent-older"]);
        let entries = view.center_rows(&view.visible_indices(), view.grouping);
        assert_eq!(
            entries
                .iter()
                .filter(|entry| matches!(entry, navigation::CenterRow::Group(_)))
                .count(),
            1
        );
        let rendered = screen(&view, 120, 30);
        assert!(
            rendered
                .lines()
                .any(|line| line.split('│').next().unwrap().trim() == "Ready")
        );
    }

    #[test]
    fn grouping_and_pins_follow_parent() {
        let mut view = fixture();
        for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
            view.grouping = grouping;
            for pins in [vec![], vec![session_key("parent")]] {
                view.set_pinned(&pins);
                let expected = vec!["parent", "agent-newer", "agent-older"];
                assert_eq!(ids(&view), expected);
                let entries = view.center_rows(&view.visible_indices(), grouping);
                assert_eq!(
                    entries
                        .iter()
                        .filter(|r| matches!(r, navigation::CenterRow::Group(_)))
                        .count(),
                    1
                );
            }
        }
    }

    #[test]
    fn children_keep_parents_position_among_other_sessions() {
        let mut view = fixture();
        let mut other = session("other", "Other task", SessionStatus::Ready, 0);
        other.updated_at = view
            .rows
            .iter()
            .find(|s| s.id == session_key("parent"))
            .unwrap()
            .updated_at
            + Duration::seconds(30);
        other.model = Some("parent-model".into());
        let mut rows = view.rows.clone();
        rows.push(other);
        view.set_sessions(rows, false);
        for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
            view.grouping = grouping;
            let expected = vec!["other", "parent", "agent-newer", "agent-older"];
            assert_eq!(ids(&view), expected);
        }
        view.grouping = Grouping::Project;
        view.set_pinned(&[session_key("parent")]);
        assert_eq!(
            ids(&view),
            ["parent", "agent-newer", "agent-older", "other"]
        );
    }

    #[test]
    fn filtered_roots_keep_children_order_group_and_pin() {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        rows.iter_mut()
            .find(|row| row.id == session_key("parent"))
            .unwrap()
            .status = SessionStatus::Inactive;
        rows.push(session("other", "Other task", SessionStatus::Working, 0));
        view.set_sessions(rows, false);
        for grouping in [Grouping::Project, Grouping::Model] {
            view.grouping = grouping;
            set_filter(&mut view, 0);
            assert_eq!(ids(&view), ["other"]);
            view.set_pinned(&[session_key("parent")]);
            assert_eq!(ids(&view), ["other"]);
            set_filter(&mut view, 1);
            assert_eq!(ids(&view), ["parent", "agent-newer", "agent-older"]);
            assert!(screen(&view, 120, 30).contains("Pinned"));
        }
        view.hide(&session_key("agent-older"));
        assert_eq!(ids(&view), ["parent", "agent-newer"]);
    }

    #[test]
    fn keyboard_and_mouse_select_child_and_show_details() {
        let mut view = fixture();
        key(&mut view, KeyCode::Down);
        key(&mut view, KeyCode::Down);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        view.rows[view.selected].transcript = Some("/tmp/child.jsonl".into());
        let rendered = screen(&view, 120, 30);
        let layout = view.center_layout(Rect::new(0, 0, 120, 30));
        let header = rendered
            .lines()
            .nth(usize::from(layout.header.y + 1))
            .unwrap();
        assert!(header.contains("Check files"), "{header}");
        assert!(!header.contains("Working"), "{header}");
        let rendered = screen(&view, 120, 30);
        assert!(rendered.contains("child-model"));
        assert!(!rendered.contains("Needs attention"));
        let (x, y) = rendered_cell(&rendered, "Review results");
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        );
        assert_eq!(view.selected_id(), Some(&session_key("agent-newer")));
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, x, y));
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, x, y));
        assert_eq!(view.selected_id(), Some(&session_key("agent-newer")));
        key(&mut view, KeyCode::Down);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
    }

    #[test]
    fn search_uses_root_and_keeps_all_descendants() {
        let mut view = fixture();
        view.search.set_text("Check files");
        assert!(ids(&view).is_empty());
        view.search.set_text("Parent task");
        assert_eq!(ids(&view), ["parent", "agent-newer", "agent-older"]);
        let rendered = screen(&view, 120, 30);
        assert!(rendered.contains("Parent task"));
        assert!(rendered.contains("Check files"));
        assert!(rendered.contains("Review results"));
    }

    #[test]
    fn children_disappear_with_hidden_or_missing_parent() {
        let mut view = fixture();
        view.hide(&session_key("parent"));
        assert!(ids(&view).is_empty());
        view.hidden.clear();
        let rows = view
            .rows
            .iter()
            .filter(|s| s.parent.is_some())
            .cloned()
            .collect();
        view.set_sessions(rows, false);
        assert!(ids(&view).is_empty());
    }

    #[test]
    fn child_width_preserves_status_and_updated_columns() {
        let mut view = fixture();
        let child = view
            .rows
            .iter_mut()
            .find(|s| s.id == session_key("agent-older"))
            .unwrap();
        child.title = "界".repeat(100);
        for width in [12, 55, 57, 80] {
            let area = Rect::new(0, 0, width, 10);
            let mut buf = Buffer::empty(area);
            view.render_center_rows(area, &mut buf, "2026-10-13T12:00:00Z".parse().unwrap());
            let row = (0..area.height)
                .find(|&y| buf[(0, y)].symbol() == "└" && buf[(3, y)].symbol() == "●")
                .unwrap();
            assert_eq!(buf[(3, row)].symbol(), "●");
            assert_eq!(buf[(5, row)].symbol(), "界");
            if width >= 57 {
                let text: String = (0..width).map(|x| buf[(x, row)].symbol()).collect();
                assert!(text.contains("Working"), "{text}");
                assert!(text.contains("1m ago"), "{text}");
            }
        }
    }

    #[test]
    fn overview_snapshot() {
        let view = fixture();
        assert_eq!(
            screen(&view, 120, 30),
            include_str!("../../tests/ui_snapshots/agents_overview_subagents.txt")
        );
    }
}

mod subagent_actions {
    use super::*;

    fn fixture() -> View {
        let mut view = subagent_rows::fixture();
        key(&mut view, KeyCode::Down);
        key(&mut view, KeyCode::Down);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        view
    }

    fn assert_unchanged(view: &View, before: &str) {
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        assert!(view.rename_target.is_none());
        assert!(view.rename.text().is_empty());
        assert!(view.confirm_dialog.is_none());
        assert!(view.hidden.is_empty());
        assert!(view.pinned.is_empty());
        assert!(!view.searching);
        assert!(!view.help);
        assert_eq!(subagent_rows::screen(view, 120, 30), before);
    }

    macro_rules! unsupported_key {
        ($name:ident, $code:expr, $modifiers:expr, $label:literal) => {
            #[test]
            fn $name() {
                for notice in [None, Some("Keep this notice".to_owned())] {
                    let mut view = fixture();
                    view.set_notice(notice.clone());
                    let before = subagent_rows::screen(&view, 120, 30);
                    assert_eq!(
                        view.handle_key(KeyEvent::new($code, $modifiers)),
                        Some(Action::Unsupported($label))
                    );
                    assert_eq!(view.notice.clone().map(|(text, _)| text), notice);
                    assert_unchanged(&view, &before);
                }
            }
        };
    }

    unsupported_key!(
        control_l,
        KeyCode::Char('l'),
        KeyModifiers::CONTROL,
        "Resume"
    );
    unsupported_key!(rename, KeyCode::Char('r'), KeyModifiers::NONE, "Rename");
    unsupported_key!(fork, KeyCode::Char('f'), KeyModifiers::NONE, "Fork");
    unsupported_key!(stop, KeyCode::Char('x'), KeyModifiers::NONE, "Stop");
    unsupported_key!(delete, KeyCode::Backspace, KeyModifiers::NONE, "Delete");
    unsupported_key!(archive, KeyCode::Char('a'), KeyModifiers::NONE, "Archive");
    unsupported_key!(hide, KeyCode::Char('h'), KeyModifiers::NONE, "Hide");
    unsupported_key!(pin, KeyCode::Char('p'), KeyModifiers::NONE, "Pin");

    #[test]
    fn resume_emits_unsupported_notice() {
        for (code, name) in [
            (KeyCode::Enter, "Resume"),
            (KeyCode::Right, "Resume"),
            (KeyCode::Char('o'), "Terminal"),
        ] {
            let mut view = fixture();
            assert_eq!(key(&mut view, code), Some(Action::Unsupported(name)));
            assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        }
        let mut view = fixture();
        subagent_rows::screen(&view, 120, 30);
        let (x, y) = view.marker_pos().unwrap();
        let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(click_mouse(&mut view, click), None);
        assert_eq!(click_mouse(&mut view, click), None);
    }

    #[test]
    fn click() {
        let mut view = fixture();
        let before = subagent_rows::screen(&view, 120, 30);
        let (x, y) = view.marker_pos().unwrap();
        assert_eq!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, y)
            ),
            None
        );
        assert_unchanged(&view, &before);
    }

    #[test]
    fn activation_keeps_search() {
        for event in [
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
        ] {
            let mut view = fixture();
            view.searching = true;
            view.search.set_text("Parent task");
            let before = subagent_rows::screen(&view, 120, 30);
            assert_eq!(view.handle_key(event), Some(Action::Unsupported("Resume")));
            assert!(view.searching);
            assert_eq!(view.search.text(), "Parent task");
            assert_eq!(subagent_rows::screen(&view, 120, 30), before);
        }
    }

    #[test]
    fn footer_snapshot() {
        let view = fixture();
        assert_eq!(
            view.center_footer_hints().to_string(),
            "? help  esc back  ↑/↓ move  n new"
        );
        assert_eq!(
            subagent_rows::screen(&view, 120, 30),
            include_str!("../../tests/ui_snapshots/agents_overview_subagent_selected.txt")
        );
    }
}

#[test]
fn unsupported_provider_actions_do_not_open_dialogs_or_emit_actions() {
    let mut view = fixture();
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities::default(),
    )]));
    for (code, name) in [
        (KeyCode::Enter, "Resume"),
        (KeyCode::Right, "Resume"),
        (KeyCode::Char('o'), "Terminal"),
        (KeyCode::Char('f'), "Fork"),
        (KeyCode::Char('r'), "Rename"),
        (KeyCode::Backspace, "Delete"),
    ] {
        assert_eq!(key(&mut view, code), Some(Action::Unsupported(name)));
        assert!(view.rename_target.is_none());
        assert!(view.confirm_dialog.is_none());
    }
}

#[test]
fn unsupported_provider_mouse_activation_emits_notice() {
    let mut view = fixture();
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities::default(),
    )]));
    let rendered = screen(&view, 160, 40);
    let second = view.selectable_indices()[1];
    let (x, y) = rendered_cell(
        &rendered,
        &view.rows[second]
            .display_title()
            .chars()
            .take(12)
            .collect::<String>(),
    );
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    click_mouse(&mut view, click);
    assert_eq!(click_mouse(&mut view, click), None);
    assert!(!view.input_box.focused);
    key(&mut view, KeyCode::Esc);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Unsupported("Resume"))
    );
}

#[test]
fn missing_provider_capabilities_do_not_allow_actions() {
    let mut view = fixture();
    view.set_capabilities(HashMap::new());
    assert_eq!(
        key(&mut view, KeyCode::Char('o')),
        Some(Action::Unsupported("Terminal"))
    );
}

fn test_view(grouping: Grouping, use_colors: bool) -> View {
    let mut view = View::new(grouping, use_colors);
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            resume: true,
            fork: true,
            rename: true,
            delete: true,
            stop: false,
            archive: false,
        },
    )]));
    view
}

#[test]
fn ask_user_question_details_snapshot() {
    let now = Utc::now();
    let mut row = session("questions", "Question task", SessionStatus::NeedsInput, 0);
    let record: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/claude-questions-pending.jsonl"
    ))
    .unwrap();
    row.pending_questions = record["message"]["content"][0]["input"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| question["question"].as_str().unwrap().to_owned())
        .collect();
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 0;
    let render = |view: &View| {
        let area = Rect::new(0, 0, 160, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        buf.content
            .chunks(160)
            .map(|row| {
                let mut text = String::new();
                let mut cells = row.iter();
                while let Some(cell) = cells.next() {
                    text.push_str(cell.symbol());
                    for _ in 1..cell.symbol().width() {
                        cells.next();
                    }
                }
                text.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end_matches('\n')
            .to_owned()
    };
    view.set_sessions(vec![row.clone()], false);
    let pending = render(&view);
    assert!(
        pending.contains(&format!(
            "Needs attention\n{}",
            row.pending_questions.join("\n")
        )),
        "{pending}"
    );
    assert!(!pending.contains("Open task to review."));
    assert_eq!(
        pending,
        include_str!("../../tests/ui_snapshots/details_questions_pending.txt")
    );
    row.pending_questions.clear();
    view.set_sessions(vec![row.clone()], false);
    let answered = render(&view);
    assert!(answered.contains("Needs attention\nOpen task to review.\nWaiting for your response."));
    assert_eq!(
        answered,
        include_str!("../../tests/ui_snapshots/details_questions_answered.txt")
    );
    row.status = SessionStatus::Working;
    row.pending_questions.push("cd 要不要显示？".into());
    view.set_sessions(vec![row], false);
    assert!(!render(&view).contains("Needs attention"));
}

#[test]
fn pending_question_details_use_model_color_and_bold_attention_title() {
    let mut row = session("questions", "Question task", SessionStatus::NeedsInput, 0);
    row.pending_questions = vec!["Pick a color?".into()];
    row.activity = Some(crate::model::Activity {
        kind: crate::log::Kind::Assistant,
        label: "Pick a color?".into(),
        at: Utc::now(),
    });
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 0;
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 100, 40);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    let mut questions = 0;
    for cells in buf.content.chunks(100) {
        let text = cells.iter().map(|cell| cell.symbol()).collect::<String>();
        if text.trim_start().starts_with("Pick a color?") {
            questions += 1;
            assert_eq!(cells[0].fg, Color::Yellow);
        }
        if text.starts_with("Needs attention") {
            assert_eq!(cells[0].fg, Color::LightRed);
            assert!(cells[0].modifier.contains(ratatui::style::Modifier::BOLD));
        }
    }
    assert_eq!(questions, 2);
}

#[test]
fn pending_question_log_uses_model_color() {
    use crate::providers::Provider;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let codex = crate::providers::codex::CodexProvider::new(
        dir.path().to_owned(),
        dir.path().join("manager"),
    );
    for fixture in [
        "tests/fixtures/claude-questions-pending.jsonl",
        "tests/fixtures/codex-input/default-pending.jsonl",
        "tests/fixtures/codex-input/plan-pending.jsonl",
    ] {
        let (mut view, now) = log_fixture();
        let row = &mut view.rows[0];
        let path = root.join(fixture);
        row.transcript = Some(path.clone());
        let events = if fixture.contains("codex-input") {
            codex.log(row, 8, false).unwrap()
        } else {
            crate::providers::claude::log::tail(&path, 8).unwrap()
        };
        let question = events.last().unwrap();
        assert_eq!(style::log_styles(&question.kind).1.fg, Some(Color::Yellow));
        view.log = events;
        let area = Rect::new(0, 0, 160, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let viewport = view.viewport.borrow();
        let log = viewport.log;
        let y = log.y;
        let x = (log.x..log.right() - 10)
            .find(|&x| buf[(x, y)].symbol() != " ")
            .unwrap();
        assert_eq!(buf[(x, y)].fg, Color::Yellow);
    }
}

#[test]
fn attention_title_style_follows_terminal_background_and_color_support() {
    for light in [false, true] {
        for (no_color, colors) in [(false, true), (true, true), (false, false)] {
            let style = style::attention_title_style_for(no_color, colors, light);
            let expected = if no_color || !colors {
                None
            } else {
                Some(if light { Color::Red } else { Color::LightRed })
            };
            assert_eq!(style.fg, expected);
            assert!(style.add_modifier.contains(ratatui::style::Modifier::BOLD));
            assert_eq!(style.bg, None);
        }
    }
}

fn click_log_text(view: &mut View, screen: &str, text: &str, button: MouseButton) {
    let (x, y) = rendered_cell(screen, text);
    if button == MouseButton::Left {
        click_mouse(view, mouse(MouseEventKind::Down(button), x, y));
        view.flush_pending_click(Instant::now() + view.double_click_interval);
    } else {
        assert!(
            view.handle_mouse(mouse(MouseEventKind::Down(button), x, y))
                .is_none()
        );
        view.handle_mouse(mouse(MouseEventKind::Up(button), x, y));
    }
}

#[test]
fn task_log_click_keeps_list_and_detail_right_click_keeps_selection() {
    let (mut view, now) = log_fixture();
    view.log[6].body = "A long response with **bold** words whose body must be cut before the right edge of the log\n\nA second paragraph".into();
    let list = log_screen(&view, 100, 45, now);
    let log = view.viewport.borrow().log;
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            log.x + 30,
            log.y + 6,
        ),
    );
    assert_eq!(view.visible_log_event(), view.log.get(6));
    let selected = log_screen(&view, 100, 45, now);
    assert!(selected.contains("A second paragraph"));
    click_log_text(&mut view, &list, "A long response", MouseButton::Right);
    assert_eq!(view.visible_log_event(), view.log.last());
    click_log_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = log_screen(&view, 100, 45, now);
    assert_eq!(
        open,
        include_str!("../../tests/ui_snapshots/task_log_open.txt")
    );
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 45));
    view.render_at(buf.area, &mut buf, now);
    let (x, y) = rendered_cell(&open, "bold");
    assert!(
        buf[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert_eq!(
        view.log_redraw_in(now),
        Some(std::time::Duration::from_secs(1))
    );
}

fn detail_parts(event: &crate::log::Event) -> Vec<super::tool_detail::Part> {
    super::tool_detail::detail(event, String::new(), Style::new().dim())
        .sections
        .into_iter()
        .flat_map(|section| section.groups)
        .flat_map(|group| group.parts)
        .collect()
}

/// The calls of a merged Bash + Read row, the Read still running.
fn tool_calls() -> Vec<crate::log::ToolCall> {
    use crate::log::{Section, ToolCall};
    vec![
        ToolCall {
            detail_left: "Bash".into(),
            detail_title: "Bash".into(),
            show_details: false,
            id: "Bash-fixture".into(),
            title: "Bash".into(),
            name: "Bash".into(),
            input: vec![Section::Shell {
                prefix: 0,
                text: "cargo test".into(),
            }],
            output: Some(vec![Section::Text {
                label: "stdout".into(),
                text: "running 3 tests\n...".into(),
            }]),
            exit_code: None,
            is_error: false,
            duration: Some(chrono::Duration::seconds(12)),
        },
        ToolCall {
            detail_left: "Read".into(),
            detail_title: "Read".into(),
            show_details: false,
            id: "Read-fixture".into(),
            title: "Read".into(),
            name: "Read".into(),
            input: vec![Section::Text {
                label: "path".into(),
                text: "agents/src/log.rs:1-40".into(),
            }],
            output: None,
            exit_code: None,
            is_error: false,
            duration: None,
        },
    ]
}

/// The log fixture with its running "run tests" row carrying `tool_calls`, opened.
fn open_tool_fixture() -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = log_fixture();
    view.log[7].calls = tool_calls();
    view.select_fixture_event(Some(view.log[7].clone()));
    (view, now)
}

#[test]
fn task_log_click_opens_a_whole_row_that_has_calls() {
    let (mut view, now) = log_fixture();
    view.log[7].calls = tool_calls();
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "run tests", MouseButton::Left);
    assert_eq!(view.visible_log_event().cloned(), Some(view.log[7].clone()));
    assert!(log_screen(&view, 100, 25, now).contains("cargo test"));
}

#[test]
fn merged_tool_titles_copy_only_titles_and_arrows_center_on_panel() {
    for (width, title) in [
        (100, "Bash"),
        (99, "看探针结果"),
        (30, "  Read 界面  Read 界面  Read 界面"),
    ] {
        let (mut view, now) = open_tool_fixture();
        let mut event = view.log[7].clone();
        event.calls[0].title = title.into();
        event.calls[0].detail_title = title.into();
        view.select_fixture_event(Some(event));
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 48));
        view.render_at(buf.area, &mut buf, now);
        for title in [title, "Read"] {
            let (rows, body) = {
                let viewport = view.viewport.borrow();
                let region = &viewport.log_text;
                let source = region
                    .sources
                    .iter()
                    .find(|source| &*source.text == title)
                    .unwrap();
                (
                    region
                        .rows
                        .iter()
                        .filter(|row| row.segment == source.id && !row.cells.is_empty())
                        .cloned()
                        .collect::<Vec<_>>(),
                    region.area,
                )
            };
            assert_eq!(rows.len(), 1);
            let row = &rows[0];
            let start = row.cells.first().unwrap().0.start;
            let end = row.cells.last().unwrap().0.end;
            let panel = view.viewport.borrow().log_detail;
            assert_eq!(
                body.x + start,
                super::centering::centered_x(panel, body, end - start)
            );
            if start > 1 {
                assert_ne!(buf[(body.x + start - 2, row.y)].symbol(), "─");
            }
            if end < body.width - 1 {
                let last = " ";
                assert_eq!(buf[(body.right() - 1, row.y)].symbol(), last);
            }
            if title != "Read" {
                let viewport = view.viewport.borrow();
                assert_eq!(row.y, viewport.log_text.rows.first().unwrap().y);
            }
            if start > 0 {
                assert!(
                    buf[(body.x, row.y)]
                        .modifier
                        .contains(ratatui::style::Modifier::DIM)
                );
            }
            assert!(
                !buf[(body.x + start, row.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
            let arrow_x = super::centering::centered_x(panel, body, 1);
            let arrow = (body.y..body.bottom())
                .find(|y| buf[(arrow_x, *y)].symbol() == "↓")
                .expect("arrow centered on panel midline");
            assert!(
                (body.x..body.right())
                    .filter(|x| *x != arrow_x)
                    .all(|x| buf[(x, arrow)].symbol() == " ")
            );
            assert!(
                buf[(arrow_x, arrow)]
                    .modifier
                    .contains(ratatui::style::Modifier::BOLD)
            );
            let x1 = body.x + start;
            let x2 = body.x + end;
            view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x1, row.y));
            view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x2, row.y));
            view.render_at(buf.area, &mut buf, now);
            assert_eq!(
                view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x2, row.y)),
                Some(Action::Copy {
                    text: title.into(),
                    label: "Copied tool message".into()
                })
            );
        }
    }
}

#[test]
fn shell_commands_share_center_and_copy_original_without_connector_hits() {
    let command = "cat /workspace/a && printf longer && echo done; pwd\n";
    let (mut view, now) = open_tool_fixture();
    let mut event = view.log[7].clone();
    event.calls.truncate(1);
    event.calls[0].input = vec![
        crate::log::Section::Shell {
            prefix: 0,
            text: command.into(),
        },
        crate::log::Section::Json {
            label: "input".into(),
            text: r#"{"timeout":1000}"#.into(),
        },
    ];
    view.select_fixture_event(Some(event));
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 48));
    view.render_at(buf.area, &mut buf, now);
    let (body, rows) = {
        let viewport = view.viewport.borrow();
        let region = &viewport.log_text;
        let source = region
            .sources
            .iter()
            .find(|source| &*source.text == command)
            .unwrap();
        let rows: Vec<_> = region
            .rows
            .iter()
            .filter(|row| row.segment == source.id)
            .cloned()
            .collect();
        assert_eq!(rows.len(), 4);
        let offset = super::centering::centered_x(
            viewport.log_detail,
            region.area,
            ("cat /workspace/a".width() + 2) as u16,
        ) - region.area.x;
        assert!(
            rows.iter()
                .all(|row| row.cells.first().unwrap().0.start == offset + 2)
        );
        let fields = region
            .sources
            .iter()
            .find(|source| source.text.contains("\"timeout\": 1000"))
            .unwrap();
        let fields_row = region
            .rows
            .iter()
            .find(|row| row.segment == fields.id)
            .unwrap();
        assert_eq!(fields_row.cells.first().unwrap().0.start, offset);
        // Shell display prefixes occupy the group origin without selectable bytes.
        let prefix_x = region.area.x + offset;
        for (row, glyph) in rows.iter().zip(["┌", "│", "└", " "]) {
            assert_eq!(buf[(prefix_x, row.y)].symbol(), glyph);
            assert_eq!(buf[(prefix_x + 1, row.y)].symbol(), " ");
            // Muted like the time labels: the default foreground dimmed, not the tool accent.
            assert_eq!(buf[(prefix_x, row.y)].fg, ratatui::style::Color::Reset);
            assert!(
                buf[(prefix_x, row.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
        }
        assert!(
            !viewport
                .log_links
                .iter()
                .any(|link| link.0.x <= prefix_x + 1)
        );
        (region.area, rows)
    };
    for row in &rows {
        for prefix_cell in 1..=2 {
            view.begin_text_selection(body.x + row.cells[0].0.start - prefix_cell, row.y);
            assert!(view.text_selection.borrow().is_none());
        }
    }
    let first = &rows[0];
    let last = rows.last().unwrap();
    let x1 = body.x + first.cells[0].0.start;
    let x2 = body.right() - 1;
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x1, first.y));
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x2, last.y));
    view.render_at(buf.area, &mut buf, now);
    // A drag covers visible bytes; a whole-section copy also retains its trailing newline.
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x2, last.y)),
        Some(Action::Copy {
            text: command.trim_end_matches('\n').into(),
            label: "Copied tool message".into(),
        })
    );
    let full_screen = log_screen(&view, 100, 48, now);
    assert_eq!(
        double_click_text(&mut view, &full_screen, "printf longer"),
        Some(Action::Copy {
            text: format!("{command}\n{{\n  \"timeout\": 1000\n}}\n\nrunning 3 tests\n..."),
            label: "Copied tool message".into(),
        })
    );
    let screen = full_screen
        .lines()
        .skip(usize::from(body.y))
        .take(usize::from(body.height))
        .collect::<Vec<_>>()
        .join("\n");
    let screen = screen.trim_end_matches('\n').to_owned() + "\n";
    assert_eq!(
        screen,
        include_str!("../../tests/ui_snapshots/task_log_shell_commands.txt")
    );
}

#[test]
fn running_tool_redraws_cached_detail_only_while_visible() {
    let (view, _) = open_tool_fixture();
    let start = chrono::DateTime::UNIX_EPOCH;
    for (millis, frame) in [(0, "•··"), (300, "·•·"), (600, "··•"), (900, "•··")] {
        let now = start + Duration::milliseconds(millis);
        let screen = log_screen(&view, 100, 48, now);
        assert!(screen.lines().any(|line| line.trim() == frame));
        let viewport = view.viewport.borrow();
        let region = &viewport.log_text;
        let row = region.rows.last().unwrap();
        assert!(row.cells.is_empty());
        let text = screen.lines().nth(usize::from(row.y)).unwrap();
        assert_eq!(
            text.find(frame),
            Some(usize::from(super::centering::centered_x(
                viewport.log_detail,
                region.area,
                frame.width() as u16,
            )))
        );
        drop(viewport);
        assert_eq!(
            view.tool_redraw_in(now),
            Some(std::time::Duration::from_millis(300))
        );
    }
    assert_eq!(
        view.tool_redraw_in(start + Duration::milliseconds(299)),
        Some(std::time::Duration::from_millis(1))
    );
    log_screen(&view, 100, 25, start);
    assert_eq!(
        view.tool_redraw_in(start),
        None,
        "running row is below the viewport"
    );
    let (view, _) = log_fixture();
    log_screen(&view, 100, 48, start);
    assert_eq!(
        view.tool_redraw_in(start),
        None,
        "plain tool rows do not animate"
    );
}

#[test]
fn task_log_selected_tool_snapshot() {
    let (view, now) = open_tool_fixture();
    let open = log_screen(&view, 100, 48, now);
    assert_eq!(
        open,
        include_str!("../../tests/ui_snapshots/task_log_open_tool.txt")
    );
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 48));
    view.render_at(buf.area, &mut buf, now);
    let (x, y) = rendered_cell(&open, "cargo test");
    assert!(matches!(
        buf[(x, y)].fg,
        Color::Rgb(..) | Color::Indexed(..)
    ));
    let (x, y) = rendered_cell(&open, super::tool_detail::running_frame(now));
    assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::DIM));
    assert_eq!(
        super::log::open_text(&view.log[7]),
        "Bash\ncargo test\n\nrunning 3 tests\n...\n\nRead\nagents/src/log.rs:1-40"
    );
}

#[test]
fn task_log_selected_tool_highlighting_refreshes_when_theme_changes() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let events =
        crate::providers::claude::log::tail(&root.join("claude-tool-calls.jsonl"), 100).unwrap();
    let event = events
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Bash"))
        .unwrap();
    let (mut view, now) = log_fixture();
    view.select_fixture_event(Some(event));
    let render = || {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 48));
        view.render_at(buf.area, &mut buf, now);
        let area = view.viewport.borrow().log_text.area;
        let mut cells = Vec::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                cells.push((buf[(x, y)].symbol().to_owned(), buf[(x, y)].fg));
            }
        }
        cells
    };
    super::highlight::use_theme(false, None).unwrap();
    let dark = render();
    super::highlight::use_theme(true, None).unwrap();
    let light = render();
    super::highlight::use_theme(false, None).unwrap();
    let dark_again = render();
    assert_eq!(
        dark.iter().map(|cell| &cell.0).collect::<Vec<_>>(),
        light.iter().map(|cell| &cell.0).collect::<Vec<_>>()
    );
    assert!(dark.iter().zip(&light).any(|(a, b)| a.1 != b.1));
    assert_eq!(dark, dark_again);
}

#[test]
fn task_log_selected_tool_layout_is_cached_until_selection_or_width_changes() {
    let (mut view, now) = open_tool_fixture();
    assert!(view.viewport.borrow().log_detail_layout.is_none());
    let open = log_screen(&view, 100, 28, now);
    assert!(view.viewport.borrow().log_detail_layout.is_some());
    assert_eq!(log_screen(&view, 100, 28, now), open);
    let narrow = log_screen(&view, 60, 28, now);
    assert_ne!(narrow, open);
    assert!(narrow.contains("running 3 tests"));
    view.select_fixture_event(Some(view.log[6].clone()));
    assert!(view.viewport.borrow().log_detail_layout.is_none());
    assert_eq!(view.visible_log_event(), view.log.get(6));
}

#[test]
fn task_log_selected_tool_gains_its_output_when_the_log_refreshes() {
    let (mut view, now) = open_tool_fixture();
    let open = log_screen(&view, 100, 48, now);
    let mut events = view.log.clone();
    events[7].calls[1].output = Some(vec![crate::log::Section::Text {
        label: "content".into(),
        text: "use chrono::{DateTime, Utc};".into(),
    }]);
    view.set_log(events.clone());
    assert_eq!(view.visible_log_event().cloned(), Some(events[7].clone()));
    let refreshed = log_screen(&view, 100, 48, now);
    assert_ne!(refreshed, open);
    assert!(refreshed.contains("use chrono::{DateTime, Utc};"));
    assert!(!refreshed.contains(super::tool_detail::running_frame(now)));
    view.set_log(view.log[..7].to_vec());
    assert_eq!(view.visible_log_event(), view.log.last());
}

#[test]
fn task_log_selected_tool_copies_a_dragged_section() {
    let (mut view, now) = open_tool_fixture();
    let open = log_screen(&view, 100, 28, now);
    let (x1, y1) = rendered_cell(&open, "cargo test");
    let (x2, y2) = rendered_cell(&open, "running 3 tests");
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x1, y1))
            .is_none()
    );
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x2 + 7, y2));
    log_screen(&view, 100, 28, now);
    let action = view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x2 + 7, y2));
    assert_eq!(
        action,
        Some(Action::Copy {
            text: "cargo test\n\nrunning".into(),
            label: "Copied selection".into()
        })
    );
}

#[test]
fn task_log_selected_event_header_centers_kind_and_keeps_time_right() {
    for body in ["A long response", "The newest reply"] {
        let (mut view, now) = log_fixture();
        // The newest row reads its time as "30s ago".
        view.log[7].kind = crate::log::Kind::Assistant;
        view.log[7].body = format!("The newest reply {}", "runs long ".repeat(20));
        let list = log_screen(&view, 100, 25, now);
        let row = list.lines().find(|line| line.contains(body)).unwrap();
        let time = row.chars().skip(91).take(8).collect::<String>();
        let time = time.trim();
        click_log_text(&mut view, &list, body, MouseButton::Left);
        let open = log_screen(&view, 100, 25, now);
        let detail = view.viewport.borrow().log_detail;
        let columns = super::detail_model::detail_columns(
            detail,
            view.center_layout(Rect::new(0, 0, 100, 25)).log,
        );
        let header = open.lines().nth(usize::from(detail.y + 1)).unwrap();
        assert_eq!(
            header.find("Assistant"),
            Some(usize::from(super::centering::centered_x(
                columns.panel,
                columns.text,
                "Assistant".len() as u16
            )))
        );
        assert_eq!(
            header.find(time),
            Some(usize::from(columns.time.right()) - time.len())
        );
    }
}

#[test]
fn task_log_click_opens_row_with_shortened_path() {
    let (mut view, now) = log_fixture();
    let path = "/private/workspace/a-very-long-directory-name/another-long-directory/and-one-more-level/report.md";
    let event = &mut view.log[7];
    event.kind = crate::log::Kind::Error;
    event
        .paths
        .push(event.body.len() + 1..event.body.len() + 1 + path.len());
    event.body = format!("{} {path}", event.body);
    view.set_log(view.log.clone());
    let list = log_list_screen(&view, 100, 25, now);
    assert!(!list.contains(path), "{list}");
    click_log_text(&mut view, &list, "run tests", MouseButton::Left);
    let open = log_screen(&view, 100, 25, now);
    // The open event wraps the full path within the message-column width.
    assert!(
        open.contains("/private/workspace/a-very-long-directory-name"),
        "{open}"
    );
    assert!(open.contains("report.md"), "{open}");
    assert!(
        view.viewport
            .borrow()
            .log_text
            .sources
            .iter()
            .any(|source| source.text.contains(path))
    );
}

fn click_text(view: &mut View, screen: &str, text: &str) -> Option<Action> {
    let (x, y) = rendered_cell(screen, text);
    let action = click_mouse(view, mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    action.or_else(|| view.flush_pending_click(Instant::now() + view.double_click_interval))
}

#[test]
fn missing_rooted_paths_stay_shortened_without_links_on_log_and_details() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let missing = dir
        .path()
        .join("a-very-long-directory-name/another-long-directory/report.md");
    for path in [missing.to_str().unwrap(), "~/C…/.w…/a…/s…/l…/plan.md"] {
        let filename = path.rsplit('/').next().unwrap();
        for kind in [crate::log::Kind::Assistant, crate::log::Kind::Tool] {
            let (mut view, now) = log_fixture();
            view.log[7].kind = kind;
            view.log[7].body = format!("Report: {path}");
            view.log[7].paths.clear();
            view.log[7].paths.push(8..8 + path.len());
            let list = log_screen(&view, 100, 25, now);
            assert!(list.contains(filename), "{list}");
            assert!(view.viewport.borrow().log_links.is_empty());
            assert_eq!(click_text(&mut view, &list, filename), None);
            view.select_fixture_event(Some(view.log[7].clone()));
            log_screen(&view, 100, 25, now);
            assert!(view.viewport.borrow().log_links.is_empty());
        }
        let mut view = fixture();
        view.rows[view.selected].prompt = format!("Report: {path}");
        let rendered = screen(&view, 120, 40);
        assert!(rendered.contains(filename), "{rendered}");
        if path == missing.to_str().unwrap() {
            assert!(!rendered.contains(path), "{rendered}");
        }
        let (x, y) = rendered_cell(&rendered, filename);
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
        view.render_at(buf.area, &mut buf, Utc::now());
        assert!(
            !buf[(x, y)]
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        );
        assert_eq!(click_text(&mut view, &rendered, filename), None);
    }
}

fn temp_log_path(filename: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let target = dir
        .path()
        .join("a-very-long-directory-name/another-long-directory/and-one-more-level")
        .join(filename);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "").unwrap();
    (dir, target)
}

#[test]
fn task_log_click_on_a_shortened_tool_path_opens_the_full_path() {
    let (mut view, now) = log_fixture();
    let (_dir, target) = temp_log_path("report.md");
    let path = target.to_str().unwrap();
    let event = &mut view.log[7];
    event
        .paths
        .push(event.body.len() + 1..event.body.len() + 1 + path.len());
    event.body = format!("{} {path}", event.body);
    let list = log_list_screen(&view, 100, 25, now);
    assert!(!list.contains(path), "{list}");
    assert_eq!(
        click_text(&mut view, &list, "report.md"),
        Some(Action::OpenPath(path.into()))
    );
    assert_eq!(view.visible_log_event(), view.log.get(7));
    let (x, y) = rendered_cell(&list, "report.md");
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(buf.area, &mut buf, now);
    assert!(
        !buf[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    );
    let (x, y) = rendered_cell(&list, "run tests");
    assert!(
        !buf[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    );
    assert_eq!(click_text(&mut view, &list, "run tests"), None);
    assert!(view.log_selected.is_some());
}

#[test]
fn task_log_click_on_a_path_in_free_tool_text_opens_the_full_path() {
    let (mut view, now) = log_fixture();
    let (_dir, target) = temp_log_path("plan.md");
    let path = target.to_str().unwrap();
    view.log[7].body = format!("start Implement workstream A of {path} (Claude provider)");
    let list = log_list_screen(&view, 100, 25, now);
    assert!(!list.contains(path), "{list}");
    assert_eq!(
        click_text(&mut view, &list, "plan.md"),
        Some(Action::OpenPath(path.into()))
    );
}

#[test]
fn task_log_click_on_an_image_placeholder_opens_the_image() {
    let (mut view, now) = log_fixture();
    view.log[0].body = "Please look at [Image #1] and [Image #2]".into();
    view.log[0].images = vec![crate::log::Image {
        label: "[Image #2]".into(),
        path: "/tmp/images/2.png".into(),
    }];
    let list = log_screen(&view, 100, 40, now);
    assert!(list.contains("[Image #1] and [Image #2]"), "{list}");
    assert_eq!(click_text(&mut view, &list, "[Image #1]"), None);
    assert_eq!(
        click_text(&mut view, &list, "[Image #2]"),
        Some(Action::OpenPath("/tmp/images/2.png".into()))
    );
}

#[test]
fn open_log_event_links_its_wrapped_path() {
    let (mut view, now) = log_fixture();
    let (_dir, target) = temp_log_path("report.md");
    let path = target.to_str().unwrap();
    view.log[6].body = format!(
        "A long response with **bold** words whose body must be cut before the right edge of the log, see {path} for details"
    );
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = log_screen(&view, 100, 25, now);
    assert!(open.contains("another-long-directory"), "{open}");
    assert_eq!(
        click_text(&mut view, &open, "another-long-directory"),
        Some(Action::OpenPath(path.into()))
    );
    assert_eq!(click_text(&mut view, &open, "details"), None);
}

#[test]
fn details_prompt_links_its_shortened_path_and_image_placeholder() {
    let mut view = fixture();
    let (_dir, target) = temp_log_path("report.md");
    let path = target.to_str().unwrap();
    let selected = &mut view.rows[view.selected];
    selected.prompt = format!("Look at [Image #1] and the report in {path} please");
    selected.prompt_images = vec![crate::log::Image {
        label: "[Image #1]".into(),
        path: "/tmp/images/1.png".into(),
    }];
    let rendered = screen(&view, 120, 40);
    assert!(!rendered.contains(path), "{rendered}");
    assert!(rendered.contains("report.md"), "{rendered}");
    assert_eq!(
        click_text(&mut view, &rendered, "report.md"),
        Some(Action::OpenPath(path.into()))
    );
    assert_eq!(
        click_text(&mut view, &rendered, "[Image #1]"),
        Some(Action::OpenPath("/tmp/images/1.png".into()))
    );
    assert_eq!(click_text(&mut view, &rendered, "Look at"), None);
    assert_eq!(
        double_click_text(&mut view, &rendered, "Look at"),
        Some(Action::Copy {
            text: view.rows[view.selected].prompt.clone(),
            label: "Copied prompt".into()
        })
    );
}

#[test]
fn details_relative_paths_link_only_when_they_exist() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "").unwrap();
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.cwd = dir.path().to_owned();
    selected.prompt = "Fix src/main.rs and/or src/missing.rs".into();
    let rendered = screen(&view, 120, 40);
    assert_eq!(
        click_text(&mut view, &rendered, "src/main.rs"),
        Some(Action::OpenPath(dir.path().join("src/main.rs")))
    );
    assert_eq!(click_text(&mut view, &rendered, "and/or"), None);
    view.last_copy_click = None;
    assert_eq!(click_text(&mut view, &rendered, "src/missing.rs"), None);
}

#[test]
fn details_project_lines_link_the_pull_request_and_the_directory() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let todo = dir.path().join("notes/todo.md");
    std::fs::create_dir_all(todo.parent().unwrap()).unwrap();
    std::fs::write(&todo, "").unwrap();
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.cwd = dir.path().to_owned();
    selected.git_branch = Some("main".into());
    selected.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    selected.queued = vec![format!("Then read {}", todo.display())];
    let rendered = screen(&view, 120, 40);
    let (rect, _) = view
        .viewport
        .borrow()
        .details_links
        .iter()
        .find(|(_, target)| target.to_str().unwrap().starts_with("vscode://"))
        .unwrap()
        .clone();
    let (x, y) = (rect.x, rect.y);
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y)
        ),
        Some(Action::OpenPath(
            super::vscode::pull_request_link(
                "https://github.com/0xbabedead/chadwallet/pull/833",
                dir.path()
            )
            .unwrap()
            .into()
        ))
    );
    assert!(!rendered.contains("main"));
    // The folder link stays on the status rule.
    let (rect, _) = view
        .viewport
        .borrow()
        .details_links
        .iter()
        .find(|(_, target)| {
            !target.to_str().unwrap().starts_with("vscode://")
                && !target.to_str().unwrap().ends_with("todo.md")
        })
        .unwrap()
        .clone();
    let (x, y) = (rect.x, rect.y - 1);
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x + 3, y + 1)
        ),
        Some(Action::OpenPath(dir.path().to_owned()))
    );
    assert_eq!(
        click_text(&mut view, &rendered, "todo.md"),
        Some(Action::OpenPath(todo))
    );
    assert_eq!(click_text(&mut view, &rendered, "Then read"), None);
}

#[test]
fn links_of_a_panel_left_out_of_the_frame_do_not_linger() {
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    screen(&view, 120, 40);
    let (rect, _) = view
        .viewport
        .borrow()
        .details_links
        .iter()
        .find(|(_, target)| {
            !target.to_str().unwrap().starts_with("vscode://")
                && !target.to_str().unwrap().ends_with("todo.md")
        })
        .unwrap()
        .clone();
    let (x, y) = (rect.x, rect.y - 1);
    let cwd_cell = mouse(MouseEventKind::Down(MouseButton::Left), x + 3, y + 1);
    assert_eq!(
        click_mouse(&mut view, cwd_cell),
        Some(Action::OpenPath("/tmp/project".into()))
    );
    // Under 90 columns the details panel is gone and the list spans its cells.
    screen(&view, 80, 40);
    assert_eq!(click_mouse(&mut view, cwd_cell), None);
}

#[test]
fn project_group_header_opens_its_directory() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let (x, y) = rendered_cell(&rendered, "/tmp/project  ");
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y)
        ),
        None
    );
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        Some(Action::OpenPath("/tmp/project".into()))
    );
    let selected = view.selected;
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x + 14, y)
        ),
        None
    );
    assert_eq!(view.selected, selected);
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    view.render_at(buf.area, &mut buf, Utc::now());
    assert!(
        !buf[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    );
    assert!(
        !buf[(x + 14, y)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    );
}

#[test]
fn task_log_selected_event_closes_when_selection_changes() {
    let (mut view, now) = log_fixture();
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "A long response", MouseButton::Left);
    assert!(view.log_selected.is_some());
    let mut other = session("other", "Other task", SessionStatus::Working, 1);
    other.transcript = Some("/tmp/other.jsonl".into());
    let mut rows = view.rows.clone();
    rows.push(other);
    view.set_sessions(rows, false);
    view.set_selected(
        view.rows
            .iter()
            .position(|row| row.id.id == "other")
            .unwrap(),
    );
    assert!(view.log_selected.is_none());
}

fn double_click_text(view: &mut View, screen: &str, text: &str) -> Option<Action> {
    let (x, y) = rendered_cell(screen, text);
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(click_mouse(view, click).is_none());
    click_mouse(view, click)
}

#[test]
fn double_click_on_open_log_body_copies_the_whole_event() {
    let (mut view, now) = log_fixture();
    let body = "A long response with **bold** words whose body must be cut before the right edge of the log\n\nA second paragraph";
    view.log[6].body = body.into();
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = log_screen(&view, 100, 25, now);
    assert_eq!(
        double_click_text(&mut view, &open, "A second paragraph"),
        Some(Action::Copy {
            text: body.into(),
            label: "Copied assistant message".into()
        })
    );
    assert_eq!(
        double_click_text(&mut view, &open, "Assistant"),
        Some(Action::Copy {
            text: body.into(),
            label: "Copied assistant message".into()
        })
    );
}

#[test]
fn log_detail_left_click_has_no_deadline() {
    let (mut view, now) = log_fixture();
    log_screen(&view, 100, 45, now);
    let area = view.viewport.borrow().log_detail;
    let (x, y) = (area.right() - 2, area.y + 4);
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y)
        ),
        None
    );
    assert_eq!(view.pending_click_in(Instant::now()), None);
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        None
    );
}

#[test]
fn double_click_log_detail_copies_the_message() {
    let (mut view, now) = log_fixture();
    view.log[6].body = "A long response with **bold** words whose body must be cut before the right edge of the log\n\nA second paragraph".into();
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = log_screen(&view, 100, 25, now);
    assert_eq!(
        double_click_text(&mut view, &open, "A second paragraph"),
        Some(Action::Copy {
            text: view.log[6].body.clone(),
            label: "Copied assistant message".into(),
        }),
    );
}

#[test]
fn double_click_on_details_prompt_or_last_message_copies_it() {
    let mut view = overflowing_details();
    let message = "First reply row\n\nSecond reply row";
    view.rows[view.selected].last_message = Some(message.into());
    let prompt = view.rows[view.selected].prompt.clone();
    let now = view.rows[view.selected].updated_at;
    let screen = log_screen(&view, 120, 40, now);
    assert_eq!(
        double_click_text(&mut view, &screen, "First reply row"),
        Some(Action::Copy {
            text: message.into(),
            label: "Copied update".into()
        })
    );
    assert_eq!(
        double_click_text(&mut view, &screen, "A long prompt"),
        Some(Action::Copy {
            text: prompt,
            label: "Copied prompt".into()
        })
    );
    assert_eq!(double_click_text(&mut view, &screen, "Update"), None);
}

#[test]
fn clicks_on_two_bodies_are_not_a_double_click() {
    let mut view = overflowing_details();
    let now = view.rows[view.selected].updated_at;
    let screen = log_screen(&view, 120, 40, now);
    for text in ["A long prompt", "First reply row"] {
        let (x, y) = rendered_cell(&screen, text);
        let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert!(click_mouse(&mut view, click).is_none());
    }
}

#[test]
fn copy_confirmation_sits_at_footer_right_for_one_second() {
    let (mut view, now) = log_fixture();
    let start = Instant::now();
    let hints = log_screen(&view, 100, 25, now)
        .lines()
        .last()
        .unwrap()
        .to_owned();
    view.show_copied("Copied prompt".into(), start);
    let footer = log_screen(&view, 100, 25, now)
        .lines()
        .last()
        .unwrap()
        .to_owned();
    assert_eq!(footer, format!("{hints:<86}Copied prompt"));
    assert_eq!(
        view.expire_messages(start + std::time::Duration::from_millis(400)),
        Some(std::time::Duration::from_millis(600))
    );
    assert_eq!(
        view.expire_messages(start + std::time::Duration::from_secs(1)),
        None
    );
    assert_eq!(
        log_screen(&view, 100, 25, now).lines().last().unwrap(),
        hints
    );
}

#[test]
fn notice_sits_at_task_list_bottom_left_for_one_second() {
    let (mut view, now) = log_fixture();
    let area = Rect::new(0, 0, 100, 25);
    let list = view.center_layout(area).list;
    let hints = log_screen(&view, 100, 25, now)
        .lines()
        .last()
        .unwrap()
        .to_owned();
    // Keep logical time behind wall time so mixing the clocks fails deterministically.
    let start = Instant::now() - std::time::Duration::from_secs(2);
    view.set_notice_at(Some("Resume is not supported".into()), start);
    let rendered = log_screen(&view, 100, 25, now);
    let lines = rendered.lines().collect::<Vec<_>>();
    let notice_row = lines[usize::from(list.bottom() - 1)];
    let notice_cells = notice_row
        .chars()
        .skip(usize::from(list.x))
        .take(usize::from(list.width))
        .collect::<String>();
    assert_eq!(
        notice_cells.trim_end(),
        "Resume is not supported",
        "{rendered}"
    );
    assert_eq!(*lines.last().unwrap(), hints);
    assert_eq!(
        view.expire_messages(start),
        Some(std::time::Duration::from_secs(1))
    );
    assert_eq!(
        view.expire_messages(start + std::time::Duration::from_millis(400)),
        Some(std::time::Duration::from_millis(600))
    );
    assert_eq!(
        view.expire_messages(start + std::time::Duration::from_secs(1)),
        None
    );
    assert!(!log_screen(&view, 100, 25, now).contains("Resume is not supported"));
}

#[test]
fn review_refresh_cancels_open_selection() {
    let (mut view, now) = open_tool_fixture();
    let open = log_screen(&view, 100, 48, now);
    let (x, y) = rendered_cell(&open, "cargo test");
    view.begin_text_selection(x, y);
    view.extend_text_selection(x + 4, y);
    let mut events = view.log.clone();
    events[7].calls[1].output = Some(vec![]);
    view.set_log(events);
    assert!(!view.text_selection_dragging());
    assert!(view.finish_text_selection(x + 4, y).is_none());
}

#[test]
fn review_running_call_refreshes_to_error_or_denied() {
    for kind in [crate::log::Kind::Error, crate::log::Kind::Denied] {
        let (mut view, now) = open_tool_fixture();
        log_screen(&view, 100, 48, now);
        let mut events = view.log.clone();
        events[7].kind = kind.clone();
        events[7].calls[1].output = Some(vec![]);
        view.set_log(events);
        assert_eq!(view.visible_log_event().unwrap().kind, kind);
    }
}

#[test]
fn review_cached_layout_shares_source_bytes() {
    let (view, now) = open_tool_fixture();
    log_screen(&view, 100, 48, now);
    let sources = view.viewport.borrow().log_text.sources.clone();
    log_screen(&view, 100, 48, now);
    let viewport = view.viewport.borrow();
    for (cached, drawn) in sources.iter().zip(&viewport.log_text.sources) {
        assert_eq!(cached.text.as_ptr(), drawn.text.as_ptr());
        assert_eq!(drawn.text.as_ptr(), drawn.clone().text.as_ptr());
    }
}

#[test]
fn open_tool_layout_titles_and_arrows() {
    let (mut view, now) = open_tool_fixture();
    view.log[7].calls.truncate(1);
    view.log[7].calls[0].title = "Run tests".into();
    view.log[7].calls[0].detail_title = "Run tests".into();
    view.select_fixture_event(Some(view.log[7].clone()));
    let open = log_screen(&view, 100, 48, now);
    assert!(
        open.lines()
            .any(|line| line.trim_start().starts_with("Bash")
                && line.contains("Run tests")
                && line.ends_with("35s")),
        "{open}"
    );
    assert!(
        !open
            .lines()
            .any(|line| matches!(line.trim(), "Bash" | "command" | "stdout"))
    );
    assert!(open.lines().any(|line| line.trim() == "↓"));
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 48));
    view.render_at(buf.area, &mut buf, now);
    let (x, y) = rendered_cell(&open, "↓");
    let area = view.viewport.borrow().log_text.area;
    assert_eq!(
        x,
        super::centering::centered_x(view.viewport.borrow().log_detail, area, 1)
    );
    assert_eq!(buf[(x, y)].fg, Color::Reset);
    assert!(
        buf[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(!buf[(x, y)].modifier.contains(ratatui::style::Modifier::DIM));
}

#[test]
fn open_json_body_is_pretty_for_every_kind() {
    for kind in [
        crate::log::Kind::Tool,
        crate::log::Kind::Error,
        crate::log::Kind::Denied,
        crate::log::Kind::Assistant,
        crate::log::Kind::User,
        crate::log::Kind::Agent,
        crate::log::Kind::Session,
        crate::log::Kind::QueuedUser,
        crate::log::Kind::QueuedSystem,
    ] {
        let (mut view, now) = log_fixture();
        let mut event = view.log[7].clone();
        event.kind = kind;
        event.body = "{\"answer\":[1,2]}".into();
        event.calls.clear();
        if event.kind == crate::log::Kind::Tool {
            assert!(super::log::open_text(&event).is_empty());
            continue;
        }
        view.select_fixture_event(Some(event));
        let open = log_screen(&view, 100, 48, now);
        assert!(open.contains("\"answer\": ["), "{open}");
        assert!(open.lines().any(|line| line.trim() == "1,"));
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 48));
        view.render_at(buf.area, &mut buf, now);
        let (x, y) = rendered_cell(&open, "\"answer\": [");
        assert!(matches!(
            buf[(x, y)].fg,
            Color::Rgb(..) | Color::Indexed(..)
        ));
    }
}

#[test]
fn open_view_scrollbar() {
    let (mut view, now) = log_fixture();
    let mut event = view.log[1].clone();
    event.body = (0..80)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    view.select_fixture_event(Some(event));
    log_screen(&view, 100, 25, now);
    let area = view.viewport.borrow().log_text.area;
    let layout = view.center_layout(Rect::new(0, 0, 100, 25));
    assert_eq!(area.right() + 11, layout.log_detail.right());
    let scrollbar_x = layout.log_detail.right() - 1;
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(buf.area, &mut buf, now);
    assert_eq!(buf[(scrollbar_x, area.top())].symbol(), "│");
    assert_eq!(buf[(scrollbar_x, area.top())].fg, Color::Reset);
    assert_eq!(buf[(scrollbar_x, area.bottom() - 1)].symbol(), " ");
    for y in area.top()..area.bottom() {
        assert_ne!(buf[(area.right() - 1, y)].symbol(), "│");
    }
    view.viewport.borrow_mut().log_detail_scroll = usize::MAX;
    log_screen(&view, 100, 25, now);
    view.render_at(buf.area, &mut buf, now);
    assert_eq!(buf[(scrollbar_x, area.bottom() - 1)].symbol(), "│");
}

#[test]
fn real_tool_headers_show_description_agent_skill_and_recipient() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let calls =
        crate::providers::claude::log::tail(&root.join("claude-tool-calls.jsonl"), 100).unwrap();
    let messages = crate::providers::claude::log::tail_session(
        &root.join("claude-tool-details.jsonl"),
        100,
        true,
    )
    .unwrap();
    for name in ["Bash", "Agent", "Skill", "SendMessage"] {
        let event = calls
            .iter()
            .chain(&messages)
            .find(|event| event.calls.iter().any(|call| call.name == name))
            .unwrap();
        let call = event.calls.iter().find(|call| call.name == name).unwrap();
        let mut event = event.clone();
        event.calls = vec![call.clone()];
        let title = call.detail_title.clone();
        let left = call.detail_left.clone();
        for kind in [
            crate::log::Kind::Tool,
            crate::log::Kind::Error,
            crate::log::Kind::Denied,
        ] {
            event.kind = kind;
            let (mut view, now) = log_fixture();
            view.select_fixture_event(Some(event.clone()));
            let open = log_screen(&view, 120, 48, now);
            let header = open
                .lines()
                .nth(usize::from(view.viewport.borrow().log_detail.y + 1))
                .unwrap();
            assert!(header.contains(&left), "{name}: {open}");
            assert!(
                header.replace(' ', "").contains(&title.replace(' ', "")),
                "{name}: {open}"
            );
            assert!(!header.contains('─'), "{header}");
        }
    }
}

#[test]
fn real_tabs_survive_copy_and_expanded_cell_selection() {
    let record: serde_json::Value = serde_json::from_str(
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .nth(3)
            .unwrap(),
    )
    .unwrap();
    let source = record["message"]["content"][0]["content"]
        .as_str()
        .unwrap()
        .lines()
        .nth(1)
        .unwrap();
    assert!(source.contains('\t'));
    let (mut view, now) = open_tool_fixture();
    let mut event = view.log[7].clone();
    event.calls.truncate(1);
    event.calls[0].input = vec![crate::log::Section::Code {
        label: "command".into(),
        lang: "text".into(),
        text: source.into(),
    }];
    event.calls[0].output = None;
    view.select_fixture_event(Some(event.clone()));
    let open = log_screen(&view, 100, 48, now);
    assert!(super::log::open_text(&event).contains(source));
    let (_, arrow_y) = rendered_cell(&open, "↓");
    let (_, running_y) = rendered_cell(&open, super::tool_detail::running_frame(now));
    assert_eq!(running_y, arrow_y + 1);
    let (x, y) = rendered_cell(&open, "45    # terminal");
    view.begin_text_selection(x, y);
    view.extend_text_selection(x + source.replace('\t', "    ").len() as u16, y);
    log_screen(&view, 100, 48, now);
    assert_eq!(
        view.finish_text_selection(x + source.replace('\t', "    ").len() as u16, y),
        Some(Action::Copy {
            text: source.into(),
            label: "Copied tool message".into()
        })
    );
    view.begin_text_selection(x + 2, y);
    view.extend_text_selection(x + 4, y);
    assert_eq!(view.finish_text_selection(x + 4, y), None);
    // All four display cells refer to one byte, and crossing the tab's end copies it once.
    view.begin_text_selection(x + 2, y);
    view.extend_text_selection(x + 6, y);
    assert_eq!(
        view.finish_text_selection(x + 6, y),
        Some(Action::Copy {
            text: "\t".into(),
            label: "Copied tool message".into()
        })
    );
}

#[test]
fn narrow_scrolled_tool_label_drag_stays_inside_the_buffer() {
    let (mut view, now) = open_tool_fixture();
    let mut event = view.log[7].clone();
    event.calls[0].input = vec![crate::log::Section::Diff {
        label: "/workspace/very/long/path/to/source.rs".repeat(6),
        text: "+中文".into(),
    }];
    view.select_fixture_event(Some(event));
    let open = log_screen(&view, 30, 25, now);
    let (x, y) = rendered_cell(&open, "/workspace");
    view.begin_text_selection(x, y);
    view.extend_text_selection(29, y + 4);
    log_screen(&view, 30, 25, now);
    let viewport = view.viewport.borrow();
    for row in &viewport.log_text.rows {
        assert!(
            row.cells
                .iter()
                .all(|(range, _)| range.end <= viewport.log_text.area.width)
        );
    }
}

#[test]
fn open_tool_without_calls_keeps_its_row_title() {
    let (mut view, now) = log_fixture();
    let event = view.log[7].clone();
    assert!(event.calls.is_empty());
    view.select_fixture_event(Some(event));
    let open = log_screen(&view, 100, 25, now);
    assert!(
        open.lines()
            .any(|line| line.trim_start().starts_with("run tests") && line.ends_with("35s")),
        "{open}"
    );
}

#[test]
fn tool_outputs_and_markdown_use_shared_json_rendering() {
    for output in [
        crate::log::Section::Text {
            label: "stdout".into(),
            text: "{\"answer\":[1,2]}".into(),
        },
        crate::log::Section::Markdown {
            label: "output".into(),
            text: "{\"answer\":[1,2]}".into(),
        },
        crate::log::Section::Code {
            label: "output".into(),
            lang: "text".into(),
            text: "{\"answer\":[1,2]}".into(),
        },
    ] {
        let (mut view, now) = open_tool_fixture();
        let mut event = view.log[7].clone();
        event.calls.truncate(1);
        event.calls[0].output = Some(vec![output]);
        view.select_fixture_event(Some(event.clone()));
        let open = log_screen(&view, 100, 48, now);
        if matches!(
            event.calls[0].output.as_ref().unwrap()[0],
            crate::log::Section::Code { .. }
        ) {
            assert!(open.contains("{\"answer\":[1,2]}"));
            assert!(!super::log::open_text(&event).contains("\n  \"answer\": ["));
        } else {
            assert!(open.contains("\"answer\": ["));
            assert!(super::log::open_text(&event).contains("\n  \"answer\": [\n    1,"));
        }
    }
    let (mut view, now) = open_tool_fixture();
    let mut event = view.log[7].clone();
    event.calls.truncate(1);
    event.calls[0].output = Some(vec![]);
    view.select_fixture_event(Some(event));
    let open = log_screen(&view, 100, 48, now);
    assert!(open.contains("(no output)"));
    assert!(open.lines().any(|line| line.trim() == "↓"));
    let area = view.viewport.borrow().log_text.area;
    assert_eq!(area.width, 88);
    let lines: Vec<&str> = open.lines().collect();
    assert!(
        (area.top()..area.bottom()).all(|y| {
            lines[usize::from(y)].chars().nth(usize::from(area.right())) != Some('│')
        })
    );
}

#[test]
fn pretty_json_body_remaps_provider_path_ranges() {
    let (mut view, now) = log_fixture();
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let path = dir.path().join("report.md");
    std::fs::write(&path, "report").unwrap();
    let path_text = path.to_str().unwrap();
    let mut event = view.log[7].clone();
    event.kind = crate::log::Kind::QueuedSystem;
    event.body = serde_json::json!({"note": "中文", "path": path_text}).to_string();
    let start = event.body.find(path_text).unwrap();
    event.paths.clear();
    event.paths.push(start..start + path_text.len());
    view.select_fixture_event(Some(event));
    log_screen(&view, 100, 48, now);
    assert!(
        view.viewport
            .borrow()
            .log_links
            .iter()
            .any(|(_, target)| target == &path)
    );
}

#[test]
fn commit_sha_links_open_vscode_from_log_and_details() {
    use std::process::Command;
    let dir = tempfile::Builder::new()
        .prefix("agents 仓库 #&")
        .tempdir()
        .unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--allow-empty",
        "-m",
        "initial",
    ]);
    let full = git(&["rev-parse", "HEAD"]);
    let sha = &full[..7];
    let root = git(&["rev-parse", "--show-toplevel"]);
    let encoded: String = root
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    let target = PathBuf::from(format!(
        "vscode://blockchainian.git/view-commit?repo={encoded}&sha={sha}"
    ));
    let (mut view, now) = log_fixture();
    view.rows[view.selected].cwd = dir.path().into();
    view.rows[view.selected].prompt = format!("已提交 `{sha}`，推送了");
    view.log[7].body = format!("commits {sha} deadbeef {}", "more text ".repeat(30));
    view.log[7].paths.clear();
    for kind in [crate::log::Kind::Assistant, crate::log::Kind::Tool] {
        view.log[7].kind = kind;
        view.set_log(view.log.clone());
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let rendered = loop {
            let rendered = log_screen(&view, 120, 40, now);
            if view
                .viewport
                .borrow()
                .log_links
                .iter()
                .any(|(_, url)| url == &target)
            {
                break rendered;
            }
            assert!(Instant::now() < deadline, "SHA never linked: {rendered}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let (x, y) = rendered_cell(&rendered, sha);
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
        view.render_at(buf.area, &mut buf, now);
        for column in x..x + sha.len() as u16 {
            assert!(
                !buf[(column, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::UNDERLINED)
            );
        }
        assert_eq!(
            click_text(&mut view, &rendered, sha),
            Some(Action::OpenPath(target.clone()))
        );
        assert_eq!(view.visible_log_event(), view.log.get(7));
        assert_eq!(click_text(&mut view, &rendered, "deadbeef"), None);
        assert!(view.log_selected.is_some());
        let rendered = log_screen(&view, 120, 40, now);
        assert_eq!(
            click_text(&mut view, &rendered, sha),
            Some(Action::OpenPath(target.clone()))
        );
        view.select_fixture_event(None);
    }
    let rendered = screen(&view, 120, 40);
    assert_eq!(
        click_text(&mut view, &rendered, sha),
        Some(Action::OpenPath(target))
    );
    // Open the layout before the full SHA has been checked; it must gain the link later.
    view.log[7].body = format!("commit {full}");
    view.select_fixture_event(Some(view.log[7].clone()));
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let rendered = log_screen(&view, 120, 40, now);
        let link = view
            .viewport
            .borrow()
            .log_links
            .iter()
            .find(|(_, url)| url.to_str().unwrap().ends_with(&format!("&sha={full}")))
            .cloned();
        if let Some((rect, target)) = link {
            // The tool title repeats the SHA; click its linked body occurrence.
            assert!(
                rendered
                    .lines()
                    .nth(usize::from(rect.y))
                    .unwrap()
                    .contains(&full)
            );
            view.handle_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                rect.x,
                rect.y,
            ));
            assert_eq!(
                view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), rect.x, rect.y))
                    .or_else(
                        || view.flush_pending_click(Instant::now() + view.double_click_interval)
                    ),
                Some(Action::OpenPath(target))
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cached layout never gained its SHA link"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn mixed_open_outputs_keep_literal_markup_and_copy_pretty_json() {
    let output = "# Result\n{\"answer\":[1,2]}\n**finished**";
    for calls in [false, true] {
        let (mut view, now) = open_tool_fixture();
        let mut event = view.log[7].clone();
        if calls {
            event.calls.truncate(1);
            event.calls[0].input.clear();
            event.calls[0].output = Some(vec![crate::log::Section::Text {
                label: "stdout".into(),
                text: output.into(),
            }]);
        } else {
            event.calls.clear();
            event.kind = crate::log::Kind::Error;
            event.body = output.into();
        }
        view.select_fixture_event(Some(event.clone()));
        let screen = log_screen(&view, 100, 64, now);
        assert!(screen.contains("Result"));
        assert!(screen.contains("\"answer\": ["));
        assert!(screen.contains("finished"));
        assert!(
            screen
                .lines()
                .skip(usize::from(view.viewport.borrow().log_detail.y))
                .any(|line| line.contains("**finished**"))
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 64));
        view.render_at(buf.area, &mut buf, now);
        let detail_y = view.viewport.borrow().log_detail.y;
        let detail_screen = screen
            .lines()
            .skip(usize::from(detail_y))
            .collect::<Vec<_>>()
            .join("\n");
        let (x, y) = rendered_cell(&detail_screen, "\"answer\"");
        let y = y + detail_y;
        assert!(matches!(
            buf[(x, y)].fg,
            Color::Rgb(..) | Color::Indexed(..)
        ));
        let viewport = view.viewport.borrow();
        let json = viewport
            .log_text
            .sources
            .iter()
            .find(|source| source.text.starts_with('{'))
            .unwrap();
        assert_eq!(
            json.text.as_ref(),
            "{\n  \"answer\": [\n    1,\n    2\n  ]\n}"
        );
        for row in viewport
            .log_text
            .rows
            .iter()
            .filter(|row| row.segment == json.id)
        {
            for (cells, bytes) in &row.cells {
                let drawn: String = (cells.start..cells.end)
                    .map(|offset| buf[(row.x + offset, row.y)].symbol())
                    .collect();
                assert_eq!(drawn, &json.text[bytes.clone()]);
            }
        }
        drop(viewport);
        let (start_x, start_y) = rendered_cell(&detail_screen, "Result");
        let start_y = start_y + detail_y;
        let (end_x, end_y) = rendered_cell(&detail_screen, "finished");
        let end_y = end_y + detail_y;
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            start_x,
            start_y,
        ));
        view.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            end_x + 8,
            end_y,
        ));
        log_screen(&view, 100, 64, now);
        assert_eq!(
            view.handle_mouse(mouse(
                MouseEventKind::Up(MouseButton::Left),
                end_x + 8,
                end_y
            )),
            Some(Action::Copy {
                text: "Result\n{\n  \"answer\": [\n    1,\n    2\n  ]\n}\n**finished".into(),
                label: "Copied selection".into(),
            })
        );
    }
}

#[test]
fn log_new_button_keeps_centering_and_target() {
    for width in [60, 100, 149, 150, 200] {
        for unseen in [1, 3] {
            let (mut view, now) = log_fixture();
            let mut event = view.log[6].clone();
            event.body = (0..100).map(|n| format!("row {n}\n\n")).collect();
            event.calls.clear();
            view.log[6] = event;
            view.select_log_event(Some(6));
            log_screen(&view, width, 40, now);
            view.viewport.borrow_mut().log_detail_scroll = 1;
            view.viewport.borrow_mut().log_unseen = unseen;
            let screen = log_screen(&view, width, 40, now);
            let layout = view.center_layout(Rect::new(0, 0, width, 40));
            let viewport = view.viewport.borrow();
            let latest = viewport.log_jump;
            let border_y = layout.log.bottom();
            assert_eq!(
                latest.x,
                super::centering::centered_x(
                    layout.log,
                    super::log::log_row_areas(layout.log).0,
                    latest.width,
                )
            );
            assert_eq!(latest.y, border_y);
            let label = format!(" ↓ {unseen} New ");
            assert_eq!(rendered_cell(&screen, &label), (latest.x, latest.y));
            let row = screen.lines().nth(usize::from(border_y)).unwrap();
            assert_eq!(row.chars().next(), Some('─'));
            drop(viewport);
            click_mouse(
                &mut view,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: latest.x,
                    row: latest.y,
                    modifiers: KeyModifiers::NONE,
                },
            );
            assert!(view.following_log());
            assert_eq!(view.viewport.borrow().log_unseen, 0);
        }
    }
}

#[test]
fn wrapped_senders_stay_in_list_and_move_to_open_header() {
    let file = crate::providers::claude::log::tests::wrapped_fixture();
    let events = crate::providers::claude::log::tail(file.path(), 20).unwrap();
    let (mut view, now) = log_fixture();
    for (index, sender, label, body) in [
        (2, "a113c3cb9c1482c7a", "Queue", "Report:"),
        (3, "a113c3cb9c1482c7a", "Agent", "Report:"),
        (
            4,
            "translate-pdf",
            "Queue",
            "translate-pdf pilots ready for review.",
        ),
        (
            5,
            "translate-pdf",
            "Session",
            "translate-pdf pilots ready for review.",
        ),
    ] {
        let mut event = events[index].clone();
        event.started_at = now - Duration::seconds(120);
        // The list retains the complete participant prefix and its rendering.
        assert!(
            super::log::body_line(&event, 200, &view.rows[0].cwd)
                .to_string()
                .starts_with(&format!("({sender}) {body}"))
        );
        let mut previous = event.clone();
        previous.sender = None;
        previous.body = crate::log::participant_text(Some(sender), &event.body);
        for width in [25, 60, 200] {
            assert_eq!(
                super::log::body_line(&event, width, &view.rows[0].cwd),
                super::log::body_line(&previous, width, &view.rows[0].cwd)
            );
        }
        let mut later = event.clone();
        later.started_at = now;
        view.set_log(vec![event.clone(), later]);
        view.select_fixture_event(Some(event));
        let rendered = log_screen(&view, 160, 25, now);
        let header = rendered
            .lines()
            .nth(usize::from(view.viewport.borrow().log_detail.y + 1))
            .unwrap();
        assert!(header.trim_start().starts_with(sender), "sender: {header}");
        assert!(header.contains(label), "kind: {header}");
        assert!(header.ends_with("11:58:00"), "time: {header}");
        assert!(
            !super::log::open_text(view.visible_log_event().unwrap())
                .contains(&format!("({sender})"))
        );
    }
}

#[test]
fn wrapped_messages_keep_list_text_and_open_paragraphs() {
    let file = crate::providers::claude::log::tests::wrapped_fixture();
    let events = crate::providers::claude::log::tail(file.path(), 20).unwrap();
    let (mut view, now) = log_fixture();
    for event in &events[2..] {
        let mut collapsed = event.clone();
        collapsed.body = event.body.split_whitespace().collect::<Vec<_>>().join(" ");
        for width in [25, 60, 200, 2000] {
            let row = super::log::body_line(event, width, &view.rows[0].cwd);
            assert!(!row.to_string().contains('\n'));
            assert_eq!(
                row.to_string(),
                super::log::body_line(&collapsed, width, &view.rows[0].cwd).to_string(),
                "{:?}, width {width}",
                event.kind
            );
        }
    }
    for event in &events[6..12] {
        view.log = vec![event.clone()];
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, 2000, 25);
        let mut buffer = Buffer::empty(area);
        view.render_log_detail(area, &mut buffer, now);
        let lines: Vec<String> = buffer
            .content
            .chunks(2000)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim()
                    .to_owned()
            })
            .collect();
        let state = lines
            .iter()
            .position(|line| line.starts_with("State as I understand it:"))
            .expect("separate second paragraph");
        let traps = lines.iter().position(|line| *line == "Traps:").unwrap();
        let first = lines
            .iter()
            .position(|line| line.starts_with("1. Version bump"))
            .unwrap();
        let second = lines
            .iter()
            .position(|line| line.starts_with("2. Your todo"))
            .unwrap();
        let last = lines
            .iter()
            .position(|line| *line == "No reply needed.")
            .unwrap();
        assert!(lines[state - 1].is_empty());
        assert!(lines[traps - 1].is_empty());
        assert!(state < traps && traps < first && first < second && second < last);
        assert!(lines[last - 1].is_empty());
    }
}

#[test]
fn usage_chart_rises_in_braille_below_the_task_rows_and_keeps_its_bins_as_time_passes() {
    use crate::usage::{Bin, bin_of};
    let (mut view, now) = log_fixture();
    let id = view.rows[view.selected].id.clone();
    let mut bins = std::collections::BTreeMap::new();
    for (offset, tokens) in [(0, 100_000), (-1, 50_000), (-40, 1_000)] {
        bins.insert(
            bin_of(now) + offset,
            Bin {
                input: tokens / 2,
                output: tokens / 2,
            },
        );
    }
    view.set_usage(&id, bins);
    let chart = |at: chrono::DateTime<Utc>| {
        let area = Rect::new(0, 0, 100, 25);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, at);
        let list = view.center_layout(area).list;
        (list.top()..list.bottom())
            .map(|y| {
                (list.left()..list.right())
                    .map(|x| buf[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let rows = chart(now);
    let braille = |row: &str| {
        row.chars()
            .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
            .count()
    };
    let task = rows
        .iter()
        .position(|row| row.contains("Task log example"))
        .unwrap();
    assert!(
        rows[..=task].iter().all(|row| braille(row) == 0),
        "{rows:#?}"
    );
    assert!(braille(rows.last().unwrap()) >= 2, "{rows:#?}");
    assert!(
        braille(&rows[task + 1]) > 0,
        "the busiest bin rises to the row under the task: {rows:#?}"
    );
    for seconds in [5, 10] {
        let later = chart(now + Duration::seconds(seconds));
        assert_eq!(
            braille(later.last().unwrap()),
            braille(rows.last().unwrap()),
            "after {seconds}s the chart shows the same bins"
        );
    }
}

#[test]
fn usage_chart_and_activity_strip_share_the_same_style() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    use crate::usage::{Bin, bin_of};

    let (mut view, now) = log_fixture();
    let id = view.rows[view.selected].id.clone();
    let bin = bin_of(now);
    view.set_usage(
        &id,
        [(
            bin,
            Bin {
                input: 100,
                output: 0,
            },
        )]
        .into(),
    );
    view.set_activity([bin].into());
    let area = Rect::new(0, 0, 100, 25);

    for use_colors in [true, false] {
        view.use_colors = use_colors;
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        let x = layout.list.right() - 1;
        let chart = &buf[(x, layout.list.bottom() - 1)];
        let strip = &buf[(x, layout.prompt.y - 1)];
        for cell in [chart, strip] {
            assert!(
                cell.symbol()
                    .chars()
                    .all(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
            );
        }
        assert_eq!(chart.fg, strip.fg, "use_colors={use_colors}");
        assert_eq!(chart.style(), strip.style(), "use_colors={use_colors}");
    }
}

#[test]
fn activity_strip_dots_the_log_rule_where_a_task_worked_in_the_inline_code_color() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (mut view, now) = log_fixture();
    view.use_colors = true;
    let area = Rect::new(0, 0, 100, 25);
    let layout = view.center_layout(area);
    let (y, right) = (layout.prompt.y - 1, layout.list.right() - 1);
    let [left_bin, right_bin] = super::timeline::cell_bins(layout.list, right, now);
    // The newest cell's both bins, then a gap, then a cell's left bin only.
    view.set_activity([left_bin, right_bin, left_bin - 4].into());
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let cell = |x: u16| &buf[(x, y)];
    assert_eq!(cell(right).symbol(), "⠒");
    assert_eq!(cell(right).fg, crate::theme::inline_code_color());
    assert!(!cell(right).modifier.contains(ratatui::style::Modifier::DIM));
    assert_eq!(cell(right - 1).symbol(), "─");
    assert_eq!(cell(right - 2).symbol(), "⠂");
    assert!(view.timeline_redraw_in(now).is_some());
}

#[path = "click_log_tests.rs"]
mod click_log_tests;

#[path = "prompt_view_tests.rs"]
mod prompt_view_tests;

#[path = "details_group_tests.rs"]
mod details_group_tests;

#[path = "details_order_tests.rs"]
mod details_order;

#[test]
fn details_activity_single_click_selects_source_and_double_click_copies() {
    details_click_selects_source(7, "Copied latest activity");
}

#[test]
fn details_update_single_click_selects_source_and_double_click_copies() {
    details_click_selects_source(6, "Copied update");
}

#[test]
fn details_prompt_single_click_selects_source_and_double_click_copies() {
    details_click_selects_source(0, "Copied prompt");
}

fn details_click_selects_source(index: usize, label: &'static str) {
    let (mut view, now) = log_fixture();
    for (index, event) in view.log.iter_mut().enumerate() {
        event.source = Some(format!("fixture:{index}"));
    }
    let activity = view.log[7].clone();
    let update = view.log[6].clone();
    let prompt = view.log[0].clone();
    let row = &mut view.rows[view.selected];
    row.activity = Some(crate::model::Activity {
        kind: activity.kind.clone(),
        label: activity.body.clone(),
        at: activity.started_at,
    });
    row.last_message = Some(update.body.clone());
    row.last_message_at = Some(update.started_at);
    row.prompt = prompt.body.clone();
    row.prompt_at = Some(prompt.started_at);
    row.detail_sources = [
        activity.source.clone(),
        update.source.clone(),
        prompt.source.clone(),
    ];
    let expected = view.log[index].clone();
    view.select_fixture_event(Some(view.log[1].clone()));
    let rendered = log_screen(&view, 160, 45, now);
    let rows = view.viewport.borrow().details_copy.clone();
    let y = rows
        .iter()
        .find(|(_, _, value)| *value == label)
        .unwrap()
        .0
        .start;
    let x = view.viewport.borrow().details.x;
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        None
    );
    assert_eq!(
        view.visible_log_event(),
        Some(&expected),
        "{label}\n{rendered}"
    );
    log_screen(&view, 160, 45, now);
    assert!(view.viewport.borrow().log_rows.contains(&index));
    assert_eq!(view.visible_log_event() == view.log.last(), index == 7);
    let previous = view.log[1].clone();
    view.select_fixture_event(Some(previous.clone()));
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(
        click_mouse(&mut view, click),
        Some(Action::Copy {
            text: expected.body.clone(),
            label: label.into(),
        })
    );
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        None
    );
    assert_eq!(view.visible_log_event(), Some(&previous));
    view.set_log(Vec::new());
    log_screen(&view, 160, 45, now);
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        None
    );
    assert!(view.log_selected.is_none());
}

#[test]
fn task_notification_detail_shows_task_id_and_output_content() {
    let dir = tempfile::tempdir().unwrap();
    let events =
        super::log::log_markdown::notification_events(dir.path(), Some("all 3 tests passed"));
    let (mut view, now) = log_fixture();
    for event in events {
        let summary = event.body.clone();
        view.log = vec![event.clone(), event.clone()];
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, 160, 15);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 40));
        view.render_log_detail(area, &mut buffer, now);
        let lines: Vec<_> = buffer
            .content
            .chunks(160)
            .take(usize::from(area.height))
            .map(|row| {
                let mut text = String::new();
                let mut column = 0;
                while column < row.len() {
                    let symbol = row[column].symbol();
                    text.push_str(symbol);
                    column += unicode_width::UnicodeWidthStr::width(symbol).max(1);
                }
                text
            })
            .collect();
        let detail = super::tool_detail::detail(&event, String::new(), Style::new().dim());
        assert_eq!(
            detail.sections[0].title.left.as_deref(),
            Some("Task b9wi31ww0")
        );
        let header = 1;
        assert!(
            !lines[header + 1..]
                .iter()
                .any(|line| line.contains(&summary))
        );
        let label_x =
            super::detail_model::detail_columns(area, view.center_layout(buffer.area).log)
                .text
                .x;
        let time_x = lines[header].trim_end().width() as u16 - 1;
        assert_eq!(
            buffer[(label_x, header as u16)].style(),
            buffer[(time_x, header as u16)].style()
        );
        assert_eq!(
            lines
                .iter()
                .map(|line| line.trim_end())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
            include_str!("../../tests/ui_snapshots/task_notification_detail.txt")
        );
        let rest = &lines[header + 1..];
        let arrow = rest.iter().position(|line| line.trim() == "↓").unwrap();
        assert!(
            rest[arrow + 1..]
                .iter()
                .any(|line| line.trim() == "all 3 tests passed")
        );
        assert!(
            !rest.iter().any(|line| line.contains("status")),
            "{lines:#?}"
        );
        assert!(
            !rest.iter().any(|line| line.contains("output-file")),
            "{lines:#?}"
        );
    }
}

#[path = "codex_exit_tests.rs"]
mod codex_exit_tests;

#[path = "details_tasks_tests.rs"]
mod details_tasks;

#[test]
fn wide_separators_join_the_rules_above_and_below() {
    let (view, now) = column_padding_fixture();
    for width in [150, 200, 240] {
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        for (x, top, bottom) in [
            (layout.gap.x + 1, layout.gap.y, layout.gap.bottom()),
            (layout.log.x - 2, layout.log.y, layout.log.bottom()),
        ] {
            assert_eq!(buf[(x, top - 1)].symbol(), "┬");
            assert_eq!(buf[(x, top)].symbol(), "│");
            assert_eq!(buf[(x, bottom - 1)].symbol(), "│");
            assert_eq!(buf[(x, bottom)].symbol(), "┴");
        }
    }
}

#[test]
fn dashboard_starts_with_tabs_and_one_header_rule() {
    for width in [120, 200] {
        let view = fixture();
        let rendered = screen(&view, width, 45);
        let layout = view.center_layout(Rect::new(0, 0, width, 45));
        assert_eq!(layout.header.height, 2);
        assert_eq!(view.viewport.borrow().tab_bar.y, 0);
        assert!(rendered.lines().next().unwrap().contains("Active"));
        assert!(rendered.lines().nth(1).unwrap().contains('─'));
        assert!(!rendered.contains("Agent command center"));
    }
}

#[test]
fn border_labels_use_dim_half_lines_beside_separator_joints() {
    let (mut view, now) = column_padding_fixture();
    view.rows[0].model = Some("opus".into());
    view.rows[0].started_at = Some(now - Duration::minutes(97));
    for width in [120, 200] {
        let area = Rect::new(0, 0, width, 45);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        let y = layout.details.bottom();
        let joint = layout.gap.x + 1;
        assert_eq!(buf[(joint, y)].symbol(), "┴");
        let cell = &buf[(joint + 1, y)];
        assert_eq!(cell.symbol(), "╴", "width {width}");
        assert_eq!(cell.style(), buf[(joint, y)].style());
        assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
        if width >= 150 {
            let joint = layout.log.x - 2;
            for (x, y, symbol) in [
                (joint - 1, layout.log.y - 1, "╶"),
                (joint - 1, layout.log.bottom(), "╶"),
                (joint + 1, layout.log.bottom(), "╴"),
            ] {
                let cell = &buf[(x, y)];
                assert_eq!(cell.symbol(), symbol, "width {width}, x {x}, y {y}");
                assert_eq!(cell.style(), buf[(joint, y)].style());
                assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
            }
        }
        let bottom: String = (0..width).map(|x| buf[(x, y)].symbol()).collect();
        assert!(bottom.contains(" Working "), "{bottom}");
        let top: String = (0..width)
            .map(|x| buf[(x, layout.header.y + 1)].symbol())
            .collect();
        assert!(top.contains(" Task log example "), "{top}");
    }
}

#[test]
fn border_labels_use_half_lines_beside_screen_edge_corners() {
    let (mut view, now) = column_padding_fixture();
    view.rows[0].model = Some("opus".into());
    view.rows[0].effort = Some("medium".into());
    for width in [120, 200] {
        let area = Rect::new(0, 0, width, 45);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let model = view
            .viewport
            .borrow()
            .field_targets
            .iter()
            .find(|(_, field)| *field == Field::Model)
            .unwrap()
            .0;
        let row: String = (0..width).map(|x| buf[(x, model.y)].symbol()).collect();
        assert!(row.ends_with(" medium╶─"), "{row}");
        for x in [model.x - 1, width - 2] {
            assert!(
                buf[(x, model.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
        }
        if width < 150 {
            assert!(row.starts_with("─╴"), "{row}");
        }
    }
}

mod log_navigation {
    use super::*;

    fn check(code: KeyCode, modifiers: KeyModifiers, forward: bool, edge: bool) {
        for search in [false, true] {
            for editing in [false, true] {
                if editing && !search {
                    continue;
                }
                let (mut view, _) = log_fixture();
                view.mouse_focus = Panel::LogList;
                if search {
                    view.prepare_log_search();
                    view.log_search.query.set_text("a");
                    view.rematch_log();
                    view.log_search.editing = editing;
                }
                screen(&view, 200, 45);
                let rect = view.viewport.borrow().log;
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    view.handle_mouse(mouse(kind, rect.x + 1, rect.y));
                }
                assert!(view.log_list_focused);
                view.viewport.borrow_mut().log.height = 40;
                let events: Vec<_> = view.shown_log_events().map(|(_, e)| e.clone()).collect();
                assert!(events.len() >= 3);
                let current = events.len() / 2;
                view.select_fixture_event(Some(events[current].clone()));
                let key = KeyEvent::new(code, modifiers);
                let mut editor = super::super::line_edit::LineEdit::default();
                editor.set_text("a");
                let consumed = editing && editor.handle(key);
                if consumed {
                    view.handle_key(key);
                    assert_eq!(view.log_search.query.text(), editor.text());
                    continue;
                }
                let target = if edge {
                    if forward { events.len() - 1 } else { 0 }
                } else if forward {
                    current + 1
                } else {
                    current - 1
                };
                view.handle_key(key);
                assert_eq!(
                    view.visible_log_event(),
                    Some(&events[target]),
                    "{key:?}, search={search}, editing={editing}"
                );
            }
        }
    }

    macro_rules! binding {
        ($name:ident, $code:expr, $mods:expr, $forward:expr, $edge:expr) => {
            #[test]
            fn $name() {
                check($code, $mods, $forward, $edge);
            }
        };
    }
    binding!(up, KeyCode::Up, KeyModifiers::NONE, false, false);
    binding!(down, KeyCode::Down, KeyModifiers::NONE, true, false);
    binding!(j, KeyCode::Char('j'), KeyModifiers::NONE, true, false);
    binding!(k, KeyCode::Char('k'), KeyModifiers::NONE, false, false);
    binding!(
        ctrl_j,
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
        true,
        false
    );
    binding!(
        ctrl_k,
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
        false,
        false
    );
    binding!(
        ctrl_n,
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
        true,
        false
    );
    binding!(
        ctrl_p,
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
        false,
        false
    );
    binding!(page_up, KeyCode::PageUp, KeyModifiers::NONE, false, true);
    binding!(page_down, KeyCode::PageDown, KeyModifiers::NONE, true, true);
    binding!(
        ctrl_b,
        KeyCode::Char('b'),
        KeyModifiers::CONTROL,
        false,
        true
    );
    binding!(
        ctrl_f,
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
        true,
        true
    );
    binding!(home, KeyCode::Home, KeyModifiers::NONE, false, true);
    binding!(end, KeyCode::End, KeyModifiers::NONE, true, true);
}

#[test]
fn log_border_double_clicks_select_both_ends_in_both_layouts() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = log_fixture();
        log_screen(&view, width, 20, now);
        let log = view.viewport.borrow().log;
        let click = |view: &mut View, y| {
            click_mouse(
                view,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: log.x + log.width / 3,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
            )
        };
        let newest = view.shown_log_events().last().unwrap().1.clone();
        let oldest = view.shown_log_events().next().unwrap().1.clone();
        assert_eq!(click(&mut view, log.y - 1), None);
        assert_eq!(view.visible_log_event(), Some(&newest));
        assert_eq!(click(&mut view, log.y - 1), None);
        assert_eq!(view.visible_log_event(), Some(&oldest), "width {width}");
        log_screen(&view, width, 20, now);
        assert!(view.viewport.borrow().log_rows.contains(&0));
        assert_eq!(view.log_scroll(), 0);
        assert!(view.viewport.borrow().log_jump.is_empty());
        click(&mut view, log.bottom());
        assert_eq!(view.visible_log_event(), Some(&oldest));
        click(&mut view, log.bottom());
        assert_eq!(view.visible_log_event(), Some(&newest));
        assert_eq!(view.log_scroll(), view.log_max_scroll());
        log_screen(&view, width, 20, now);
        assert!(view.viewport.borrow().log_jump.is_empty());
        // New arrivals provide a single-click return path after jumping to the oldest.
        click(&mut view, log.y - 1);
        click(&mut view, log.y - 1);
        view.viewport.borrow_mut().log_unseen = 1;
        log_screen(&view, width, 20, now);
        let jump = view.viewport.borrow().log_jump;
        click_mouse(
            &mut view,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: jump.x,
                row: jump.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(view.visible_log_event(), Some(&newest));
    }
}

#[test]
fn log_border_fields_keep_click_actions_without_jumping() {
    for width in [100, 200] {
        for field in [Field::Model, Field::Effort] {
            let (mut view, now) = log_fixture();
            view.rows[view.selected].model = Some("opus".into());
            view.rows[view.selected].background_job = Some(crate::model::BackgroundJob {
                id: "e772d00b".into(),
                needs: None,
            });
            view.rows[view.selected].effort = Some("high".into());
            view.rows[view.selected].process.as_mut().unwrap().label = Some("bg".into());
            let config = tempfile::tempdir().unwrap();
            crate::providers::claude::write_fixture_cache(config.path());
            let provider = crate::providers::claude::ClaudeProvider::new(config.path().into());
            view.set_input_choices(HashMap::from([(
                ProviderId::Claude,
                crate::providers::Provider::input_choices(&provider),
            )]));
            let selected = view.log[3].clone();
            view.select_fixture_event(Some(selected.clone()));
            log_screen(&view, width, 45, now);
            let rect = view
                .viewport
                .borrow()
                .field_targets
                .iter()
                .find(|(_, candidate)| *candidate == field)
                .unwrap()
                .0;
            let mouse = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            };
            click_mouse(&mut view, mouse);
            assert_eq!(view.visible_log_event(), Some(&selected));
            click_mouse(&mut view, mouse);
            assert_eq!(view.visible_log_event(), Some(&selected));
            assert!(view.selector.is_some());
        }
    }
}

#[test]
fn log_border_omits_model_and_effort_it_cannot_read() {
    let (mut view, now) = log_fixture();
    let row = &mut view.rows[view.selected];
    row.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
    });
    row.process.as_mut().unwrap().label = Some("bg".into());
    row.model = None;
    row.model_label = None;
    row.effort = None;
    let config = tempfile::tempdir().unwrap();
    crate::providers::claude::write_fixture_cache(config.path());
    let provider = crate::providers::claude::ClaudeProvider::new(config.path().into());
    view.set_input_choices(HashMap::from([(
        ProviderId::Claude,
        crate::providers::Provider::input_choices(&provider),
    )]));
    let screen = log_screen(&view, 100, 45, now);
    assert!(!screen.contains("Unknown"), "{screen}");
    assert!(view.viewport.borrow().field_targets.is_empty());
}

#[test]
fn log_border_help_entries_fit() {
    let view = View::default();
    for width in [40, 54, 58, 100, 200] {
        let lines = view.center_help_lines(width);
        assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
        let text = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("2× click top border"));
        assert!(text.contains("Latest"));
        assert!(text.contains("2× click bottom border"));
        assert!(text.contains("Earliest"));
    }
}

#[test]
fn log_border_bottom_double_click_selects_latest_in_both_layouts() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = log_fixture();
        view.select_fixture_event(Some(view.log[0].clone()));
        log_screen(&view, width, 20, now);
        let log = view.viewport.borrow().log;
        let mouse = mouse(
            MouseEventKind::Down(MouseButton::Left),
            log.x + log.width / 3,
            log.bottom(),
        );
        click_mouse(&mut view, mouse);
        click_mouse(&mut view, mouse);
        assert_eq!(view.visible_log_event(), view.log.last(), "width {width}");
        assert_eq!(view.log_scroll(), view.log_max_scroll());
    }
}

#[path = "index_selection_tests.rs"]
mod index_selection;

#[test]
fn claude_agent_display_from_real_records() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-agent-display.jsonl");
    let events = crate::providers::claude::log::tail(&path, 100).unwrap();
    for (id, label, metadata) in [
        (
            "toolu_015h3Ngbytx63sMddkJwjDX7",
            "attach-probe · 实测 attach 发提示词",
            "general-purpose",
        ),
        (
            "toolu_01X69UrWoFVNnNKno4yavSre",
            "接手边框和标题行改动",
            "feature:ux-implementer",
        ),
        ("toolu_01YMEgiGEMc9cjoePq6j2E69", "滚动条挪到最右边", "fork"),
    ] {
        let event = events
            .iter()
            .find(|e| e.calls.iter().any(|c| c.id == id))
            .unwrap();
        assert_eq!(event.body, "Agent");
        assert_eq!(event.calls[0].title, "Agent");
        let (mut view, now) = log_fixture();
        view.select_fixture_event(Some(event.clone()));
        let screen = log_screen(&view, 200, 45, now);
        let rows: Vec<_> = screen.lines().collect();
        let detail = view.viewport.borrow().log_detail;
        let header = usize::from(detail.y + 1);
        assert!(!rows[header].contains(metadata), "{screen}");
        assert!(
            rows[header + 1..].iter().any(|row| row.contains(metadata)),
            "{screen}"
        );
        assert!(!label.is_empty());
        let description = event.calls[0].detail_title.clone();
        assert!(
            rows[header]
                .replace(' ', "")
                .contains(&description.replace(' ', "")),
            "{screen}"
        );
        assert!(
            !rows[header + 1..].iter().any(|row| row.trim() == metadata),
            "{screen}"
        );
        for kind in [
            crate::log::Kind::Tool,
            crate::log::Kind::Error,
            crate::log::Kind::Denied,
        ] {
            let mut event = event.clone();
            event.kind = kind;
            view.log = vec![event.clone(), event.clone()];
            view.select_fixture_event(Some(event));
            let mut buffer = Buffer::empty(Rect::new(0, 0, 200, 45));
            view.render_at(buffer.area, &mut buffer, now);
            let title = view.viewport.borrow().log_text.sources[0].text.to_string();
            assert!(title.contains("Agent"));
            assert!((0..200).any(|x| buffer[(x, header as u16)].symbol() == "A"));
        }
        let description = event.calls[0].detail_title.clone();
        assert!(
            !rows[header + 1..]
                .iter()
                .any(|row| row.replace(' ', "") == description.replace(' ', "")),
            "{screen}"
        );
    }
}

#[test]
fn session_search_overlays_bottom_of_unchanged_usage_chart() {
    for (width, height) in [(200, 50), (120, 45), (80, 12), (60, 6)] {
        let (mut view, now) = log_fixture();
        view.set_usage(
            &session_key("log"),
            [(
                crate::usage::bin_of(now),
                crate::usage::Bin {
                    input: 100,
                    output: 0,
                },
            )]
            .into(),
        );
        let area = Rect::new(0, 0, width, height);
        let baseline = view.center_layout(area).list;
        let mut before = Buffer::empty(area);
        view.render_at(area, &mut before, now);
        let chart_top = (baseline.y..baseline.bottom())
            .find(|&y| {
                (baseline.x..baseline.right()).any(|x| {
                    before[(x, y)]
                        .symbol()
                        .chars()
                        .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                })
            })
            .unwrap_or(baseline.bottom());
        view.searching = true;
        view.search.insert("Task");
        let layout = view.center_layout(area);
        assert_eq!(layout.usage_chart, baseline);
        assert_eq!(layout.search.y, baseline.bottom() - 1, "{width}x{height}");
        assert_eq!(layout.search.bottom(), baseline.bottom());
        assert_eq!(layout.list.y, baseline.y);
        assert_eq!(layout.list.bottom(), layout.search.y);
        let mut after = Buffer::empty(area);
        view.render_at(area, &mut after, now);
        for y in chart_top..layout.search.y {
            for x in baseline.x..baseline.right() {
                assert_eq!(after[(x, y)], before[(x, y)], "{width}x{height} ({x}, {y})");
            }
        }
        assert!(
            (layout.search.x..layout.search.right()).all(|x| {
                after[(x, layout.search.y)]
                    .symbol()
                    .chars()
                    .all(|c| !('\u{2801}'..='\u{28ff}').contains(&c))
            }),
            "search row must not contain chart dots: {width}x{height}"
        );
        view.set_notice(Some("Notice".into()));
        view.render_at(area, &mut after, now);
        if !layout.list.is_empty() {
            let notice: String = (layout.list.x..layout.list.right())
                .map(|x| after[(x, layout.list.bottom() - 1)].symbol())
                .collect();
            assert!(notice.contains("Notice"));
        }
        let input: String = (layout.search.x..layout.search.right())
            .map(|x| after[(x, layout.search.y)].symbol())
            .collect();
        assert!(input.contains("Search ❯ Task"));
        assert_eq!(view.cursor_pos(area).unwrap().1, layout.search.y);
        assert!(
            !view
                .viewport
                .borrow()
                .list_rows
                .contains((layout.search.x, layout.search.y).into())
        );
        view.searching = false;
        key(&mut view, KeyCode::Char('r'));
        let rename = view.center_layout(area);
        assert_eq!(rename.search.y, baseline.y);
        assert_eq!(rename.list.y, baseline.y + 1);
        assert_eq!(rename.list.bottom(), baseline.bottom());
        assert_eq!(rename.usage_chart, rename.list);
        view.render_at(area, &mut after, now);
        let input: String = (rename.search.x..rename.search.right())
            .map(|x| after[(x, rename.search.y)].symbol())
            .collect();
        assert!(input.contains("Rename ❯"));
    }
}

#[test]
fn session_search_keeps_selection_visible_and_mouse_rows_aligned() {
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(
        (0..20)
            .map(|i| {
                session(
                    &format!("s{i}"),
                    &format!("Session {i}"),
                    SessionStatus::Working,
                    i,
                )
            })
            .collect(),
        false,
    );
    let area = Rect::new(0, 0, 80, 12);
    view.set_selected(19);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    let baseline = view.center_layout(area).list;
    view.searching = true;
    view.render(area, &mut buf);
    let layout = view.center_layout(area);
    assert_eq!(layout.search.y, baseline.bottom() - 1);
    assert_eq!(layout.list.y, baseline.y);
    assert_eq!(layout.list.height, baseline.height - 1);
    let viewport = view.viewport.borrow();
    let entries = view.center_rows(&view.selectable_indices(), view.grouping);
    let selected = entries.iter().position(|entry| matches!(entry, super::navigation::CenterRow::Task(index) if *index == view.selected)).unwrap();
    assert!(selected >= viewport.first_entry);
    assert!(selected < viewport.first_entry + usize::from(viewport.list_rows.height));
    let target = entries
        .iter()
        .enumerate()
        .skip(viewport.first_entry)
        .take(usize::from(viewport.list_rows.height))
        .find_map(|(offset, entry)| match entry {
            super::navigation::CenterRow::Task(index) => Some((
                *index,
                viewport.list_rows.y + (offset - viewport.first_entry) as u16,
            )),
            _ => None,
        })
        .unwrap();
    drop(viewport);
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.list.x,
            target.1,
        ),
    );
    assert_eq!(view.selected, 19);
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            layout.search.x,
            layout.search.y,
        ),
    );
    assert_eq!(view.selected, 19);
}

#[path = "reading_interaction_tests.rs"]
mod reading_interaction_tests;

#[path = "terminal_panel_tests.rs"]
mod terminal_panel_tests;
