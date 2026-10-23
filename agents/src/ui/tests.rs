#[path = "middle_paste_tests.rs"]
mod middle_paste_tests;
#[path = "subagent_handback_tests.rs"]
mod subagent_handback_tests;
#[path = "web_search_tests.rs"]
mod web_search_tests;

#[path = "claude_client_tests.rs"]
mod claude_client_tests;

#[path = "spawn_agent_tests.rs"]
mod spawn_agent_tests;
#[path = "task_navigation_tests.rs"]
mod task_navigation_tests;

#[path = "codex_memory_tests.rs"]
mod codex_memory_tests;

#[path = "codex_subagents_tests.rs"]
mod codex_subagents_tests;

#[path = "focus_rules_tests.rs"]
mod focus_rules_tests;
#[path = "panel_focus_tests.rs"]
mod panel_focus_tests;

#[path = "prompt_mouse_tests.rs"]
mod prompt_mouse_tests;

#[path = "event_center_tests.rs"]
mod event_center_tests;

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

#[path = "task_details_project_tests.rs"]
mod task_details_project;

#[path = "task_details_footer_tests.rs"]
mod task_details_footer;

#[path = "thread_clock_tests.rs"]
mod thread_clock_tests;

#[path = "time_alignment_tests.rs"]
mod time_alignment_tests;

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
fn mouse_tabs_ignore_modals_but_save_title_on_blur() {
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
                view.set_active_panel(Panel::TaskDetails);
            }
            3 => {
                key(&mut view, KeyCode::Backspace);
            }
            _ => unreachable!(),
        }
        screen(&view, 120, 40);
        view.viewport.borrow_mut().scroll = 7;
        let selected = view.selected;
        assert!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y)
            )
            .is_none()
        );
        if overlay == 2 {
            assert!(view.rename_target.is_none());
            assert_eq!(view.status_filter, 1);
            continue;
        }
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
        assert_eq!(view.handle_mouse(mouse(kind, rect.x, rect.y)), None);
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

fn overflowing_task_details() -> View {
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.status = SessionStatus::Ready;
    selected.prompt = "A long prompt that wraps across several rows. ".repeat(16);
    selected.last_message = Some("First reply row\n\nSecond reply row".into());
    selected.transcript = Some(selected.cwd.join("session.jsonl"));
    view
}

fn task_details_panel(view: &View) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    view.render_at(buf.area, &mut buf, view.rows[view.selected].updated_at);
    let area = view.viewport.borrow().task_details;
    let mut panel = Buffer::empty(Rect::new(0, 0, area.width, area.height));
    for y in 0..area.height {
        for x in 0..area.width {
            panel[(x, y)] = buf[(area.x + x, area.y + y)].clone();
        }
    }
    panel
}

#[test]
fn mouse_event_wheel_shifts_rendered_rows_and_hides_live_time() {
    let (mut view, now) = event_fixture();
    let mut older = view.event_panel[0].clone();
    older.body = "Older event".into();
    view.event_panel.insert(0, older);
    view.set_event(view.event_panel.clone());
    let before = event_list_screen(&view, 100, 32, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 32)).event_panel;
    assert!(
        before
            .lines()
            .any(|line| line.contains("run tests") && line.ends_with("35s")),
        "{before}"
    );
    for _ in 0..view.event_panel.len() {
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
        if view.event_scroll() == 0 {
            break;
        }
    }
    assert_eq!(view.event_scroll(), 0);
    let after = event_list_screen(&view, 100, 32, now);
    assert!(after.contains("Older event"), "{after}");
    assert!(!after.contains("run tests"));
    for y in rect.y + super::event_list::EVENT_TOP..rect.bottom() - 3 {
        assert_eq!(
            before.lines().nth(y as usize),
            after.lines().nth(y as usize + 1)
        );
    }
    // A tool at the visible edge must use its recorded duration, even if unfinished.
    view.event_panel[7] = view.event_panel[8].clone();
    view.event_panel[7].body = "Earlier unfinished tool".into();
    let after = event_list_screen(&view, 100, 32, now);
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
    view.event_list.scroll.set(0);
    assert!(
        event_screen(&view, 100, 32, now)
            .lines()
            .any(|line| line.contains("run tests") && line.ends_with("35s"))
    );
}

#[test]
fn mouse_event_wheel_keeps_body_column_when_clock_leaves_window() {
    use crate::event::Kind;
    let (mut view, now) = event_fixture();
    let mut event = view.event_panel.last().unwrap().clone();
    event.kind = Kind::Tool;
    event.ended_at = Some(event.started_at);
    event.previous_end = Some(event.started_at - Duration::seconds(12));
    view.event_panel = (0..12)
        .map(|index| {
            let mut event = event.clone();
            event.body = format!("Event {index:02}");
            event.started_at -= Duration::seconds(11 - index);
            event.ended_at = Some(event.started_at);
            event.previous_end = Some(event.started_at - Duration::seconds(12));
            event
        })
        .collect();
    view.event_panel.last_mut().unwrap().previous_end = None;
    view.set_event(view.event_panel.clone());
    let before = event_list_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().event_panel;
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
    let initial_scroll = view.event_scroll();
    let (before_x, before_y) = rendered_cell(&before, "Event 10");
    for _ in 0..view.event_panel.len() {
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
        if view.event_scroll() < initial_scroll {
            break;
        }
    }
    assert_eq!(view.event_scroll(), initial_scroll - 1);
    let after = event_list_screen(&view, 100, 25, now);
    assert!(!after.contains("Event 11"), "{after}");
    let (after_x, after_y) = rendered_cell(&after, "Event 10");
    assert_eq!(after_y, before_y + 1);
    assert_eq!(after_x, before_x);
}

#[test]
fn mouse_event_wheel_selects_one_event_per_report() {
    let (view, now) = event_fixture();
    event_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().event_panel;
    let mut view = view;
    view.handle_mouse(mouse(
        MouseEventKind::ScrollUp,
        rect.x + 2,
        rect.y + super::event_list::EVENT_TOP,
    ));
    assert_eq!(
        view.visible_event(),
        view.event_panel.get(view.event_panel.len() - 2)
    );
    assert_eq!(view.event_scroll(), view.event_max_scroll());
    view.handle_mouse(mouse(
        MouseEventKind::ScrollDown,
        rect.x + 2,
        rect.y + super::event_list::EVENT_TOP,
    ));
    assert_eq!(view.event_scroll(), view.event_max_scroll());
    assert_eq!(view.visible_event(), view.event_panel.last());
}

#[test]
fn mouse_event_wheel_down_at_newest_stays_at_newest() {
    let (mut view, now) = event_fixture();
    let before = event_screen(&view, 100, 25, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 25)).event_panel;
    view.handle_mouse(mouse(
        MouseEventKind::ScrollDown,
        rect.x + 2,
        rect.y + super::event_list::EVENT_TOP,
    ));
    assert_eq!(before, event_screen(&view, 100, 25, now));
}

fn with_newer_event(view: &View, now: chrono::DateTime<Utc>) -> Vec<crate::event::Event> {
    let mut events = view.event_panel.clone();
    let mut event = events.last().unwrap().clone();
    event.body = "brand new event".into();
    event.started_at = now - Duration::seconds(5);
    event.previous_end = Some(now - Duration::seconds(10));
    events.push(event);
    events
}

#[test]
fn new_events_keep_rows_still_while_mouse_is_held() {
    let (mut view, now) = event_fixture();
    let before = event_list_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().event_panel;
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 20,
        rect.y + 2,
    ));
    view.set_event(with_newer_event(&view, now));
    let after = event_list_screen(&view, 100, 25, now);
    assert!(!after.contains("brand new event"), "{after}");
    for body in ["run tests", "run a background job"] {
        assert_eq!(rendered_cell(&after, body), rendered_cell(&before, body));
    }
}

#[test]
fn new_events_while_dragging_copy_the_rows_on_screen() {
    let (mut view, now) = event_fixture();
    let before = event_screen(&view, 100, 25, now);
    let end_x =
        rendered_cell(&before, "run a background job").0 + "run a background job".len() as u16;
    let (x, y) = rendered_cell(&before, "failed check");
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    view.set_event(with_newer_event(&view, now));
    event_screen(&view, 100, 25, now);
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end_x, y + 2));
    event_screen(&view, 100, 25, now);
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
fn new_events_keep_rows_still_off_the_newest_row() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().event_panel;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
    let before = event_list_screen(&view, 100, 25, now);
    assert!(!before.contains("↓ Latest"), "{before}");
    view.set_event(with_newer_event(&view, now));
    let after = event_list_screen(&view, 100, 25, now);
    assert!(!after.contains("brand new event"), "{after}");
    let body = rendered_cell(&after, "run a background job");
    assert_eq!(body, rendered_cell(&before, "run a background job"));
    let jump = rendered_cell(&after, "↓ 1 New");
    let label = " ↓ 1 New".width() as u16;
    assert_eq!((jump.0, jump.1), (rect.right() - label, rect.bottom()));
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(Rect::new(0, 0, 100, 25), &mut buf, now);
    assert_eq!(buf[jump].fg, Color::Reset);
    assert_eq!(buf[jump].modifier, ratatui::style::Modifier::DIM);
}

/// The event list fixture with a second task, and the row index of each.
fn two_task_event_fixture() -> (View, chrono::DateTime<Utc>, usize, usize) {
    let (mut view, now) = event_fixture();
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
    let (event_panel, other) = (index(&view, "log"), index(&view, "other"));
    view.set_selected(event_panel);
    (view, now, event_panel, other)
}

/// Selects the task at `index` and loads `events` as its event list.
fn show_task_event(view: &mut View, index: usize, events: Vec<crate::event::Event>) {
    view.set_selected(index);
    view.set_event(events);
    view.event_session = view.selected_id().cloned();
}

#[test]
fn switching_tasks_resets_event_to_newest_and_clears_unseen() {
    let (mut view, now, event_panel, other) = two_task_event_fixture();
    event_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().event_panel;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y));
    assert_eq!(
        view.visible_event(),
        view.event_panel.get(view.event_panel.len() - 2)
    );
    let newer = with_newer_event(&view, now);
    show_task_event(&mut view, other, Vec::new());
    assert_eq!(view.event_scroll(), 0);
    assert!(view.event_list.cursor.is_none());
    show_task_event(&mut view, event_panel, newer);
    assert_eq!(view.event_scroll(), view.event_max_scroll());
    assert_eq!(view.visible_event(), view.event_panel.last());
    let after = event_screen(&view, 100, 25, now);
    assert!(after.contains("brand new event"), "{after}");
    assert!(!after.contains("↓ 1 New"), "{after}");
    assert!(!after.contains("↓ Latest"), "{after}");
}

#[test]
fn task_search_returns_but_selected_detail_resets_to_newest() {
    let (mut view, now, event_panel, other) = two_task_event_fixture();
    view.event_panel[2].body = (1..=40)
        .map(|i| format!("Reply line {i}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let events = view.event_panel.clone();
    {
        view.active_panel = Panel::EventList;
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
    };
    for letter in "tool".chars() {
        key(&mut view, KeyCode::Char(letter));
    }
    view.select_fixture_event(Some(events[2].clone()));
    view.event_detail.scroll.borrow_mut().offset = 5;
    event_screen(&view, 100, 25, now);
    show_task_event(&mut view, other, Vec::new());
    assert!(!view.event_list.search.active());
    assert!(view.event_list.cursor.is_none());
    show_task_event(&mut view, event_panel, events);
    assert_eq!(view.event_list.search.query.text(), "tool");
    assert_eq!(view.visible_event(), view.event_panel.last());
    assert_eq!(view.event_detail.scroll.borrow().offset, 0);
    assert_eq!(view.event_scroll(), 0);
    assert!(event_screen(&view, 100, 25, now).contains("run tests"));
}

#[test]
fn jump_button_returns_to_the_newest_event_row() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().event_panel;
    view.handle_mouse(mouse(
        MouseEventKind::ScrollUp,
        rect.x + 2,
        rect.y + super::event_list::EVENT_TOP,
    ));
    view.set_event(with_newer_event(&view, now));
    let scrolled = event_screen(&view, 100, 25, now);
    let (x, y) = rendered_cell(&scrolled, "↓ 1 New");
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y));
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 2, y));
    assert_eq!(view.event_scroll(), view.event_max_scroll());
    let after = event_screen(&view, 100, 25, now);
    assert!(after.contains("brand new event"), "{after}");
    assert!(
        !after.lines().nth(usize::from(y)).unwrap().contains('↑'),
        "{after}"
    );
}

#[test]
fn mouse_event_selection_tracks_selected_session_identity() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().event_panel;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    let selected = view.event_list.cursor;
    assert_eq!(
        view.visible_event(),
        view.event_panel.get(view.event_panel.len() - 2)
    );
    let id = view.selected_id().unwrap().to_owned();
    let mut rows = view.rows.clone();
    rows.push(session("recent", "Recent", SessionStatus::Working, -7200));
    view.set_sessions(rows.clone(), false);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(view.event_list.cursor, selected);
    rows.retain(|row| row.id != id);
    for row in &mut rows {
        row.transcript = Some(row.cwd.join("session.jsonl"));
    }
    view.set_sessions(rows, false);
    assert_eq!(view.event_scroll(), 0);
    assert!(view.event_list.cursor.is_none());
    view.set_event(view.event_panel.clone());
    assert_eq!(view.visible_event(), view.event_panel.last());
}

#[test]
fn mouse_task_details_wheel_scrolls_three_rows_per_report() {
    let mut view = overflowing_task_details();
    task_details_panel(&view);
    let rect = view.viewport.borrow().task_details;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    let revealed = |view: &View| {
        view.viewport
            .borrow()
            .task_details_scroll
            .reveal
            .iter()
            .sum::<usize>()
    };
    assert_eq!(revealed(&view), 3);
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    assert_eq!(revealed(&view), 0);
}

#[test]
fn mouse_task_details_scroll_resets_with_selection_and_stays_across_tabs() {
    for code in [KeyCode::Down, KeyCode::End, KeyCode::PageDown, KeyCode::Tab] {
        let mut view = overflowing_task_details();
        task_details_panel(&view);
        let rect = view.viewport.borrow().task_details;
        view.scroll_content_row(true, false, rect.y);
        assert_eq!(
            view.viewport
                .borrow()
                .task_details_scroll
                .reveal
                .iter()
                .sum::<usize>(),
            1
        );
        key(&mut view, code);
        assert_eq!(
            view.viewport
                .borrow()
                .task_details_scroll
                .reveal
                .iter()
                .sum::<usize>(),
            0,
            "{code:?}"
        );
    }
    let mut view = overflowing_task_details();
    task_details_panel(&view);
    let rect = view.viewport.borrow().task_details;
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
            .task_details_scroll
            .reveal
            .iter()
            .sum::<usize>(),
        3
    );
}

fn task_details_reveal(view: &View) -> Vec<usize> {
    view.viewport.borrow().task_details_scroll.reveal.clone()
}

#[test]
fn task_details_scroll_returns_with_its_task() {
    let mut view = overflowing_task_details();
    task_details_panel(&view);
    let rect = view.viewport.borrow().task_details;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    let scrolled = task_details_reveal(&view);
    assert_eq!(scrolled.iter().sum::<usize>(), 3);
    let before = task_details_panel(&view);
    view.set_active_panel(Panel::TaskList);
    key(&mut view, KeyCode::Down);
    task_details_panel(&view);
    assert_eq!(task_details_reveal(&view).iter().sum::<usize>(), 0);
    key(&mut view, KeyCode::Up);
    assert_eq!(task_details_panel(&view), before);
    assert_eq!(task_details_reveal(&view), scrolled);
}

#[test]
fn task_details_body_changed_while_away_opens_folded() {
    let mut view = overflowing_task_details();
    view.rows[view.selected].last_message = Some(
        (1..=12)
            .map(|i| format!("Reply row {i}"))
            .collect::<Vec<_>>()
            .join("\n\n"),
    );
    task_details_panel(&view);
    let rect = view.viewport.borrow().task_details;
    for _ in 0..8 {
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
        task_details_panel(&view);
    }
    let scrolled = task_details_reveal(&view);
    let prompt = 1;
    assert!(scrolled[prompt] > 0 && scrolled[0] > 0, "{scrolled:?}");
    let index = view.selected;
    view.set_active_panel(Panel::TaskList);
    key(&mut view, KeyCode::Down);
    task_details_panel(&view);
    view.rows[index].prompt = "A changed prompt that wraps across several rows. ".repeat(8);
    key(&mut view, KeyCode::Up);
    task_details_panel(&view);
    let mut expected = scrolled;
    expected[prompt] = 0;
    assert_eq!(task_details_reveal(&view), expected);
}

#[test]
fn task_details_scroll_preserves_background_cells_and_hyperlinks() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "Background output").unwrap();
    let mut view = overflowing_task_details();
    let selected = &mut view.rows[view.selected];
    selected.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: Some(output.path().into()),
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
    let body = viewport.task_details;
    let output_y = (body.y..body.bottom())
        .find(|&y| {
            (body.x..body.right())
                .map(|x| cell_text(before[(x, y)].symbol()))
                .collect::<String>()
                .contains("Background output")
        })
        .unwrap();
    let (pr, _) = viewport
        .task_details_links
        .iter()
        .find(|(_, path)| path == &PathBuf::from("https://example.com/pull/833"))
        .unwrap();
    let pr = *pr;
    assert!(cell_text(before[(pr.x, pr.y)].symbol()).contains("https://example.com/pull/833"));
    assert!(matches!(
        before[(pr.x, pr.y)].diff_option,
        ratatui::buffer::CellDiffOption::ForcedWidth(_)
    ));
    let prompt_index = 0;
    let prompt = viewport.task_details_scroll.bodies[prompt_index].start as u16 + body.y;
    assert!(viewport.task_details_scroll.hidden[prompt_index] > 0);
    drop(viewport);
    view.scroll_content_row(true, false, prompt);
    let mut after = Buffer::empty(area);
    view.render_at(area, &mut after, now);
    assert!(view.viewport.borrow().task_details_scroll.reveal[prompt_index] > 0);
    let after_y = (body.y..body.bottom())
        .find(|&y| {
            (body.x..body.right())
                .map(|x| cell_text(after[(x, y)].symbol()))
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
fn task_details_scroll_tracks_session_identity_across_refreshes() {
    let mut view = overflowing_task_details();
    task_details_panel(&view);
    let rect = view.viewport.borrow().task_details;
    view.scroll_content_row(true, false, rect.y);
    let id = view.selected_id().unwrap().to_owned();
    let mut rows = view.rows.clone();
    rows.push(session("recent", "Recent", SessionStatus::Working, -7200));
    view.set_sessions(rows.clone(), false);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(
        view.viewport
            .borrow()
            .task_details_scroll
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
            .task_details_scroll
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
    assert!(!view.prompt_focused());
    assert_eq!(view.selected_id(), Some(&id));
}

#[test]
fn mouse_list_slow_or_split_clicks_only_select() {
    let mut view = fixture();
    let rendered = screen(&view, 160, 40);
    let list = view.center_layout(Rect::new(0, 0, 160, 40)).list;
    let [first, second] = [0, 1].map(|i| view.selectable_indices()[i]);
    let [first_click, second_click] = [first, second].map(|row| {
        let (x, y) = rendered_cell(
            &rendered
                .lines()
                .map(|line| {
                    line.chars()
                        .take(usize::from(list.right()))
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n"),
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
                .trim()
                == "project"
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
    assert!(!view.prompt_focused());
    assert_eq!(view.selected_id(), Some(&view.rows[target].id));
}

#[test]
fn mouse_list_ignores_modals_but_saves_title_on_blur() {
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
        screen(&view, 160, 40);
        let selected = view.selected;
        if overlay == KeyCode::Char('r') {
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, y),
            );
            assert!(view.rename_target.is_none());
            assert_eq!(view.selected, second);
            continue;
        }
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
        cwd: crate::test_project(),
        name: None,
        title: title.into(),
        prompt: title.into(),
        task_details_sources: Default::default(),
        prompt_images: Vec::new(),
        queued: Vec::new(),
        prompt_at: None,
        turn_messages: 0,
        last_record_at: None,
        cache_expires_at: None,
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
                .map(|cell| cell_text(cell.symbol()))
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
    view.set_pinned(&crate::state::TaskPins {
        active: vec![session_key("42"), session_key("44")],
        inactive: vec![],
    });
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
    if std::env::var_os("UPDATE_HELP_SNAPSHOTS").is_some() {
        std::fs::write(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/ui_snapshots/live_center_help_two_columns.txt"
            ),
            screen(&view, 60, 24),
        )
        .unwrap();
    }
    assert_eq!(
        screen(&view, 60, 24),
        include_str!("../../tests/ui_snapshots/live_center_help_two_columns.txt")
    );
    // Both widths fit the task tabs and shortcut columns.
    let narrow = screen(&view, 60, 24);
    let wide = screen(&view, 62, 24);
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
fn tab_switches_filter_without_entering_a_revisited_task() {
    assert_tab_switches_filter_without_entering_a_revisited_task(KeyCode::Tab);
}

#[test]
fn backtab_switches_filter_without_entering_a_revisited_task() {
    assert_tab_switches_filter_without_entering_a_revisited_task(KeyCode::BackTab);
}

fn assert_tab_switches_filter_without_entering_a_revisited_task(code: KeyCode) {
    let mut view = fixture();
    key(&mut view, KeyCode::Home);
    let selected = view.selected_id().cloned();
    view.set_active_panel(Panel::Prompt);
    view.set_active_panel(Panel::TaskList);
    key(&mut view, KeyCode::Char('j'));
    assert_ne!(view.selected_id(), selected.as_ref());
    key(&mut view, KeyCode::Char('k'));
    assert_eq!(view.selected_id(), selected.as_ref());
    assert!(view.task_entry_pending);
    assert_eq!(view.active_panel, Panel::TaskList);
    screen(&view, 120, 40);
    let filter = view.status_filter;

    assert_eq!(key(&mut view, code), None);

    assert_eq!(view.status_filter, 1 - filter);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(screen(&view, 120, 40).contains("Inactive 1"));
}

#[test]
fn refresh_keeps_active_when_selected_task_becomes_inactive() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        for status in [
            SessionStatus::Working,
            SessionStatus::Ready,
            SessionStatus::NeedsInput,
        ] {
            for renaming in [false, true] {
                let mut view = test_view(Grouping::Project, false);
                view.capabilities
                    .insert(provider, view.capabilities[&ProviderId::Claude]);
                let mut task = session("a", "Selected task", status, 120);
                task.id.provider = provider;
                let other = session("other", "Other active task", SessionStatus::Ready, 60);
                view.set_sessions(vec![task.clone(), other.clone()], false);
                view.select(view.rows.iter().position(|row| row.id == task.id).unwrap());
                if renaming {
                    key(&mut view, KeyCode::Char('r'));
                    view.set_active_panel(Panel::TaskDetails);
                    assert_eq!(view.rename_target.as_ref(), Some(&task.id));
                }
                task.status = SessionStatus::Inactive;

                view.set_sessions(vec![task, other.clone()], false);

                assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
                assert_eq!(view.selected_id(), Some(&other.id));
                assert_eq!(view.selected, view.fallback_selection());
                assert!(view.rename_target.is_none());
                assert!(!screen(&view, 120, 24).contains("Rename ❯"));
            }
        }
    }
}

#[test]
fn refresh_follows_selected_inactive_task_to_active() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        for status in [
            SessionStatus::Working,
            SessionStatus::Ready,
            SessionStatus::NeedsInput,
        ] {
            let mut view = test_view(Grouping::Project, false);
            let mut task = session("b", "Selected task", SessionStatus::Inactive, 120);
            task.id.provider = provider;
            let other = session("other", "Other active task", SessionStatus::Ready, 60);
            view.set_sessions(vec![task.clone(), other.clone()], false);
            key(&mut view, KeyCode::Tab);
            view.select(view.rows.iter().position(|row| row.id == task.id).unwrap());
            task.status = status;

            view.set_sessions(vec![task.clone(), other], false);

            assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
            assert_eq!(view.selected_id(), Some(&task.id));
            assert_eq!(view.tab_selections[0], Some(task.id.clone()));
            key(&mut view, KeyCode::Tab);
            key(&mut view, KeyCode::Tab);
            assert_eq!(view.selected_id(), Some(&task.id));
        }
    }
}

#[test]
fn needs_input_preempts_and_returns_to_selected_task() {
    for status in [SessionStatus::Working, SessionStatus::Inactive] {
        let mut view = test_view(Grouping::Project, false);
        let original = session("original", "Original", status, 300);
        let mut waiting = session("waiting", "Waiting", SessionStatus::Working, 60);
        view.set_sessions(vec![original.clone(), waiting.clone()], false);
        if status == SessionStatus::Inactive {
            key(&mut view, KeyCode::Tab);
        }
        view.select(
            view.rows
                .iter()
                .position(|row| row.id == original.id)
                .unwrap(),
        );

        waiting.status = SessionStatus::NeedsInput;
        view.set_sessions(vec![original.clone(), waiting.clone()], false);
        assert_eq!(view.selected_id(), Some(&waiting.id));
        assert_eq!(view.status_filter, 0);
        assert!(screen(&view, 120, 24).contains("Waiting"));

        waiting.status = SessionStatus::Working;
        view.set_sessions(vec![original.clone(), waiting], false);
        assert_eq!(view.selected_id(), Some(&original.id));
        assert_eq!(view.status_filter, tab_for_status(status));
    }
}

#[test]
fn needs_input_queue_handles_longest_wait_first_without_interrupting() {
    let mut view = test_view(Grouping::Project, false);
    let original = session("original", "Original", SessionStatus::Ready, 300);
    let mut first = session("first", "First", SessionStatus::Working, 120);
    let mut second = session("second", "Second", SessionStatus::Working, 60);
    let mut arriving = session("arriving", "Arriving", SessionStatus::Working, 600);
    view.set_sessions(
        vec![
            original.clone(),
            first.clone(),
            second.clone(),
            arriving.clone(),
        ],
        false,
    );
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );

    first.status = SessionStatus::NeedsInput;
    second.status = SessionStatus::NeedsInput;
    view.set_sessions(
        vec![
            original.clone(),
            second.clone(),
            first.clone(),
            arriving.clone(),
        ],
        false,
    );
    assert_eq!(view.selected_id(), Some(&first.id));

    arriving.status = SessionStatus::NeedsInput;
    view.set_sessions(
        vec![
            original.clone(),
            second.clone(),
            first.clone(),
            arriving.clone(),
        ],
        false,
    );
    assert_eq!(view.selected_id(), Some(&first.id));

    first.status = SessionStatus::Working;
    view.set_sessions(
        vec![
            original.clone(),
            second.clone(),
            first.clone(),
            arriving.clone(),
        ],
        false,
    );
    assert_eq!(view.selected_id(), Some(&arriving.id));
    arriving.status = SessionStatus::Working;
    view.set_sessions(
        vec![
            original.clone(),
            second.clone(),
            first.clone(),
            arriving.clone(),
        ],
        false,
    );
    assert_eq!(view.selected_id(), Some(&second.id));
    second.status = SessionStatus::Working;
    view.set_sessions(vec![original.clone(), second, first, arriving], false);
    assert_eq!(view.selected_id(), Some(&original.id));
}

#[test]
fn needs_input_queue_continues_after_sending_and_returns_to_original_tab() {
    let mut view = test_view(Grouping::Project, false);
    let original = session("original", "Original", SessionStatus::Inactive, 300);
    let mut first = session("first", "First", SessionStatus::Working, 120);
    let mut second = session("second", "Second", SessionStatus::Working, 60);
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    key(&mut view, KeyCode::Tab);
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );

    first.status = SessionStatus::NeedsInput;
    second.status = SessionStatus::NeedsInput;
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(view.selected_id(), Some(&first.id));

    assert_eq!(view.status_filter, 0);

    let input = crate::providers::SessionInput::Prompt("continue".into());
    view.input_sending(&first.id, &input);
    first.status = SessionStatus::Working;
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(view.selected_id(), Some(&second.id));

    view.input_sending(&second.id, &input);
    second.status = SessionStatus::Working;
    view.set_sessions(vec![original.clone(), first, second], false);
    assert_eq!(view.selected_id(), Some(&original.id));
    assert_eq!(view.status_filter, tab_for_status(original.status));
    assert!(view.attention_selection.is_none());
    assert!(view.interrupted_selection.is_none());
}

#[test]
fn needs_input_task_left_waiting_stays_behind_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut view = test_view(Grouping::Project, false);
    let original = session("original", "Original", SessionStatus::Ready, 300);
    let mut first = session("first", "First", SessionStatus::Working, 120);
    let mut second = session("second", "Second", SessionStatus::Working, 60);
    let select = |view: &mut View, id: &SessionKey| {
        view.select(view.rows.iter().position(|row| &row.id == id).unwrap());
    };
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    select(&mut view, &original.id);
    first.status = SessionStatus::NeedsInput;
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(view.selected_id(), Some(&first.id));
    select(&mut view, &original.id);
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);

    let mut preferences = crate::state::Preferences::default();
    let mut actions = vec![view.take_pending_action().unwrap()];
    while let Some(action) = actions.pop() {
        match action {
            Action::Batch(batch) => actions.extend(batch),
            Action::PersistSkip { id, skipped_at } => preferences
                .save_task_skipped_at(dir.path(), &id, skipped_at)
                .unwrap(),
            _ => {}
        }
    }
    let mut restarted = test_view(Grouping::Project, false);
    restarted.restore_attention_skips(&crate::state::Preferences::load(dir.path()));
    restarted.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(restarted.selected_id(), Some(&original.id));
    second.status = SessionStatus::NeedsInput;
    second.updated_at = Utc::now() + Duration::seconds(1);
    restarted.set_sessions(vec![original, first, second.clone()], false);
    assert_eq!(restarted.selected_id(), Some(&second.id));
}

#[test]
fn needs_input_task_left_waiting_queues_behind_tasks_that_start_waiting_later() {
    let mut view = test_view(Grouping::Project, false);
    let original = session("original", "Original", SessionStatus::Ready, 300);
    let mut first = session("first", "First", SessionStatus::Working, 120);
    let mut second = session("second", "Second", SessionStatus::Working, 60);
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );
    first.status = SessionStatus::NeedsInput;
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(view.selected_id(), Some(&first.id));

    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(view.selected_id(), Some(&original.id));
    second.status = SessionStatus::NeedsInput;
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(view.selected_id(), Some(&second.id));
    view.input_sending(
        &second.id,
        &crate::providers::SessionInput::Prompt("continue".into()),
    );
    second.status = SessionStatus::Working;
    view.set_sessions(vec![original.clone(), first.clone(), second.clone()], false);
    assert_eq!(view.selected_id(), Some(&first.id));
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );
    first.status = SessionStatus::Working;
    view.set_sessions(vec![original.clone(), first, second], false);
    assert_eq!(view.selected_id(), Some(&original.id));
}

#[test]
fn needs_input_tasks_lead_their_group_and_siblings_in_queue_order() {
    for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
        let mut view = test_view(grouping, false);
        let mut other = session("other", "Other", SessionStatus::NeedsInput, 900);
        other.cwd = "/tmp/another".into();
        let ready = session("ready", "Ready", SessionStatus::Ready, 0);
        let newer = session("newer", "Newer", SessionStatus::NeedsInput, 60);
        let older = session("older", "Older", SessionStatus::NeedsInput, 300);
        let left = session("left", "Left", SessionStatus::NeedsInput, 600);
        let child = |id: &str, status, age| {
            let mut child = session(id, id, status, age);
            child.parent = Some(ready.id.clone());
            child
        };
        let busy_child = child("busy-child", SessionStatus::Working, 0);
        let newer_child = child("newer-child", SessionStatus::NeedsInput, 60);
        let older_child = child("older-child", SessionStatus::NeedsInput, 300);
        view.attention_skips
            .insert(left.id.clone(), Utc::now() - Duration::seconds(30));
        view.set_sessions(
            vec![
                other,
                ready,
                newer,
                older,
                left,
                busy_child,
                newer_child,
                older_child,
            ],
            false,
        );

        let order: Vec<_> = view
            .visible_indices()
            .into_iter()
            .map(|index| view.rows[index].id.id.clone())
            .collect();
        let project = [
            "older",
            "newer",
            "left",
            "ready",
            "older-child",
            "newer-child",
            "busy-child",
        ];
        let expected: Vec<_> = match grouping {
            Grouping::Project => ["other"].into_iter().chain(project).collect(),
            _ => [
                "other",
                "older",
                "newer",
                "left",
                "ready",
                "older-child",
                "newer-child",
                "busy-child",
            ]
            .into(),
        };
        assert_eq!(order, expected, "{grouping:?}");
    }
}

#[test]
fn needs_input_queue_uses_question_time_before_session_update() {
    let payload: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/claude-ask-user-question/post.json"
    ))
    .unwrap();
    let mut question = session("question", "Question", SessionStatus::NeedsInput, 0);
    let since = question.updated_at;
    question.ask = Some(crate::ask::Ask {
        id: payload["tool_use_id"].as_str().unwrap().into(),
        session: question.id.clone(),
        since,
        kind: crate::ask::AskKind::Questions {
            blocking: true,
            questions: payload["tool_input"]["questions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|raw| crate::ask::Question {
                    id: raw["question"].as_str().unwrap().into(),
                    text: raw["question"].as_str().unwrap().into(),
                    header: raw["header"].as_str().unwrap().into(),
                    multi: raw["multiSelect"].as_bool().unwrap(),
                    other: true,
                    secret: false,
                    options: serde_json::from_value(raw["options"].clone()).unwrap(),
                })
                .collect(),
        },
    });
    question.status = SessionStatus::Working;
    question.updated_at = since + Duration::seconds(120);
    let original = session("original", "Original", SessionStatus::Ready, 300);
    let mut permission = session("permission", "Permission", SessionStatus::Working, 60);
    permission.updated_at = since + Duration::seconds(60);
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(
        vec![original.clone(), permission.clone(), question.clone()],
        false,
    );
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );

    question.status = SessionStatus::NeedsInput;
    permission.status = SessionStatus::NeedsInput;
    view.set_sessions(vec![original, permission, question.clone()], false);
    assert_eq!(view.selected_id(), Some(&question.id));
}

#[test]
fn needs_input_skips_children_and_search_mismatches() {
    for excluded in ["child", "search"] {
        let mut view = test_view(Grouping::Project, false);
        let original = session("original", "Visible original", SessionStatus::Ready, 300);
        let mut waiting = session("waiting", "Visible waiting", SessionStatus::Working, 60);
        match excluded {
            "child" => waiting.parent = Some(original.id.clone()),
            "search" => {
                view.search.set_text("original");
            }
            _ => unreachable!(),
        }
        view.set_sessions(vec![original.clone(), waiting.clone()], false);
        waiting.status = SessionStatus::NeedsInput;
        view.set_sessions(vec![original.clone(), waiting], false);
        assert_eq!(view.selected_id(), Some(&original.id), "{excluded}");
        assert!(view.attention_selection.is_none());
    }
}

#[test]
fn needs_input_does_not_interrupt_draft_or_manually_selected_waiting_task() {
    for draft in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut view = test_view(Grouping::Project, false);
        let current = session("current", "Current", SessionStatus::NeedsInput, 300);
        let mut arriving = session("arriving", "Arriving", SessionStatus::Working, 600);
        view.set_sessions(vec![current.clone()], false);
        view.select(
            view.rows
                .iter()
                .position(|row| row.id == current.id)
                .unwrap(),
        );
        let selected_draft = draft.then(|| {
            view.add_draft(DraftSetup {
                provider: ProviderId::Claude,
                cwd: directory.path().into(),
                model: None,
                effort: None,
                branch: String::new(),
            })
        });
        arriving.status = SessionStatus::NeedsInput;
        view.set_sessions(vec![current.clone(), arriving.clone()], false);
        assert_eq!(view.selected_draft, selected_draft);
        if !draft {
            assert_eq!(view.selected_id(), Some(&current.id));
        }
        assert!(view.attention_selection.is_none());
        assert!(view.waiting_sessions.contains(&arriving.id));
    }
}

#[test]
fn needs_input_missing_interrupted_task_uses_fallback() {
    let mut view = test_view(Grouping::Project, false);
    let original = session("original", "Original", SessionStatus::Ready, 300);
    let fallback = session("fallback", "Fallback", SessionStatus::Ready, 240);
    let mut waiting = session("waiting", "Waiting", SessionStatus::Working, 60);
    view.set_sessions(
        vec![original.clone(), fallback.clone(), waiting.clone()],
        false,
    );
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );
    waiting.status = SessionStatus::NeedsInput;
    view.set_sessions(vec![original, fallback.clone(), waiting.clone()], false);
    assert_eq!(view.selected_id(), Some(&waiting.id));

    waiting.status = SessionStatus::Working;
    view.set_sessions(vec![fallback.clone(), waiting], false);
    assert_eq!(view.selected_id(), Some(&fallback.id));
    assert_eq!(view.selected, view.fallback_selection());
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
fn refresh_keeps_active_when_selected_ready_session_becomes_inactive() {
    let mut view = fixture();
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Inactive;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some(&session_key("46")));
    key(&mut view, KeyCode::Tab);
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_id(), Some(&session_key("46")));
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
                    let expected = if parent_visible {
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
        screen(&view, 100, 16),
        include_str!("../../tests/ui_snapshots/live_center_search_active.txt")
    );
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 100, 16)), Some((14, 11)));
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
fn inactive_without_its_chosen_task_selects_first_visible_row() {
    let mut view = fixture();
    view.set_sessions(view.rows.clone(), true);

    key(&mut view, KeyCode::BackTab);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    assert_eq!(view.selected, view.visible_indices()[0]);
}

#[test]
fn inactive_defaults_to_top_of_task_tree_and_remembers_user_selection() {
    for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
        for has_more in [false, true] {
            let mut view = test_view(grouping, false);
            let parent = session("parent", "Live parent", SessionStatus::Working, 10);
            let mut child = session("child", "Finished child", SessionStatus::Inactive, 600);
            child.parent = Some(parent.id.clone());
            let other = session("other", "Other finished task", SessionStatus::Inactive, 300);
            let rows = vec![other.clone(), child.clone(), parent.clone()];
            view.set_sessions(rows.clone(), has_more);
            view.viewport.get_mut().scroll = 20;

            key(&mut view, KeyCode::Tab);

            let visible = view.visible_indices();
            assert_eq!(visible.len(), 3);
            assert_eq!(view.selected, visible[0]);
            assert_eq!(view.viewport.borrow().scroll, 0);
            screen(&view, 120, 24);
            assert_eq!(view.viewport.borrow().scroll, 0);

            view.select(visible[1]);
            let chosen = view.selected_id().cloned();
            key(&mut view, KeyCode::BackTab);
            key(&mut view, KeyCode::Tab);
            assert_eq!(view.selected_id(), chosen.as_ref());

            // A remembered task that disappears must also fall back to the top.
            key(&mut view, KeyCode::BackTab);
            view.set_sessions(
                rows.into_iter()
                    .filter(|row| Some(&row.id) != chosen.as_ref())
                    .collect(),
                has_more,
            );
            key(&mut view, KeyCode::Tab);
            assert_eq!(view.selected, view.visible_indices()[0]);
            assert_eq!(view.viewport.borrow().scroll, 0);
        }
    }
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
    key(&mut view, KeyCode::Tab);
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Inactive");
    key(&mut view, KeyCode::Home);
    let max_steps = view.rows.len();
    let mut steps = 0;
    while view.selected_id() != Some(&session_key("45")) {
        assert!(
            steps < max_steps,
            "task 45 not reached after {steps} Down events: selected index {}, selected id {:?}",
            view.selected,
            view.selected_id()
        );
        key(&mut view, KeyCode::Down);
        steps += 1;
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
        screen(&view, 100, 12),
        include_str!("../../tests/ui_snapshots/live_center_show_more.txt")
    );
    assert_eq!(key(&mut view, KeyCode::Enter), Some(Action::ShowMore));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    let rendered = screen(&view, 100, 12);
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
                    view.set_pinned(&crate::state::TaskPins {
                        active: vec![session_key("ready"), session_key("working")],
                        inactive: vec![],
                    });
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
                        "Pinned".to_owned()
                    } else {
                        match grouping {
                            Grouping::Project => "project".to_owned(),
                            Grouping::Status => view.rows[*index].status.label().to_owned(),
                            Grouping::Model => view.session_model_text(&view.rows[*index]).unwrap(),
                        }
                    };
                    let heading = (0..area.width)
                        .map(|x| cell_text(buf[(x, y as u16)].symbol()))
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
    let rendered = screen(&view, 100, 60);
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
    assert_eq!(key(&mut view, KeyCode::Esc), None);
}
#[test]
fn task_list_actions() {
    let mut view = fixture();
    assert_eq!(key(&mut view, KeyCode::Char('o')), None);
    assert_eq!(view.active_panel, Panel::Prompt);
    view.set_active_panel(Panel::TaskList);
    assert_eq!(
        key(&mut view, KeyCode::Char('f')),
        Some(Action::Fork(session_key("42")))
    );
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.active_panel, Panel::Prompt);
    view.set_active_panel(Panel::TaskList);
    assert_eq!(
        key(&mut view, KeyCode::Char('x')),
        Some(Action::Unsupported("Stop"))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin {
            tab: 0,
            id: session_key("42"),
            pinned: false
        })
    );
    key(&mut view, KeyCode::Down);
    assert_eq!(view.selected_id(), Some(&session_key("44")));
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    view.set_active_panel(Panel::TaskDetails);
    assert!(!screen(&view, 100, 16).contains("Rename ❯"));
    assert_eq!(view.rename.text(), "Needs an answer");
    key(&mut view, KeyCode::Esc);
    view.set_active_panel(Panel::TaskList);

    assert_eq!(
        key(&mut view, KeyCode::Char('g')),
        Some(Action::PersistGrouping(Grouping::Status))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('n')),
        Some(Action::NewDraft {
            cwd: crate::test_project()
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
                    row.cwd = crate::test_project().with_file_name("second-project");
                }
                row
            })
            .collect(),
        true,
    );
    key(&mut view, KeyCode::Home);
    screen(&view, 110, 18);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), Some(&session_key("10")));
    key(&mut view, KeyCode::PageUp);
    assert_eq!(view.selected_id(), Some(&session_key("1")));
    key(&mut view, KeyCode::Home);
    assert_eq!(view.selected_id(), Some(&session_key("0")));
    for _ in 0..40 {
        key(&mut view, KeyCode::PageDown);
        screen(&view, 110, 18);
    }
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::Down);
    assert_eq!(view.selected_id(), None);
}
#[test]
fn styles_and_unicode_clipping() {
    super::highlight::use_theme(true, None).unwrap();
    use super::style::selection_style_for;
    assert_eq!(
        selection_style_for(false, true, true),
        Style::default()
            .bold()
            .fg(Color::Indexed(18))
            .bg(Color::Indexed(111))
    );
    super::highlight::use_theme(false, None).unwrap();
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
fn task_details_include_attention_and_clip_messages() {
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
        "Needs input",
        "Open task to review.",
        "Waiting for your response.",
        "Update",
    ] {
        assert!(rendered.contains(label), "{label}");
    }
    let position = |label: &str| {
        rendered.lines().position(|line| {
            line.split('│')
                .any(|section| section.trim_start().starts_with(label))
        })
    };
    assert!(position("Update") < position("Prompt"));
    assert!(position("Prompt") < position("Needs input"));
    let viewport = view.viewport.borrow();
    let folder = viewport
        .task_details_links
        .iter()
        .find(|(_, path)| path == &crate::test_project())
        .unwrap()
        .0;
    assert_eq!(folder.y, viewport.task_details.bottom());
    assert!(
        viewport
            .field_targets
            .iter()
            .any(|(_, field)| *field == Field::Model)
    );
    assert!(rendered.contains("main"));
    assert!(!rendered.contains("Reasoning"));
}

#[test]
fn empty_states_notices_and_help_only_handle_close_keys() {
    let mut view = test_view(Grouping::Project, true);
    assert_empty_task_list(&view);
    view.set_loading(true);
    assert_empty_task_list(&view);
    let mut alpha = session("a", "Alpha", SessionStatus::Ready, 1);
    // Search also matches the directory, so it must not contain the query.
    alpha.cwd = "/tmp/project".into();
    view.set_sessions(vec![alpha], false);
    key(&mut view, KeyCode::Char('/'));
    key(&mut view, KeyCode::Char('z'));
    assert_empty_task_list(&view);
    key(&mut view, KeyCode::Esc);
    view.show_error("Notice".into());
    let lines = screen(&view, 100, 12);
    assert!(
        lines.lines().last().unwrap().trim_end().ends_with("Notice"),
        "{lines}"
    );
    key(&mut view, KeyCode::Left);
    assert!(view.feedback.is_none());
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
    let area = Rect::new(0, 0, 100, 12);
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
fn task_details_name_a_missing_prompt() {
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
    view.set_active_panel(Panel::TaskDetails);
    let rendered = screen(&view, 100, 16);
    assert!(!rendered.contains("Rename ❯"));
    let title = view.viewport.borrow().task_details_title;
    assert_eq!(
        view.cursor_pos(Rect::new(0, 0, 100, 16)),
        Some((title.right() - 1, title.y))
    );
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
    assert!(view.cursor_pos(Rect::new(0, 0, 100, 16)).is_none());
}

#[test]
fn rename_locks_navigation_cancels_and_ignores_empty_enter() {
    let mut view = fixture();
    let hints = view.center_footer_hints().to_string();
    key(&mut view, KeyCode::Char('r'));
    view.set_active_panel(Panel::TaskDetails);
    assert_eq!(view.center_footer_hints().to_string(), "? help");
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
    assert!(!screen(&view, 100, 16).contains("Rename ❯"));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    key(&mut view, KeyCode::Esc);
    assert_eq!(view.center_footer_hints().to_string(), hints);
    assert_eq!(view.rows[view.selected].display_title(), "Current task");
    assert!(view.cursor_pos(Rect::new(0, 0, 100, 16)).is_none());
}

#[test]
fn rename_parks_inactive_target_and_drops_removed_target() {
    let mut view = fixture();
    view.status_filter = TASK_FILTERS
        .iter()
        .position(|(label, _)| *label == "Active")
        .unwrap();
    key(&mut view, KeyCode::Char('r'));
    view.set_active_panel(Panel::TaskDetails);
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some(&session_key("46")));
    assert!(!screen(&view, 100, 16).contains("Rename ❯"));
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert!(
        view.task_uis[&draft_controls::TaskKey::Session(session_key("42"))]
            .task_details
            .title
            .is_some()
    );
    let mut view = fixture();
    key(&mut view, KeyCode::Char('r'));
    view.set_active_panel(Panel::TaskDetails);
    view.set_sessions(vec![], true);
    assert!(view.cursor_pos(Rect::new(0, 0, 100, 16)).is_none());
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    view.set_active_panel(Panel::TaskDetails);
    assert!(view.cursor_pos(Rect::new(0, 0, 100, 16)).is_none());
}

#[test]
fn live_center_rename_active() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![session("42", "Task to find", SessionStatus::Ready, 120)],
        false,
    );
    key(&mut view, KeyCode::Char('r'));
    view.set_active_panel(Panel::TaskDetails);
    assert_eq!(
        screen(&view, 100, 16),
        include_str!("../../tests/ui_snapshots/live_center_rename_active.txt")
    );
    let area = Rect::new(0, 0, 100, 16);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    assert!(view.center_layout(area).search.is_empty());
    let title = view.viewport.borrow().task_details_title;
    assert_eq!(title.y, view.viewport.borrow().task_details.bottom());
    assert_eq!(view.cursor_pos(area), Some((title.right() - 1, title.y)));
}

#[test]
fn double_clicking_the_name_edits_on_the_rule_and_escape_keeps_the_title() {
    let mut view = test_view(Grouping::Project, true);
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            rename: true,
            ..Capabilities::default()
        },
    )]));
    view.set_sessions(
        vec![session("42", "Task to find", SessionStatus::Ready, 120)],
        false,
    );
    let area = Rect::new(0, 0, 100, 16);
    screen(&view, 100, 16);
    let title = view.viewport.borrow().task_details_title;
    let press = mouse(
        MouseEventKind::Down(MouseButton::Left),
        title.x + 2,
        title.y,
    );
    assert_eq!(click_mouse(&mut view, press), None);
    assert!(view.rename_target.is_none(), "single click must not rename");
    assert_eq!(click_mouse(&mut view, press), None);
    assert_eq!(view.rename_target, Some(session_key("42")));
    assert_eq!(view.rename.text(), "Task to find");
    let rendered = screen(&view, 100, 16);
    assert!(!rendered.contains("Rename ❯"));
    assert!(view.center_layout(area).search.is_empty());
    let title = view.viewport.borrow().title_input;
    assert_eq!(view.cursor_pos(area), Some((title.right() - 1, title.y)));
    view.handle_paste(" again");
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Rename {
            id: session_key("42"),
            name: "Task to find again".into()
        })
    );
    key(&mut view, KeyCode::Char('r'));
    view.set_active_panel(Panel::TaskDetails);
    view.handle_paste(" discarded");
    key(&mut view, KeyCode::Esc);
    assert!(view.rename_target.is_none());
    assert_eq!(view.selected_row().unwrap().display_title(), "Task to find");
    view.set_capabilities(HashMap::new());
    assert_eq!(click_mouse(&mut view, press), None);
    assert_eq!(
        click_mouse(&mut view, press),
        Some(Action::Unsupported("Rename"))
    );
}

#[test]
fn delete_dialog_defaults_to_cancel_and_requires_enter() {
    let mut view = fixture();
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(!view.confirm_dialog.as_ref().unwrap().confirm);
    let rendered = screen(&view, 120, 48);
    assert!(rendered.find("1. Cancel").unwrap() < rendered.find("2. Permanently delete").unwrap());
    assert_eq!(view.center_footer_hints().to_string(), "? help");
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
fn archive_directly_returns_action_without_a_dialog() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        let mut view = lifecycle_fixture();
        for row in &mut view.rows {
            row.id.provider = provider;
        }
        view.set_capabilities(HashMap::from([(
            provider,
            Capabilities {
                archive: true,
                ..Capabilities::default()
            },
        )]));
        let id = view.selected_id().unwrap().clone();
        assert_eq!(
            key(&mut view, KeyCode::Char('a')),
            Some(Action::Archive(id))
        );
        assert!(view.confirm_dialog.is_none());
        assert!(!screen(&view, 120, 48).contains("Archive “"));
    }
}

#[test]
fn delete_dialog_mouse_follows_visible_options() {
    for option in 0..2 {
        let mut view = fixture();
        key(&mut view, KeyCode::Backspace);
        let area = Rect::new(7, 3, 120, 48);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        let label = format!("{}.", option + 1);
        let (x, y) = (area.y..area.bottom())
            .flat_map(|y| (area.x..area.right() - 1).map(move |x| (x, y)))
            .find(|&(x, y)| {
                format!(
                    "{}{}",
                    cell_text(buf[(x, y)].symbol()),
                    cell_text(buf[(x + 1, y)].symbol())
                ) == label
            })
            .unwrap();
        let action = click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y),
        );
        let expected = if option == 1 {
            Some(Action::Delete(session_key("42")))
        } else {
            None
        };
        assert_eq!(action, expected);
        assert!(view.confirm_dialog.is_none());
    }
}

#[test]
fn delete_dialog_cancel_and_position_keys() {
    for code in [KeyCode::Char('1'), KeyCode::Esc] {
        let mut view = fixture();
        key(&mut view, KeyCode::Backspace);
        assert_eq!(key(&mut view, code), None);
        assert!(view.confirm_dialog.is_none());
    }
    for code in [KeyCode::End, KeyCode::PageDown] {
        let mut view = fixture();
        key(&mut view, KeyCode::Backspace);
        assert!(!view.confirm_dialog.as_ref().unwrap().confirm);
        key(&mut view, code);
        assert!(view.confirm_dialog.as_ref().unwrap().confirm);
        key(&mut view, KeyCode::Home);
        assert!(!view.confirm_dialog.as_ref().unwrap().confirm);
        key(&mut view, code);
        assert_eq!(
            key(&mut view, KeyCode::Enter),
            Some(Action::Delete(session_key("42")))
        );
    }
}

#[test]
fn escape_in_an_empty_prompt_stops_only_a_stoppable_task() {
    let mut view = lifecycle_fixture();
    view.set_active_panel(Panel::Prompt);
    view.prepare_prompt();
    view.input_box.editor.set_text("draft");
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert!(view.input_box.editor.is_empty());
    assert_eq!(
        key(&mut view, KeyCode::Esc),
        Some(Action::Stop(session_key("42")))
    );
    assert!(view.prompt_focused());
    let mut view = fixture();
    view.set_active_panel(Panel::Prompt);
    view.prepare_prompt();
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert!(view.prompt_focused());
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
        screen(&view, 100, 16),
        include_str!("../../tests/ui_snapshots/live_center_delete_dialog.txt")
    );
    let area = Rect::new(0, 0, 100, 16);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    assert!(
        buf[(12, 4)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(
        buf[(12, 6)]
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
    // A painted cell reports unset colours as Reset, so compare against a painted default cell.
    let mut expected = ratatui::buffer::Cell::default();
    expected.set_style(super::style::selection_style());
    assert_eq!(buf[(12, 8)].style(), expected.style());
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
        let right = if super::render::ScreenLayout::for_width(width)
            == super::render::ScreenLayout::Large
        {
            layout.event_panel.right()
        } else {
            layout.task_details.right()
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
                let line: String = (0..area.width)
                    .map(|x| cell_text(buf[(x, y)].symbol()))
                    .collect();
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
fn task_details_folder_and_pr_use_rule_styles_and_keep_links() {
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
        let links = viewport.task_details_links.clone();
        let y = viewport.task_details.bottom();
        drop(viewport);
        let (folder, _) = links
            .iter()
            .find(|(_, path)| path == &crate::test_project())
            .unwrap();
        let (pr, url) = links
            .iter()
            .find(|(_, path)| path != &crate::test_project())
            .unwrap();
        assert_eq!(folder.y, y);
        assert_eq!(pr.y, y);
        assert!(folder.right() < pr.x);
        for (offset, symbol) in "project".chars().enumerate() {
            let cell = &buf[(folder.x + offset as u16, y)];
            assert_eq!(cell_text(cell.symbol()), symbol.to_string());
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
                assert!(cell_text(cell.symbol()).contains(url.to_str().unwrap()));
            }
        }
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
            Some(Action::OpenPath(crate::test_project()))
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
fn event_model_and_effort_use_dim_style_and_picker_targets() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    for use_colors in [true, false] {
        for width in [120, 200] {
            for human_name in [None, Some("Opus 5.5")] {
                let mut row = session("model", "Model task", SessionStatus::Inactive, 120);
                row.model = Some("claude-opus-5-5".into());

                row.effort = Some("medium".into());
                row.transcript = Some("/tmp/model.jsonl".into());
                let mut view = test_view(Grouping::Project, use_colors);
                view.status_filter = 1;
                view.set_sessions(vec![row], false);
                if let Some(label) = human_name {
                    view.input_choices
                        .entry(ProviderId::Claude)
                        .or_default()
                        .display_names
                        .insert("claude-opus-5-5".into(), label.into());
                }
                let area = Rect::new(0, 0, width, 40);
                let mut buf = Buffer::empty(area);
                view.render(area, &mut buf);
                let layout = view.center_layout(area);
                let y = layout.list.bottom();
                let targets = view.viewport.borrow().field_targets.clone();
                for (field, text) in [
                    (Field::Model, human_name.unwrap_or("claude-opus-5-5")),
                    (Field::Effort, "medium"),
                ] {
                    let rect = targets.iter().find(|(_, value)| *value == field).unwrap().0;
                    assert_eq!(rect.y, y);
                    let available = (layout.gap.x + 2) / 2 - 4;
                    let text = crate::ui::line_truncation::truncate_line_with_ellipsis_if_overflow(
                        Line::from(text),
                        usize::from(available),
                    )
                    .to_string();
                    assert_eq!(rect.width as usize, text.width());
                    assert_eq!(
                        rect.x,
                        if field == Field::Model {
                            2
                        } else {
                            layout.gap.x - rect.width
                        }
                    );
                    for (offset, symbol) in text.chars().enumerate() {
                        let cell = &buf[(rect.x + offset as u16, y)];
                        assert_eq!(cell_text(cell.symbol()), symbol.to_string());
                        assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
                        assert!(!cell.modifier.contains(ratatui::style::Modifier::BOLD));
                    }
                }
                if use_colors && width == 120 && human_name.is_none() {
                    let text: String = (0..layout.gap.x + 2)
                        .map(|x| cell_text(buf[(x, y)].symbol()))
                        .collect();
                    let styles: String = (0..layout.gap.x + 2)
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
                        include_str!("../../tests/ui_snapshots/task_details_model_style.txt")
                    );
                }
            }
        }
    }
}

#[test]
fn task_details_status_folder_and_optional_pr_snapshot() {
    let mut row = session("pr", "PR task", SessionStatus::Inactive, 120);
    row.git_branch = Some("hidden-branch".into());
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 1;
    view.set_sessions(vec![row.clone()], false);
    let render = |view: &View| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        view.render(buf.area, &mut buf);
        let viewport = view.viewport.borrow();
        let y = viewport.task_details.bottom();
        let layout = view.center_layout(buf.area);
        let mut text = String::new();
        for x in layout.task_details.x..layout.task_details.right() {
            let symbol = cell_text(buf[(x, y)].symbol());
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
        include_str!("../../tests/ui_snapshots/task_details_branch.txt")
    );
    assert_eq!(view.viewport.borrow().task_details_links.len(), 1);
    row.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    view.set_sessions(vec![row.clone()], false);
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/task_details_branch_pr.txt")
    );
    for width in [80, 120, 160, 200] {
        let mut buf = Buffer::empty(Rect::new(5, 8, width, 30));
        view.render(buf.area, &mut buf);
        let viewport = view.viewport.borrow();
        for (rect, _) in &viewport.task_details_links {
            assert_eq!(rect.y, viewport.task_details.bottom());
            assert!(rect.right() <= buf.area.right());
        }
    }
    row.pr = None;
    view.set_sessions(vec![row], false);
    let text = render(&view);
    assert!(!text.contains("#833"));
    assert!(text.contains("hidden-branch"));
    assert_eq!(view.viewport.borrow().task_details_links.len(), 1);
}

#[test]
fn task_details_activity_and_message_are_independent() {
    let now = Utc::now();
    let mut row = session("activity", "Activity task", SessionStatus::Working, 7);
    row.prompt_at = Some(now - Duration::seconds(4200));
    row.turn_messages = 42;
    row.last_record_at = Some(now - Duration::seconds(3595));
    row.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Tool,
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
        view.render_task_details(area, &mut buf, now);
        buf.content
            .chunks(40)
            .map(|row| {
                row.iter()
                    .map(|cell| cell_text(cell.symbol()))
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
    assert!(rendered.contains("             Last activity       7s ago"));
    assert!(rendered.contains("                 Update          1h ago"));
    assert!(rendered.find("Run checks").unwrap() < rendered.find("Update").unwrap());
    assert!(rendered.contains("Checking the project"));
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/task_details_activity_working.txt")
    );
    row.activity.as_mut().unwrap().label =
        "Run checks on every changed surface in the project".into();
    view.set_sessions(vec![row.clone()], false);
    let clipped = render(&view);
    assert!(clipped.contains("Run checks on every changed surface in …"));
    assert!(!clipped.contains("surface in the project"));
    // Both sections remain visible regardless of status or timestamp order.
    row.activity.as_mut().unwrap().label = "Run checks".into();
    row.status = SessionStatus::Ready;
    view.set_sessions(vec![row.clone()], false);
    assert!(render(&view).contains("             Last activity       7s ago"));
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/task_details_activity_ready.txt")
    );
    row.status = SessionStatus::Working;
    row.last_message_at = Some(now - Duration::seconds(3));
    view.set_sessions(vec![row.clone()], false);
    let replied = render(&view);
    assert!(replied.contains("             Last activity       7s ago"));
    assert!(replied.contains("                 Update          3s ago"));
    assert_eq!(
        replied,
        include_str!("../../tests/ui_snapshots/task_details_activity_replied.txt")
    );
}

#[test]
fn task_details_activity_and_message_keep_copy_links_and_short_panel_budgets() {
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
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Tool,
        label: activity.into(),
        at: now,
    });
    row.last_message = Some(message.into());
    row.last_message_at = Some(now);
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let rendered = event_screen(&view, 120, 40, now);
    for (body, label, path) in [
        (activity, "Copied last activity", "reports/activity.md"),
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
        view.render_task_details(area, &mut buf, now);
        let viewport = view.viewport.borrow();
        if height == 16 {
            assert_eq!(viewport.task_details_scroll.fallback_max, None);
            assert_eq!(viewport.task_details_copy.len(), 3);
            assert_eq!(viewport.task_details_scroll.hidden[0], 0);
            assert!(viewport.task_details_scroll.hidden[1] > 0);
            assert!(viewport.task_details_scroll.hidden[2] > 0);
        } else {
            assert!(viewport.task_details_scroll.fallback_max.unwrap() > 0);
        }
    }
}

#[test]
fn task_details_activity_uses_tool_color() {
    use ratatui::style::Modifier;
    let now = Utc::now();
    let mut row = session("activity", "Activity task", SessionStatus::Ready, 0);
    row.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Tool,
        label: "Run checks".into(),
        at: now - Duration::minutes(2),
    });
    row.last_message = Some("Checking the project".into());
    row.last_message_at = Some(now - Duration::minutes(5));
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 60, 24);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, now);
    let header = "                       Last activity                 2m ago";
    let y = (0..area.height)
        .find(|y| {
            header
                .chars()
                .enumerate()
                .all(|(x, ch)| cell_text(buf[(x as u16, *y)].symbol()) == ch.to_string())
        })
        .expect("activity header");
    for (offset, character) in header.chars().enumerate() {
        let cell = &buf[(offset as u16, y)];
        assert_eq!(cell_text(cell.symbol()), character.to_string());
        assert_eq!(cell.fg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::DIM));
    }
    for (offset, character) in "Run checks".chars().enumerate() {
        let cell = &buf[(25 + offset as u16, y + 1)];
        assert_eq!(cell_text(cell.symbol()), character.to_string());
        assert_eq!(cell.fg, crate::theme::text_accent_color());
        assert!(!cell.modifier.contains(Modifier::DIM));
    }
}

#[test]
fn task_details_message_uses_model_color_under_markdown() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    use ratatui::style::Modifier;
    let now = Utc::now();
    let mut row = session("message", "Message task", SessionStatus::Ready, 0);
    row.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Tool,
        label: "Run checks".into(),
        at: now - Duration::minutes(5),
    });
    row.last_message = Some("Plain `code` **bold** *italic*\n\nSecond paragraph".into());
    row.last_message_at = Some(now - Duration::minutes(2));
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 60, 24);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, now);
    let header = "                           Update                    2m ago";
    let y = (0..area.height)
        .find(|y| {
            header
                .chars()
                .enumerate()
                .all(|(x, ch)| cell_text(buf[(x as u16, *y)].symbol()) == ch.to_string())
        })
        .expect("message header");
    for (offset, character) in header.chars().enumerate() {
        let cell = &buf[(offset as u16, y)];
        assert_eq!(cell_text(cell.symbol()), character.to_string());
        assert_eq!(cell.fg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::DIM));
    }
    for (offset, character) in "Plain code bold italic".chars().enumerate() {
        let cell = &buf[(19 + offset as u16, y + 1)];
        assert_eq!(cell_text(cell.symbol()), character.to_string());
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
        let cell = &buf[(19 + offset as u16, y + 3)];
        assert_eq!(cell_text(cell.symbol()), character.to_string());
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
            "                 Prompt             45s",
        ),
        (
            SessionStatus::Ready,
            Some(now),
            Some(now - Duration::seconds(1)),
            0,
            "                 Prompt              0s",
        ),
        (
            SessionStatus::Working,
            Some(now + Duration::seconds(1)),
            None,
            0,
            "                 Prompt              0s",
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
        view.render_task_details(area, &mut buf, now);
        let header = buf
            .content
            .chunks(40)
            .find(|row| {
                row.iter()
                    .map(|cell| cell_text(cell.symbol()))
                    .collect::<String>()
                    .trim_start()
                    .starts_with("Prompt")
            })
            .unwrap();
        assert_eq!(
            header
                .iter()
                .map(|cell| cell_text(cell.symbol()))
                .collect::<String>()
                .trim_end(),
            expected
        );
        for cell in &header[..expected.chars().count()] {
            assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
        }
    }
}

mod task_details_header {
    use super::*;
    use ratatui::style::Modifier;

    fn rule(view: &View, now: chrono::DateTime<Utc>) -> (Buffer, Rect) {
        rule_at_width(view, now, 112)
    }

    fn rule_at_width(view: &View, now: chrono::DateTime<Utc>, width: u16) -> (Buffer, Rect) {
        let area = Rect::new(0, 0, width, 25);
        let layout = view.center_layout(area);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let rule = if layout.kind == render::ScreenLayout::Small {
            Rect::new(area.x, layout.gap.y, area.width, 1)
        } else {
            Rect::new(
                layout.task_details.x,
                layout.header.y + 1,
                layout.task_details.width,
                1,
            )
        };
        (buf, rule)
    }

    fn label(buf: &Buffer, rule: Rect) -> String {
        (rule.x..rule.right())
            .map(|x| cell_text(buf[(x, rule.y)].symbol()))
            .collect::<String>()
            .trim_matches(['─', '╶', '╴'])
            .trim()
            .to_owned()
    }

    /// The rule under the task details panel, which carries the task's name.
    fn name_row(view: &View, rule: Rect) -> Rect {
        Rect::new(
            rule.x,
            view.viewport.borrow().task_details.bottom(),
            rule.width,
            1,
        )
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
                            view.viewport.borrow().task_details.bottom(),
                            rule.width,
                            1,
                        );
                        let header = label(&buf, rule);
                        assert!(!label(&buf, bottom).contains("Working"));
                        assert!(header.starts_with("Working"), "{header}");
                        assert!(label(&buf, bottom).ends_with("Fix login bug"));
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

    fn assert_inactive_elapsed(provider: ProviderId, last_record: bool) {
        let start = "2026-10-13T12:00:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        for (seconds, expected) in [(125, "2m 5s"), (-1, "0s")] {
            let mut row = session("header", "Finished task", SessionStatus::Inactive, 0);
            row.id.provider = provider;
            row.started_at = Some(start);
            row.updated_at = start + Duration::seconds(if last_record { 999 } else { seconds });
            row.last_record_at = last_record.then_some(start + Duration::seconds(seconds));
            let mut view = test_view(Grouping::Project, true);
            view.status_filter = 1;
            view.set_sessions(vec![row], false);
            let headers: Vec<_> = [300, 900]
                .into_iter()
                .map(|advance| {
                    let (buf, rule) = rule(&view, start + Duration::seconds(advance));
                    label(&buf, rule)
                })
                .collect();
            assert_eq!(headers[0], headers[1]);
            assert!(headers[0].ends_with(expected), "{}", headers[0]);
        }
    }

    #[test]
    fn inactive_elapsed_stops_at_last_record() {
        assert_inactive_elapsed(ProviderId::Claude, true);
    }

    #[test]
    fn inactive_elapsed_uses_updated_at_without_last_record() {
        assert_inactive_elapsed(ProviderId::Codex, false);
    }

    #[test]
    fn status_is_on_top_and_the_title_below_in_large_and_medium_layouts() {
        let row = session("header", "Header task", SessionStatus::Working, 0);
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        for width in [112, 200] {
            let (buf, rule) = rule_at_width(&view, Utc::now(), width);
            assert_eq!(label(&buf, rule), "Working");
            let bottom = label(&buf, name_row(&view, rule));
            assert!(bottom.ends_with("Header task"), "{bottom}");
            assert!(bottom.starts_with("project"), "{bottom}");
            assert!(name_row(&view, rule).y > rule.y + 1);
        }
    }

    #[test]
    fn medium_and_large_truncate_the_title_below_the_status() {
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
            let header = label(&buf, name_row(&view, rule));
            assert!(header.contains('升'), "{header}");
            assert!(header.contains("…"), "{header}");
            assert!(header.starts_with("project"), "{header}");
            assert!(label(&buf, rule).starts_with("Working"));
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
            let name = name_row(&view, rule);
            let header = "Header task";
            assert!(label(&buf, name).ends_with(header));
            let start = name.x - 2 + (name.width + 3 - (header.width() as u16 + 2)) / 2 + 1;
            let title = &buf[(start, name.y)];
            assert_eq!(cell_text(title.symbol()), "H");
            assert_eq!(title.fg, view.title_color(&session_key("header")));
            assert!(title.modifier.contains(Modifier::BOLD));
            assert!(!title.modifier.contains(Modifier::DIM));
            assert_eq!(view.viewport.borrow().task_details_title.x, start);
            let start = rule.x - 2 + (rule.width + 3 - (status.label().len() as u16 + 2)) / 2 + 1;
            for (offset, symbol) in status.label().chars().enumerate() {
                let cell = &buf[(start + offset as u16, rule.y)];
                assert_eq!(cell_text(cell.symbol()), symbol.to_string());
                assert_eq!(cell.fg, ratatui::style::Color::Reset);
                assert!(cell.modifier.contains(Modifier::BOLD));
                assert!(!cell.modifier.contains(Modifier::DIM));
            }
        }
    }

    #[test]
    fn prompt_label_keeps_the_task_list_color() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(
            vec![session("task", "Header task", SessionStatus::Ready, 0)],
            false,
        );
        let color = view.title_color(&session_key("task"));
        for focused in [false, true] {
            if focused {
                view.set_active_panel(Panel::Prompt);
            }
            assert_eq!(view.prompt_focused(), focused);
            let (buf, _) = rule(&view, Utc::now());
            let prompt = view.center_layout(buf.area).prompt;
            let y = prompt.y;
            let x = (prompt.x..prompt.right())
                .find(|&x| buf[(x, y)].symbol() == "H")
                .unwrap();
            assert_eq!(buf[(x, y)].fg, color);
            assert!(!buf[(x, y)].modifier.contains(Modifier::DIM));
            let marker = (x..prompt.right())
                .find(|&x| buf[(x, y)].symbol() == PROMPT_MARKER)
                .unwrap();
            assert_ne!(buf[(marker, y)].fg, color);
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
        let header = label(&buf, name_row(&view, rule));
        assert!(header.contains("A very long"), "{header}");
        assert!(header.contains("…"), "{header}");
        assert_eq!(
            task_details_at(row, 15, now),
            include_str!("../../tests/ui_snapshots/task_details_header_elapsed.txt")
        );
    }

    #[test]
    fn truncates_title_on_the_name_row() {
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
        let header = label(&buf, name_row(&view, rule));
        let rendered = queued_task_details(row, 15);
        assert!(header.contains("…"), "{header}");
        assert!(header.starts_with("project"), "{header}");
        assert_eq!(
            rendered,
            include_str!("../../tests/ui_snapshots/task_details_header_truncated.txt")
        );
    }
    #[test]
    fn borders_keep_title_status_elapsed_and_event_jump_separate() {
        let (mut view, now) = event_fixture();
        view.select_fixture_event(Some(view.event_panel[6].clone()));
        view.rows[view.selected].started_at = Some(now - Duration::seconds(11520));
        view.event_list.unseen.set(1);
        for width in [112, 200] {
            for with_event in [true, false] {
                view.rows[view.selected].transcript =
                    with_event.then(|| "/tmp/task-log.jsonl".into());
                let (buf, top) = rule_at_width(&view, now, width);
                let layout = view.center_layout(buf.area);
                let bottom = view.viewport.borrow().task_details.bottom();
                let expected = " Working ";
                // The status centers on the whole segment between the separator's joint and the
                // event list's joint or the screen's edge, not the space left of the time.
                let segment = top.width
                    + if crate::ui::render::ScreenLayout::for_width(width)
                        == crate::ui::render::ScreenLayout::Large
                    {
                        4
                    } else {
                        3
                    };
                let start = top.x + (segment - expected.width() as u16) / 2 - 2;
                for (offset, symbol) in expected.chars().enumerate() {
                    assert_eq!(
                        cell_text(buf[(start + offset as u16, top.y)].symbol()),
                        symbol.to_string()
                    );
                }
                assert!((top.x..start).all(|x| buf[(x, top.y)].symbol() == "─"));
                // The time ends a space before the event list's joint or the screen's edge.
                let time_end = top.right()
                    - u16::from(
                        crate::ui::render::ScreenLayout::for_width(width)
                            != crate::ui::render::ScreenLayout::Large,
                    );
                assert!(
                    (start + expected.width() as u16..time_end - " 3h 12m ".len() as u16)
                        .all(|x| buf[(x, top.y)].symbol() == "─")
                );
                let bottom_text = (top.x..top.right())
                    .map(|x| cell_text(buf[(x, bottom)].symbol()))
                    .collect::<String>();
                assert!(!bottom_text.contains("Working"));
                assert!(bottom_text.contains("project"));
                let title_start = (usize::from(segment) - " Task log example ".len()) / 2 - 2;
                assert_eq!(
                    bottom_text
                        .chars()
                        .skip(title_start)
                        .take(" Task log example ".len())
                        .collect::<String>(),
                    " Task log example "
                );
                for x in time_end - 6..time_end {
                    let cell = &buf[(x, top.y)];
                    assert!(cell.modifier.contains(Modifier::DIM));
                    assert!(!cell.modifier.contains(Modifier::BOLD));
                }
                assert!(label(&buf, top).ends_with(" 3h 12m"));
                assert!(!bottom_text.contains("3h 12m"));
                assert!(!label(&buf, top).contains("Task log example"));
                if !layout.event_panel.is_empty() {
                    let rule = &buf[(layout.list.right() - 1, bottom)];
                    assert!(
                        !cell_text(rule.symbol())
                            .chars()
                            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                    );
                    if super::super::render::ScreenLayout::for_width(width)
                        == super::super::render::ScreenLayout::Large
                    {
                        assert_eq!(
                            view.viewport.borrow().event_jump.y,
                            layout.event_panel.bottom()
                        );
                        assert!(view.viewport.borrow().event_jump.x >= top.right());
                    }
                }
            }
        }
    }

    #[test]
    fn centered_status_truncates_before_elapsed_is_dropped() {
        let (mut view, now) = event_fixture();
        view.rows[view.selected].started_at = Some(now - Duration::seconds(11520));
        for columns in 10..=100 {
            let (buf, top) = rule_at_width(&view, now, columns);
            let rule = Rect::new(
                top.x,
                view.viewport.borrow().task_details.bottom(),
                top.width,
                1,
            );
            let width = rule.width;
            let small = render::ScreenLayout::for_width(columns) == render::ScreenLayout::Small;
            let segment_width = width + if small { 0 } else { 3 };
            let content_offset = if small { 0 } else { 2 };
            let text = (rule.x..rule.right())
                .map(|x| cell_text(buf[(x, rule.y)].symbol()))
                .collect::<String>();
            assert!(!text.contains("3h 12m"));
            let top_text = (top.x..top.right())
                .map(|x| cell_text(buf[(x, top.y)].symbol()))
                .collect::<String>();
            // The centered status keeps elapsed's width and its spaces clear on both sides, plus
            // a status cell, two spaces and two rule cells.
            let elapsed_width = " 3h 12m ".len() as u16;
            let reserved = if segment_width >= 2 * elapsed_width + 7 {
                let elapsed_end = if small { " 3h 12m╶─" } else { " 3h 12m╶" };
                assert!(top_text.ends_with(elapsed_end), "width {width}: {top_text}");
                2 * elapsed_width
            } else {
                assert!(!top_text.contains("3h 12m"), "width {width}: {top_text}");
                0
            };
            // The rule runs from the separator's joint to the screen's edge or the event list's joint,
            // a cell beyond the task details panel on each side.
            let available = usize::from(segment_width).saturating_sub(6 + usize::from(reserved));
            if available >= 1 {
                let title = "Working";
                let expected = if title.len() > available {
                    format!("{}…", &title[..available - 1])
                } else {
                    title.to_owned()
                };
                let expected = format!(" {expected} ");
                let start =
                    (usize::from(segment_width) - expected.chars().count()) / 2 - content_offset;
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
    fn tight_top_border_keeps_centered_status() {
        let (mut view, now) = event_fixture();
        view.rows[view.selected].started_at = Some(now - Duration::seconds(11520));
        let area = Rect::new(0, 0, 28, 25);
        let layout = view.center_layout(area);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let top = Rect::new(area.x, layout.gap.y, area.width, 1);
        let text = (top.x..top.right())
            .map(|x| cell_text(buf[(x, top.y)].symbol()))
            .collect::<String>();
        let expected = " Worki… ";
        let start = (usize::from(top.width) - expected.chars().count()) / 2;
        assert_eq!(
            text.chars()
                .skip(start)
                .take(expected.chars().count())
                .collect::<String>(),
            expected
        );
        assert!(text.ends_with(" 3h 12m╶─"), "{text}");
    }
}

#[test]
fn task_details_prompt_shows_queued_entries_in_order() {
    let mut row = session("queue", "Queue task", SessionStatus::Working, 0);
    row.prompt = "Current prompt".into();
    row.queued = vec!["First queued prompt".into(), "Second queued prompt".into()];
    assert_eq!(
        queued_task_details(row, 15),
        include_str!("../../tests/ui_snapshots/task_details_queued.txt")
    );
}

#[test]
fn task_details_prompt_renders_all_wrapped_lines() {
    let mut row = session("prompt", "Prompt task", SessionStatus::Working, 0);
    row.prompt = format!("{} {} {}", "a".repeat(40), "b".repeat(40), "c".repeat(40));
    let rendered = queued_task_details(row, 15);
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(lines[1], "a".repeat(40));
    assert_eq!(lines[2], "b".repeat(40));
    assert_eq!(lines[3], "c".repeat(40));
    assert_eq!(lines.len(), 4);
    assert!(!rendered.contains('…'));
}

#[test]
fn task_details_prompt_overflow_marks_panel_and_scrolls_to_all_lines() {
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
    view.render_task_details(area, &mut buf, now);
    let line = |buf: &Buffer, y: u16| {
        (0..area.width)
            .map(|x| cell_text(buf[(x, y)].symbol()))
            .collect::<String>()
            .trim_end()
            .to_owned()
    };
    assert_eq!(line(&buf, 1), wrapped[0]);
    assert_eq!(line(&buf, 2), wrapped[1]);
    assert!(line(&buf, 5).ends_with('…'));
    let hidden = view.viewport.borrow().task_details_scroll.hidden[0];
    assert!(hidden > 0);
    for _ in 0..hidden {
        view.scroll_content_row(true, false, area.y + 1);
        buf.reset();
        view.render_task_details(area, &mut buf, now);
    }
    assert_eq!(line(&buf, 5), wrapped.last().unwrap().as_str());
    assert!(!buf.content.iter().any(|cell| cell.symbol() == "…"));
}

#[test]
fn task_details_prompt_wraps_long_queued_entry() {
    let mut row = session("queue", "Queue task", SessionStatus::Working, 0);
    row.prompt = "Current prompt ".repeat(12);
    row.queued = vec!["Queued prompt that extends beyond the panel width".into()];
    assert_eq!(
        queued_task_details(row, 15),
        include_str!("../../tests/ui_snapshots/task_details_queued_wrapped.txt")
    );
}

#[test]
fn task_details_queued_messages_render_all_paragraphs_and_message_style() {
    let mut row = session("queue", "Queue task", SessionStatus::Working, 0);
    row.prompt = "Current prompt".into();
    row.queued = vec![
        "First queued line  \nSecond queued line\n\nAnother paragraph with words that wrap across the panel width".into(),
        "Next queued message".into(),
    ];
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, 20);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, Utc::now());
    let rows = buf
        .content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|cell| cell_text(cell.symbol()))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        &rows[1..11],
        &[
            "             Current prompt",
            "",
            " First queued line",
            " Second queued line",
            "",
            " Another paragraph with words that wrap",
            " across the panel width",
            "",
            "          Next queued message",
            "",
        ]
    );
    for y in [3, 4, 6, 7, 9] {
        for x in rows[y].len() - rows[y].trim_start().len()..rows[y].len() {
            assert_eq!(
                buf[(x as u16, y as u16)]
                    .modifier
                    .contains(ratatui::style::Modifier::BOLD),
                style::event_styles(&crate::event::Kind::QueuedUser)
                    .1
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD),
            );
        }
    }
    assert!(
        !buf[(13, 1)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
}

#[test]
fn task_details_queued_messages_keep_links_selection_and_copy() {
    let mut view = fixture();
    let queued = "Queued first line\n\nRead [report](https://example.com/report) and finish";
    let selected = &mut view.rows[view.selected];
    selected.prompt = "Current prompt".into();
    selected.queued = vec![queued.into(), "Second queued message".into()];
    let rendered = screen(&view, 120, 50);
    assert_eq!(
        click_text(&mut view, &rendered, "report"),
        Some(Action::OpenPath("https://example.com/report".into()))
    );
    assert_eq!(
        double_click_text(&mut view, &rendered, "Queued first"),
        Some(Action::Copy {
            text: queued.into(),
            label: "Copied prompt".into(),
        })
    );
    view.last_copy_click = None;
    let (x, y) = rendered_cell(&rendered, "Queued first");
    let (end_x, end_y) = rendered_cell(&rendered, "finish");
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end_x, end_y));
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), end_x, end_y)),
        Some(Action::Copy {
            text: "Queued first line\n\nRead report and ".into(),
            label: "Copied prompt".into(),
        })
    );
    assert_eq!(
        double_click_text(&mut view, &rendered, "Second queued"),
        Some(Action::Copy {
            text: "Second queued message".into(),
            label: "Copied prompt".into(),
        })
    );
}

fn queued_task_details(row: Session, height: u16) -> String {
    task_details_at(row, height, Utc::now())
}

fn task_details_at(row: Session, height: u16, now: chrono::DateTime<Utc>) -> String {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, height);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, now);
    buf.content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|cell| cell_text(cell.symbol()))
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
fn task_details_sections_run_background_activity_update_prompt_tasks() {
    let mut row = session("order", "Order task", SessionStatus::Ready, 120);
    row.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Tool,
        label: "Run checks".into(),
        at: Utc::now(),
    });
    row.last_message = Some("Checking the project".into());
    row.background = vec![crate::model::BackgroundTask {
        description: "Waiting for output".into(),
        output: Some(std::path::PathBuf::from("/nonexistent/output")),
        started_at: Utc::now(),
    }];
    row.tasks = vec![crate::model::SessionTask {
        id: "1".into(),
        subject: "Move the prompt".into(),
        status: crate::model::TaskStatus::InProgress,
    }];
    let rendered = queued_task_details(row, 40);
    let positions = [
        "Background\n",
        "Last activity",
        "Update",
        "Prompt\n",
        "Tasks",
    ]
    .map(|label| rendered.find(label).expect(label));
    assert!(positions.is_sorted(), "{rendered}");
}

#[test]
fn task_details_background_boxes_tail_and_clip_output() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "discard\nfirst\nlast\n{}", "x".repeat(80)).unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.last_message = Some("Checking the project".into());
    row.background = vec![
        crate::model::BackgroundTask {
            description: "Waiting for output".into(),
            output: Some(output.path().with_extension("missing")),
            started_at: Utc::now(),
        },
        crate::model::BackgroundTask {
            description: "Run checks".into(),
            output: Some(output.path().into()),
            started_at: Utc::now(),
        },
    ];
    let rendered = queued_task_details(row.clone(), 22);
    assert!(rendered.find("Background\n").unwrap() < rendered.find("Prompt\n").unwrap());
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/task_details_background.txt")
    );
    row.background[1].description = "A very long description ".repeat(5);
    assert_eq!(
        queued_task_details(row.clone(), 22),
        include_str!("../../tests/ui_snapshots/task_details_background_long_title.txt")
    );
    let clipped = queued_task_details(row, 9);
    assert!(
        clipped
            .lines()
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n")
            .starts_with(
                "Background\n…\n\nUpdate\nChecking the project\n\nPrompt\nBackground task"
            ),
        "{clipped}"
    );
    assert!(clipped.contains("Update"));
    assert!(!clipped.contains("Project"));
    assert!(!clipped.contains("Unknown"));
    assert!(!clipped.contains("xxxxxxxx"));
}

#[test]
fn task_details_background_includes_only_working_subagents() {
    let now = Utc::now();
    let mut parent = session("parent", "Parent", SessionStatus::Ready, 0);
    let output = tempfile::NamedTempFile::new().unwrap();
    parent.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: Some(output.path().into()),
        started_at: now - Duration::seconds(10),
    });
    let mut worker = session("agent-worker", "Check files", SessionStatus::Working, 0);
    worker.parent = Some(parent.id.clone());
    worker.started_at = Some(now - Duration::seconds(65));
    worker.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Tool,
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
    nested.started_at = Some(now - Duration::seconds(125));
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
    view.render_task_details(area, &mut buf, now);
    let rendered = buf
        .content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|c| cell_text(c.symbol()))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        rendered.trim_end_matches('\n'),
        include_str!("../../tests/ui_snapshots/task_details_subagents.txt")
    );
    let (x, y) = rendered_cell(&rendered, "Read files");
    assert_eq!(
        buf[(x, y)].fg,
        style::event_styles(&crate::event::Kind::Tool).1.fg.unwrap()
    );
    view.selected = view
        .rows
        .iter()
        .position(|s| s.id == session_key("agent-worker"))
        .unwrap();
    assert!(view.has_background());
    buf.reset();
    view.render_task_details(area, &mut buf, now);
    assert!(buf.content.chunks(40).any(|row| {
        row.iter()
            .map(|c| cell_text(c.symbol()))
            .collect::<String>()
            .contains("Background")
    }));
    view.rows[view.selected]
        .background
        .push(crate::model::BackgroundTask {
            description: "Worker command".into(),
            output: Some(output.path().into()),
            started_at: now,
        });
    assert!(view.has_background());
    buf.reset();
    view.render_task_details(area, &mut buf, now);
    let text = buf
        .content
        .iter()
        .map(|cell| cell_text(cell.symbol()))
        .collect::<String>();
    assert!(text.contains("● Worker command"));
    assert!(text.contains("Nested agent"));
    assert!(!text.contains("Review files"));
}

#[test]
fn background_box_dims_title_but_not_indicator_or_output() {
    use ratatui::style::Modifier;
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: Some(output.path().into()),
        started_at: Utc::now(),
    });
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, 20);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, Utc::now());
    let top = (0..20)
        .find(|y| buf[(14, *y)].symbol() == "●")
        .expect("background indicator");
    assert!(!buf[(14, top)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(14, top)].fg, View::status_indicator(SessionStatus::Working).style.fg.unwrap());
    assert_eq!(cell_text(buf[(16, top)].symbol()), "R");
    assert!(buf[(16, top)].modifier.contains(Modifier::DIM));
    let (tool_view, _) = open_tool_fixture();
    let mut event = tool_view.event_panel[7].clone();
    event.calls[0].title = "Run checks".into();
    event.calls[0].event_detail_title = "Run checks".into();
    let detail =
        super::tool_detail::event_detail_content(&event, String::new(), Style::new().dim());
    let mut title_buf = Buffer::empty(Rect::new(0, 0, 40, 1));
    let columns = super::event_detail::event_detail_columns(title_buf.area, title_buf.area);
    (&detail.sections[0].title.line(columns).0).render(title_buf.area, &mut title_buf);
    let title_x = (0..40)
        .find(|x| cell_text(title_buf[(*x, 0)].symbol()) == "R")
        .unwrap();
    assert!(title_buf[(0, 0)].modifier.contains(Modifier::DIM));
    assert_eq!(
        title_buf[(title_x, 0)].fg,
        detail.sections[0].title.title_style.fg.unwrap()
    );
    assert_eq!(cell_text(buf[(18, top + 1)].symbol()), "t");
    assert!(!buf[(18, top + 1)].modifier.contains(Modifier::DIM));
}

#[test]
fn background_task_whose_output_ended_is_hidden() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick\n\n[exited with code 0]").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        description: "Run checks".into(),
        output: Some(output.path().into()),
        started_at: Utc::now(),
    });
    let rendered = queued_task_details(row, 19);
    assert!(!rendered.contains("\nBackground\n"));
    assert!(!rendered.contains("Run checks"));
}

#[test]
fn background_redraw_follows_selection() {
    let mut first = session("first", "First", SessionStatus::Ready, 0);
    first.background.push(crate::model::BackgroundTask {
        description: "Running".into(),
        output: Some("/tmp/unused.output".into()),
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
fn task_details_markdown_snapshot() {
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
        .draw(|frame| view.render_task_details(frame.area(), frame.buffer_mut(), Utc::now()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = buffer
        .content
        .chunks(40)
        .map(|row| {
            row.iter()
                .map(|cell| cell_text(cell.symbol()))
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
        include_str!("../../tests/ui_snapshots/task_details_markdown.txt")
    );
    let prompt_y = rendered
        .lines()
        .position(|line| line.trim() == "Prompt")
        .unwrap() as u16;
    let update_y = rendered
        .lines()
        .position(|line| line.trim() == "Update")
        .unwrap() as u16;
    assert_eq!(
        buffer[(14, prompt_y + 1)].fg,
        crate::theme::inline_code_color()
    );
    assert!(
        buffer[(24, prompt_y + 1)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(
        buffer[(14, prompt_y + 3)]
            .modifier
            .contains(ratatui::style::Modifier::ITALIC)
    );
    assert_eq!(buffer[(19, update_y + 3)].fg, Color::Yellow);
    assert_eq!(buffer[(13, update_y + 5)].fg, Color::Cyan);
}

#[test]
fn kind_labels_are_kind_names_as_capitalized_words() {
    use crate::event::Kind;
    for (kind, label) in [
        (Kind::Assistant, "Assistant"),
        (Kind::QueuedUser, "Queued User"),
        (Kind::QueuedSystem, "Queued System"),
    ] {
        assert_eq!(super::event_list::kind_label(&kind), label);
    }
}

#[test]
fn compaction_summary_renders_compaction_tag_and_keeps_visible_body() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-compaction.jsonl");
    let events = crate::providers::claude::event_reader::tail(&path, 10).unwrap();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(super::event_list::kind_label(&event.kind), "Compaction");
    assert_eq!(super::event_list::copied_label(event), "Copied message");
    let (mut view, now) = event_fixture();
    view.event_panel = events.clone();
    view.select_fixture_event(Some(event.clone()));
    let rendered = event_screen(&view, 120, 60, now);
    assert!(rendered.contains("Compaction"), "{rendered}");
    assert!(
        rendered.matches("This session is being continued").count() >= 2,
        "summary must remain visible in both list and detail: {rendered}"
    );
    assert!(!rendered.contains("User"), "{rendered}");
}

fn event_fixture() -> (View, chrono::DateTime<Utc>) {
    use crate::event::{Event, Kind};
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
    view.event_session = Some(session_key("log"));
    view.event_panel = [
        (Kind::User, "Please\n  inspect the task", 120, None),
        (Kind::Assistant, "I will check the files", 110, None),
        (Kind::Tool, "Read src/main.rs，Read src/app.rs", 100, Some(90)),
        (Kind::Error, "failed check", 80, Some(70)),
        (Kind::QueuedUser, "Also inspect the tests", 60, None),
        (Kind::Tool, "run a background job", 50, None),
        (Kind::Assistant, "A long response whose whitespace collapses and whose body must be truncated before the right aligned timestamp", 40, None),
        (Kind::Tool, "run tests", 30, None),
    ].into_iter().map(|(kind, body, age, end)| Event {
        turn: None,
        source: None,
        completed_time: false,
        background: false,
        source_records: Vec::new(),
        sender: None,
        detail_body: None,
        kind, body: body.into(), paths: Vec::new(), started_at: now - Duration::seconds(age),
        ended_at: end.map(|age| now - Duration::seconds(age)),
        duration: None,
        previous_end: Some(now - Duration::seconds(age + 5)),
        images: Vec::new(),
        calls: Vec::new(),
    }).collect();
    let mut previous_end = None;
    for event in &mut view.event_panel {
        if event.kind == Kind::Assistant {
            event.previous_end = previous_end;
        }
        previous_end = if matches!(
            event.kind,
            crate::event::Kind::Tool | crate::event::Kind::Error
        ) {
            event.ended_at
        } else {
            Some(event.started_at)
        };
    }
    view.set_event(view.event_panel.clone());
    (view, now)
}

fn event_screen(view: &View, width: u16, height: u16, now: chrono::DateTime<Utc>) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    buf.content
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(|cell| cell_text(cell.symbol()))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn event_list_screen(view: &View, width: u16, height: u16, now: chrono::DateTime<Utc>) -> String {
    let rendered = event_screen(view, width, height, now);
    let detail = view
        .center_layout(Rect::new(0, 0, width, height))
        .event_detail;
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
    let (mut view, now) = event_fixture();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    view.set_event(crate::providers::claude::event_reader::tail_session(&path, 200, true).unwrap());
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
        output: Some(path),
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
        .map(|x| cell_text(buf[(x, y)].symbol()))
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
                    cell_text(buf[(list.x, y)].symbol()),
                    View::status(&view.rows[second]).1.content
                );
                assert_eq!(cell_text(buf[(list.x + 2, y)].symbol()), &title[..1]);
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
fn task_metadata_medium_100_columns() {
    assert_task_metadata(100, true, true);
}

#[test]
fn task_metadata_large_150_columns() {
    assert_task_metadata(150, true, false);
}

#[test]
fn task_metadata_large_200_columns() {
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
            .map(|x| cell_text(buf[(x, y)].symbol()))
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
            buf[(area.x + 2 + title_width - 1 - u16::from(show_status), y)].symbol(),
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
                            target: super::draft_controls::TaskKey::Session(
                                view.rows[0].id.clone(),
                            ),
                            values: vec!["claude-sonnet-5-5".into(), "claude-haiku-5-5".into()],
                            labels: vec!["claude-sonnet-5-5".into(), "claude-haiku-5-5".into()],
                            families: vec!["Sonnet".into(), "Haiku".into()],
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
                (layout.task_details, row.y)
            } else {
                (layout.list, viewport.marker.expect("selected row").1)
            };
            let marker_x = content.x;
            if kind == "picker" {
                assert_eq!(cell_text(buf[(marker_x, y)].symbol()), "c");
            } else {
                assert!(!(content.x..content.right()).any(|x| buf[(x, y)].symbol() == "❯"));
            }
            if kind == "task" {
                assert_eq!(buf[(marker_x, y)].symbol(), "●");
                assert_eq!(
                    cell_text(buf[(marker_x + 2, y)].symbol()),
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
            let bottom = list.y + list.height - 1;
            assert!(
                (list.x..list.right()).all(|x| !matches!(buf[(x, bottom)].symbol(), "↑" | "↓"))
            );
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
fn dashboard_event_bodies_center_on_the_panel() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = event_fixture();
        for event in view.event_panel.clone() {
            view.set_event(vec![event.clone()]);
            let area = Rect::new(0, 0, width, 60);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let event_panel = view.viewport.borrow().event_panel;
            let y = event_panel.y;
            let body = super::event_list::body_line(
                &event,
                usize::from(event_panel.width - 10),
                &view.rows[0].cwd,
            )
            .to_string();
            for (offset, ch) in body.chars().take(12).enumerate() {
                let x = super::centering::centered_x(
                    event_panel,
                    super::event_list::event_row_areas(event_panel).0,
                    body.width() as u16,
                );
                let cell = &buf[(x + offset as u16, y)];
                assert_eq!(
                    cell_text(cell.symbol()),
                    ch.to_string(),
                    "kind {:?} at {width}",
                    event.kind
                );
                assert_eq!(
                    cell.fg,
                    super::style::event_styles(&event.kind)
                        .1
                        .fg
                        .unwrap_or(Color::Reset)
                );
            }
            if crate::ui::render::ScreenLayout::for_width(width)
                == crate::ui::render::ScreenLayout::Large
                && event.kind == crate::event::Kind::Assistant
            {
                let detail = view.center_layout(area).event_detail;
                let header: String = (detail.x..detail.right())
                    .map(|x| cell_text(buf[(x, detail.y + 1)].symbol()))
                    .collect();
                assert_eq!(header.trim().split("  ").next(), Some("Assistant"));
            }
        }
    }
}

#[test]
fn dashboard_event_list_fills_rows_next_to_rules() {
    for width in [100, 149, 150, 200] {
        for search in [false, true] {
            let (mut view, now) = column_padding_fixture();
            view.rows[0].model = Some("opus".into());
            let event = view
                .event_panel
                .iter()
                .find(|event| event.kind == crate::event::Kind::Tool)
                .unwrap()
                .clone();
            view.event_list.search.editing = search;
            let body_event = event.clone();
            view.set_event(vec![event; 100]);
            let area = Rect::new(0, 0, width, 60);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let layout = view.center_layout(area);
            let event_panel = view.viewport.borrow().event_panel;
            let body = super::event_list::body_line(
                &body_event,
                usize::from(event_panel.width - 10),
                &view.rows[0].cwd,
            );
            let x = super::centering::centered_x(
                event_panel,
                super::event_list::event_row_areas(event_panel).0,
                body.width() as u16,
            );
            let first_symbol = body.to_string().chars().next().unwrap().to_string();
            let first = event_panel.y;
            let last = event_panel.bottom() - 1 - u16::from(search);
            assert_eq!(
                cell_text(buf[(x, first)].symbol()),
                first_symbol,
                "first row at {width}, search={search}"
            );
            assert_eq!(
                cell_text(buf[(x, last)].symbol()),
                first_symbol,
                "last row at {width}, search={search}"
            );
            assert_eq!(
                view.viewport.borrow().event_rows.len(),
                usize::from(last - first + 1)
            );
            assert_eq!(view.event_count(area), usize::from(last - first + 1));
            if crate::ui::render::ScreenLayout::for_width(width)
                == crate::ui::render::ScreenLayout::Large
            {
                let rule_y = layout.prompt.y - 1;
                assert_eq!(layout.list.bottom(), rule_y);
                assert_eq!(layout.task_details.bottom(), rule_y);
                assert_eq!(event_panel.bottom(), rule_y);
                assert_eq!(buf[(event_panel.x - 2, rule_y)].symbol(), "┴");
                assert_eq!(buf[(event_panel.x - 1, rule_y)].symbol(), "─");
                assert_ne!(buf[(event_panel.x, rule_y)].symbol(), " ");
            }
        }
    }
}

#[test]
fn dashboard_event_detail_starts_directly_below_rule() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = column_padding_fixture();
        view.select_fixture_event(Some(view.event_panel.last().unwrap().clone()));
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        let detail = layout.event_detail;
        assert!(
            (detail.x..detail.right()).any(|x| buf[(x, detail.y + 1)].symbol() != " "),
            "detail header at {width}"
        );
    }
}

fn text_row_has_background_rule(buf: &Buffer, width: u16, y: u16) -> bool {
    width >= 2 && buf[(width - 2, y)].symbol() == "╶" && buf[(width - 1, y)].symbol() == "─"
}

#[test]
fn dashboard_screen_padding_is_one_cell() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = column_padding_fixture();
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.rows[view.selected].process = None;
        view.set_active_panel(Panel::Prompt);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        assert_eq!(layout.list.x, 1);
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_eq!(cell_text(buf[(2, 0)].symbol()), "A");
        assert_eq!(view.viewport.borrow().tab_bar.x, 1);
        for x in 1..width - 1 {
            if x < layout.task_details.x || x >= layout.task_details.right() {
                let joint = x == layout.gap.x + 1
                    || (crate::ui::render::ScreenLayout::for_width(width)
                        == crate::ui::render::ScreenLayout::Large
                        && x == layout.event_panel.x - 2);
                assert_eq!(
                    cell_text(buf[(x, 1)].symbol()),
                    if joint { "┬" } else { "─" }
                );
            }
        }
        for y in 0..area.height {
            // Rules run edge to edge; everything else keeps the padding cell.
            let rule = buf[(0, y)].symbol() == "─";
            assert_eq!(buf[(0, y)].symbol() == "─", rule, "row {y}");
            let background_rule = width < 150 && text_row_has_background_rule(&buf, width, y);
            assert_eq!(
                buf[(width - 1, y)].symbol() == "─",
                rule || background_rule,
                "row {y}"
            );
            if !rule {
                assert_eq!(buf[(0, y)].symbol(), " ", "row {y}");
                let right = buf[(width - 1, y)].symbol();
                assert!(
                    right == " "
                        || background_rule && right == "─"
                        || (layout.event_panel.y..layout.event_panel.bottom()).contains(&y)
                            && ["┌", "│", "└"].contains(&right),
                    "row {y}: {right}"
                );
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
                .map(|x| cell_text(buf[(x, y)].symbol()))
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
                    Some(right - 2 - u16::from(width < 150)),
                    "{label} right at {width}"
                );
            }
        }
        let y = (2..35)
            .find(|&y| text(y, left + 1, right).contains("Run checks"))
            .unwrap();
        assert!(
            text(y, left + 1, right)
                .trim_end()
                .ends_with("25s")
        );
        assert_eq!(
            text(y, left + 1, right).trim_end().chars().count() as u16,
            right - left - 2 - u16::from(width < 150),
            "background time at {width}"
        );
        // Wrapped bodies, selections and links stay inside the same content bounds.
        for y in 3..view.center_layout(area).task_details.bottom() {
            for x in [left + 1, right - 1] {
                let background_tail = x == right - 1 && ["╶", "─"].contains(&buf[(x, y)].symbol());
                assert!(
                    buf[(x, y)].symbol() == " " || background_tail,
                    "padding ({x}, {y}) at {width}"
                );
            }
        }
        let viewport = view.viewport.borrow();
        if matches!(width, 100 | 200) {
            println!(
                "{width} cols: separators={separators:?}, task Updated end={}, details={}..{}, log={}..{}",
                updated.trim_end().chars().count(),
                viewport.task_details_text.area.x,
                viewport.task_details_text.area.right(),
                viewport.event_panel.x,
                viewport.event_panel.right()
            );
        }
        assert_eq!(viewport.task_details_text.area.x, left + 2);
        assert_eq!(viewport.task_details_text.area.right(), right - 1);
    }
}

#[test]
fn dashboard_event_padding_and_edge_clicks_use_rendered_rows() {
    for width in [100, 149, 150, 200] {
        for edge in [false, true] {
            let (mut view, now) = column_padding_fixture();
            let area = Rect::new(0, 0, width, 60);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let large =
                super::render::ScreenLayout::for_width(width) == super::render::ScreenLayout::Large;
            let separators: Vec<_> = (0..width)
                .filter(|&x| buf[(x, 2)].symbol() == "│")
                .collect();
            let left = if large { separators[1] + 2 } else { 1 };
            let y = view.viewport.borrow().event_panel.y
                + view
                    .viewport
                    .borrow()
                    .event_rows
                    .iter()
                    .position(|&index| index == EventRow::Timeline(view.event_panel.len() - 2))
                    .unwrap() as u16;
            let row: String = (0..width)
                .map(|x| cell_text(buf[(x, y)].symbol()))
                .collect();
            let event_panel: String = (left..width - 1)
                .map(|x| cell_text(buf[(x, y)].symbol()))
                .collect();
            assert!(
                event_panel.trim_end().chars().count() as u16 + left <= width - 11,
                "log body right at {width}: {row}"
            );
            assert!((width - 11..width - 1).all(|x| buf[(x, y)].symbol() == " "));
            assert!(!event_panel.trim().is_empty(), "log body at {width}");
            if large {
                assert_eq!(buf[(left - 1, y)].symbol(), " ");
            }
            let selected = {
                let viewport = view.viewport.borrow();
                let index = viewport.event_rows[usize::from(y - viewport.event_panel.y)];
                assert_eq!(viewport.event_list_text.area.x, left);
                assert_eq!(viewport.event_list_text.area.right(), width - 2);
                match index {
                    EventRow::Timeline(index) => view.event_panel[index].clone(),
                    EventRow::Pinned(_) => panic!("expected timeline row"),
                }
            };
            let x = if edge { width - 2 } else { left };
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, y),
            );
            view.flush_pending_click(Instant::now() + view.double_click_interval);
            assert_eq!(
                view.visible_event().cloned(),
                Some(selected),
                "edge click at {width}"
            );
            if large {
                let detail = view.viewport.borrow().event_text.area;
                let columns = super::event_detail::event_detail_columns(
                    view.viewport.borrow().event_detail,
                    view.center_layout(buf.area).event_panel,
                );
                assert_eq!(detail.x, columns.text.x);
                assert_eq!(detail.right(), columns.text.right());
            }
        }
    }
}

#[test]
fn dashboard_event_search_uses_the_row_padding() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = column_padding_fixture();
        view.set_active_panel(Panel::EventList);
        view.event_list.search.editing = true;
        view.event_list.search.query.set_text("Tool");
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let left = if crate::ui::render::ScreenLayout::for_width(width)
            == crate::ui::render::ScreenLayout::Large
        {
            2 * ((width - 8) / 4) + 7
        } else {
            1
        };
        let y = view.viewport.borrow().event_panel.bottom() - 1;
        let input: String = (left..left + 13)
            .map(|x| cell_text(buf[(x, y)].symbol()))
            .collect();
        assert_eq!(input, "Search ❯ Tool");
        assert_eq!(view.cursor_pos(area), Some((left + 13, y)));
        view.event_list.search.editing = false;
        view.render_at(area, &mut buf, now);
        let query: String = (width - 5..width - 1)
            .map(|x| cell_text(buf[(x, y)].symbol()))
            .collect();
        assert_eq!(query, "Tool");
        assert_eq!(buf[(width - 1, y)].symbol(), " ");
    }
}

#[test]
fn task_event_time_column_and_ellipses_align_in_both_layouts() {
    use crate::event::Kind;
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let source = crate::providers::claude::event_reader::tail_session(&path, 200, true)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == Kind::Tool)
        .unwrap();
    for width in [100, 149, 150, 200] {
        let (mut view, now) = event_fixture();
        view.event_panel = [2, 590, 0]
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
        let event_panel = view.viewport.borrow().event_panel;
        let top = event_panel.y;
        for (offset, time) in ["2s", "9m 50s", "11:59:50"].into_iter().enumerate() {
            let y = top + offset as u16;
            let text = |start, end| {
                (start..end)
                    .map(|x| cell_text(buffer[(x, y)].symbol()))
                    .collect::<String>()
            };
            assert_eq!(
                text(event_panel.x, event_panel.x + 10),
                "long body ",
                "width {width}"
            );
            assert_eq!(
                text(event_panel.right() - 9, event_panel.right() - 1),
                format!("{time:>8}")
            );
            assert_eq!(
                text(event_panel.right() - 11, event_panel.right() - 9),
                "  "
            );
            assert_eq!(buffer[(event_panel.right() - 12, y)].symbol(), "…");
            assert_eq!(cell_text(buffer[(event_panel.x, y)].symbol()), "l");
        }
        view.scroll_content_row(true, true, top);
        view.render_at(area, &mut buffer, now);
        assert!(view.viewport.borrow().event_jump.is_empty());
    }
}

#[test]
fn task_event_body_centers_on_its_panel() {
    let (mut view, now) = event_fixture();
    view.event_panel.pop();
    let rendered = event_screen(&view, 100, 32, now);
    assert!(
        rendered
            .lines()
            .nth(view.viewport.borrow().event_panel.y as usize)
            .unwrap()
            .starts_with(&format!("{}Please", " ".repeat(39)))
    );
}

#[test]
fn task_event_denied_snapshot() {
    use crate::event::Kind;
    use crate::providers::claude::event_reader::tail;
    let (mut view, now) = event_fixture();
    view.event_panel.truncate(1);
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
            .event_panel
            .last()
            .map(|previous| previous.ended_at.unwrap_or(previous.started_at));
        assert_eq!(
            super::style::event_styles(&event.kind),
            super::style::event_styles(&Kind::Error)
        );
        view.event_panel.push(event);
    }
    view.set_event(view.event_panel.clone());
    view.select_event(None);
    assert_eq!(
        event_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_event_denied.txt")
    );
}

#[test]
fn codex_inner_task_event_snapshot() {
    use crate::providers::Provider;
    let (mut view, now) = event_fixture();
    view.rows[0].id.provider = crate::providers::ProviderId::Codex;
    view.event_session = Some(view.rows[0].id.clone());
    let dir = tempfile::tempdir().unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        dir.path().to_owned(),
        dir.path().join("manager"),
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-inner");
    view.event_panel.clear();
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
            .event_panel(&view.rows[0], usize::MAX, false)
            .unwrap()
            .into_iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    crate::event::Kind::Tool | crate::event::Kind::Error
                )
            })
            .collect();
        assert_eq!(events.len(), 1, "{fixture}: {events:?}");
        let mut event = events.into_iter().next().unwrap();
        let duration = event.ended_at.unwrap() - event.started_at;
        event.started_at = now - Duration::seconds(100 - index as i64 * 15);
        event.ended_at = Some(event.started_at + duration);
        event.previous_end = view
            .event_panel
            .last()
            .map(|previous| previous.ended_at.unwrap());
        view.event_panel.push(event);
    }
    view.set_event(view.event_panel.clone());
    view.select_event(None);
    let rendered = event_screen(&view, 160, 25, now);
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/codex_inner_event.txt");
    update_event_snapshot_body(&path, &rendered);
    assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
    view.prepare_event_search();
    view.event_list.search.query.set_text("ImageView");
    view.rematch_event();
    assert_eq!(view.matching_event_count(), 1);
    assert!(view.event_matches(4));
    for query in ["type", "exec_command"] {
        view.event_list.search.query.set_text(query);
        view.rematch_event();
        assert_eq!(view.matching_event_count(), 1, "{query}");
        assert!(view.event_matches(0));
    }
}

#[test]
fn task_event_tall_running_snapshot() {
    let (view, now) = event_fixture();
    assert_eq!(
        event_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_event_running.txt")
    );
}

#[test]
fn task_event_background_record_has_no_marker_and_keeps_its_time() {
    let records = include_str!("../../tests/fixtures/background/two-awaits.jsonl");
    let file = tempfile::NamedTempFile::new().unwrap();
    for (lines, expected_time) in [(3, "3m 13s"), (4, "3m 15s")] {
        std::fs::write(
            file.path(),
            format!(
                "{}\n",
                records.lines().take(lines).collect::<Vec<_>>().join("\n")
            ),
        )
        .unwrap();
        let events = crate::providers::claude::event_reader::tail(file.path(), 100).unwrap();
        let event = events.last().unwrap();
        assert_eq!(event.body, "等用量图颜色线程");
        assert!(event.background);
        assert_eq!(event.ended_at.is_some(), lines == 4);
        let now = event.started_at + Duration::minutes(2);
        let (mut view, _) = event_fixture();
        view.set_event(events);
        for now in [now, now + Duration::minutes(1)] {
            let rendered = event_screen(&view, 100, 25, now);
            let row = rendered
                .lines()
                .find(|row| row.replace(' ', "").contains("等用量图颜色线程"))
                .unwrap_or_else(|| panic!("{rendered}"));
            assert!(!row.contains(" *"), "{row}");
            assert!(
                row.trim_end_matches(['┌', '│', '└'])
                    .trim_end()
                    .ends_with(expected_time),
                "{row}"
            );
        }
    }
}

#[test]
fn task_event_tall_finished_snapshot() {
    let (mut view, now) = event_fixture();
    view.event_panel.last_mut().unwrap().ended_at = Some(now - Duration::seconds(5));
    view.set_event(view.event_panel.clone());
    assert_eq!(
        event_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_event_finished.txt")
    );
}

#[test]
fn task_event_short_terminal_reserves_output_bottom_rule() {
    let mut view = fixture();
    view.rows[view.selected].transcript = Some("/tmp/task-log.jsonl".into());
    let (event_panel, _) = event_fixture();
    view.set_event(event_panel.event_panel);
    view.event_session = view.selected_id().cloned();
    let layout = view.center_layout(Rect::new(0, 0, 140, 22));
    assert_eq!(layout.event_panel.height, 5);
    assert_eq!(layout.event_detail.height, 5);
    assert_eq!(layout.list.height, 5);
    let rendered = screen(&view, 140, 22);
    assert!(rendered.contains("Also inspect the tests"));
    assert!(!rendered.contains("Please inspect"));
}

#[test]
fn task_event_requires_room_for_header_and_six_body_rows() {
    let (mut view, now) = event_fixture();
    let hidden = event_screen(&view, 100, 18, now);
    assert!(!hidden.contains("Please inspect"));
    view.rows[view.selected].transcript = None;
    assert_eq!(hidden, event_screen(&view, 100, 18, now));
}

#[test]
fn task_event_rules_match_header_and_output_bottom_rule() {
    use ratatui::style::Modifier;
    let (mut view, now) = event_fixture();
    for loaded in [true, false] {
        if !loaded {
            view.event_session = None;
        }
        let area = Rect::new(0, 0, 100, 25);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        let middle = layout.prompt.y - 1;
        for x in 0..100 {
            for y in [middle, layout.event_detail.y] {
                if y != middle || x < layout.task_details.x || x >= layout.task_details.right() {
                    let rule = if (layout.task_details.x..layout.task_details.right()).contains(&x)
                    {
                        &buf[(1, 1)]
                    } else {
                        &buf[(x, 1)]
                    };
                    let expected = if y == middle && x == layout.gap.x + 1 {
                        "┴"
                    } else if y == middle && x == layout.task_details.x - 1 {
                        "╴"
                    } else if y == layout.event_detail.y && x == layout.gap.x + 1 {
                        "─"
                    } else {
                        cell_text(rule.symbol())
                    };
                    assert_eq!(cell_text(buf[(x, y)].symbol()), expected);
                    assert_eq!(buf[(x, y)].modifier, rule.modifier);
                }
            }
            if !loaded {
                for y in layout.event_panel.y..layout.event_panel.bottom() {
                    assert_eq!(buf[(x, y)].symbol(), " ");
                    assert_eq!(buf[(x, y)].modifier, Modifier::empty());
                }
                for y in layout.event_detail.y + 1..layout.event_detail.bottom() {
                    assert_eq!(buf[(x, y)].symbol(), " ");
                    assert_eq!(buf[(x, y)].modifier, Modifier::empty());
                }
            }
            assert_eq!(buf[(x, layout.footer.y - 1)].symbol(), "─");
            assert!(
                buf[(x, layout.footer.y - 1)]
                    .modifier
                    .contains(Modifier::DIM)
            );
        }
        assert!(buf[(2, middle)].modifier.contains(Modifier::DIM));
    }
}

#[test]
fn task_event_last_tool_clock_freezes_for_inactive_and_background_sessions() {
    let (mut view, now) = event_fixture();
    assert!(
        event_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("35s"))
    );
    assert!(
        event_screen(&view, 100, 25, now + Duration::seconds(1))
            .lines()
            .any(|line| line.ends_with("36s"))
    );
    view.rows[view.selected].process = None;
    assert!(
        event_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("25s"))
    );
    view.event_panel.last_mut().unwrap().background = true;
    assert!(
        event_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("5s"))
    );
}

#[test]
fn task_event_last_message_uses_relative_time_and_missing_transcript_keeps_panel() {
    let (mut view, now) = event_fixture();
    view.rows[view.selected].model = Some("opus".into());
    view.event_panel.pop();
    assert!(
        event_screen(&view, 100, 25, now)
            .lines()
            .any(|line| line.ends_with("40s ago"))
    );
    view.rows[view.selected].transcript = None;
    let rendered = event_screen(&view, 100, 25, now);
    assert!(!rendered.contains("Please inspect"));
    let model_y = view.center_layout(Rect::new(0, 0, 100, 25)).list.bottom();
    assert!(
        rendered
            .lines()
            .nth(usize::from(model_y))
            .unwrap()
            .contains("╴opus ")
    );
}

#[test]
fn non_current_event_time_matches_unselected_task_and_task_details_times() {
    let (mut view, now) = event_fixture();
    view.event_panel.truncate(5);
    view.select_event(None);
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
                .map(|x| cell_text(tasks[(x, y)].symbol()))
                .collect::<String>()
                .contains("Other task")
        })
        .expect("unselected task row");
    let task_time = &tasks[(99, task_y)];
    assert_ne!(task_time.symbol(), " ");
    assert!(task_time.modifier.contains(ratatui::style::Modifier::DIM));

    let mut event_panel = Buffer::empty(area);
    view.render_at(area, &mut event_panel, now);
    let y = view.center_layout(area).event_panel.y;
    assert_ne!(event_panel[(97, y)].symbol(), " ");
    assert_eq!(event_panel[(97, y)].style(), task_time.style());

    view.rows[view.selected].last_message = Some("Reply".into());
    view.rows[view.selected].last_message_at = Some(now - Duration::minutes(1));
    let mut task_details = Buffer::empty(area);
    view.render_task_details(area, &mut task_details, now);
    let task_details_y = (0..area.height)
        .find(|&y| {
            (0..area.width)
                .map(|x| cell_text(task_details[(x, y)].symbol()))
                .collect::<String>()
                .contains("Update")
        })
        .expect("message header");
    assert_ne!(task_details[(98, task_details_y)].symbol(), " ");
    assert_eq!(
        task_details[(98, task_details_y)].style(),
        task_time.style()
    );
}

#[test]
fn selected_event_row_preserves_foreground_and_adds_bold() {
    use crate::event::Kind;
    use ratatui::style::Modifier;
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
    let event = crate::providers::claude::event_reader::tail_session(&source, 200, true)
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
                        let (mut view, now) = event_fixture();
                        let mut event = event.clone();
                        event.kind = kind.clone();
                        // Include a visible link and a search match alongside plain body text.
                        event.body =
                            "Read `code` **bold** [link](https://example.com) and results".into();
                        event.calls.clear();
                        event.paths.clear();
                        let mut older = event.clone();
                        older.started_at -= Duration::seconds(1);
                        view.set_event(vec![older, event]);
                        if search {
                            view.event_list.search.query.set_text("Read");
                            view.rematch_event();
                        }
                        let area = Rect::new(0, 0, width, 60);
                        let event_panel = view.center_layout(area).event_panel;
                        view.event_list.cursor = None;
                        for index in 0..2 {
                            let mut baseline = Buffer::empty(area);
                            view.event_list.cursor = None;
                            view.render_event(event_panel, &mut baseline, now);
                            view.select_fixture_event(Some(view.event_panel[index].clone()));
                            let mut selected = Buffer::empty(area);
                            view.render_event(event_panel, &mut selected, now);
                            let y = event_panel.y + index as u16;
                            for x in event_panel.x - 1..=event_panel.right() {
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
        for kind in [crate::event::Kind::Tool, crate::event::Kind::QueuedSystem] {
            let (label, body) = super::style::event_styles(&kind);
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
fn task_event_uses_cctail_colours_and_highlights_latest_time() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    super::highlight::use_theme(false, None).unwrap();
    use crate::event::Kind;
    use ratatui::style::Modifier;
    let (mut view, now) = event_fixture();
    view.event_panel.truncate(5);
    view.event_list.cursor = None;
    let area = Rect::new(0, 0, 100, 25);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let top = view.center_layout(area).event_panel.y;
    for (offset, colour) in [
        (0, Color::Reset),
        (1, Color::Yellow),
        (2, crate::theme::text_accent_color()),
        (3, Color::Rgb(160, 99, 93)),
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
        assert_eq!(buf[(97, y)].fg, Color::Reset, "time row {y}");
        assert_eq!(buf[(97, y)].modifier, Modifier::DIM, "time row {y}");
    }
    for (kind, colour) in [
        (Kind::User, Color::Reset),
        (Kind::Assistant, Color::Yellow),
        (Kind::Agent, Color::Yellow),
        (Kind::Session, Color::Yellow),
        (Kind::Tool, crate::theme::text_accent_color()),
        (Kind::Error, Color::Rgb(160, 99, 93)),
        (Kind::Denied, Color::Rgb(160, 99, 93)),
        (Kind::QueuedUser, Color::Reset),
        (Kind::QueuedSystem, crate::theme::text_accent_color()),
    ] {
        let (label_style, body_style) = super::style::event_styles(&kind);
        let foreground = (colour != Color::Reset).then_some(colour);
        assert_eq!(label_style.fg, foreground);
        assert_eq!(body_style.fg, foreground);
        view.event_panel.first_mut().unwrap().kind = kind.clone();
        view.event_panel.first_mut().unwrap().previous_end = None;
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let time = &buf[(97, top)];
        let time_style = super::style::event_time_style(&kind, false);
        assert_eq!(time_style, super::style::time_style());
        assert_eq!(time.fg, Color::Reset);
        assert_eq!(time.modifier, Modifier::DIM);
        view.event_panel.last_mut().unwrap().kind = kind;
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
        let time = &buf[(97, top + 4)];
        assert_ne!(time.symbol(), " ");
        assert_eq!(time.fg, colour);
        assert!(time.modifier.contains(Modifier::BOLD));
        assert!(!time.modifier.contains(Modifier::DIM));
    }
}

#[test]
fn task_event_preserves_time_when_truncated() {
    use crate::event::Kind;
    use ratatui::style::Modifier;
    let (view, now) = event_fixture();
    let area = Rect::new(0, 0, 100, 25);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let y = view.center_layout(area).event_panel.bottom() - 2;
    assert_eq!(buf[(87, y)].symbol(), "…");
    assert_eq!(buf[(97, y)].fg, Color::Reset);
    assert_eq!(buf[(97, y)].modifier, Modifier::DIM);
    for kind in [
        Kind::User,
        Kind::Assistant,
        Kind::Agent,
        Kind::Session,
        Kind::QueuedUser,
        Kind::QueuedSystem,
    ] {
        let mut view = event_fixture().0;
        let event = view.event_panel.last_mut().unwrap();
        event.kind = kind;
        assert!(
            event_screen(&view, 100, 25, now)
                .lines()
                .any(|line| line.ends_with("30s ago"))
        );
    }
}

#[test]
fn task_event_right_aligns_mixed_width_times_and_centers_content() {
    use crate::event::Kind;
    let (mut view, now) = event_fixture();
    view.event_panel.truncate(3);
    view.select_event(None);
    view.event_panel[0].kind = Kind::Tool;
    view.event_panel[0].previous_end = Some(now - Duration::seconds(123));
    view.event_panel[0].ended_at = Some(now - Duration::seconds(120));
    view.event_panel[1].started_at = now + Duration::seconds(225);
    view.event_panel[1].previous_end = None;
    view.event_panel[2].kind = Kind::Assistant;
    view.event_panel[2].started_at = now - Duration::minutes(44);
    let rendered = event_screen(&view, 100, 25, now);
    assert_eq!(
        rendered
            .lines()
            .skip(view.center_layout(Rect::new(0, 0, 100, 25)).event_panel.y as usize)
            .take(3)
            .map(|line| line.trim_start().chars().take(9).collect::<String>())
            .collect::<Vec<_>>(),
        ["Please in", "I will ch", "Read src/"]
    );
    for (index, (line, (time, label))) in rendered
        .lines()
        .skip(view.center_layout(Rect::new(0, 0, 100, 25)).event_panel.y as usize)
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
            98,
            "times end at the same column"
        );
        let body = super::event_list::body_line(&view.event_panel[index], 88, &view.rows[0].cwd);
        assert_eq!(line.find(label), Some(50 - body.width() / 2));
    }
}

#[test]
fn task_event_shows_only_visible_events_and_hides_without_selection() {
    let (mut view, now) = event_fixture();
    let first = crate::event::Event {
        turn: None,
        source: None,
        completed_time: false,
        background: false,
        source_records: Vec::new(),
        sender: None,
        detail_body: None,
        kind: crate::event::Kind::User,
        body: "Discarded event".into(),
        paths: Vec::new(),
        started_at: now - Duration::seconds(180),
        ended_at: None,
        duration: None,
        previous_end: None,
        images: Vec::new(),
        calls: Vec::new(),
    };
    view.event_panel.insert(0, first);
    view.event_panel
        .extend(vec![view.event_panel.last().unwrap().clone(); 2]);
    assert!(!event_screen(&view, 100, 25, now).contains("Discarded event"));
    view.selected = usize::MAX;
    let rendered = event_screen(&view, 100, 25, now);
    assert!(!rendered.contains("Please inspect"));
    assert!(!rendered.lines().nth(14).unwrap().contains('─'));
}

#[test]
fn center_layout_medium_buffer_ranges_at_120_columns() {
    let (view, now) = event_fixture();
    let area = Rect::new(0, 0, 120, 60);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let layout = view.center_layout(area);
    assert_eq!(
        [layout.list, layout.task_details, layout.event_panel].map(|rect| rect.x..rect.right()),
        [1..59, 62..119, 1..119],
    );
    for y in layout.list.y..layout.list.bottom() {
        let separators: Vec<_> = (0..120).filter(|&x| buf[(x, y)].symbol() == "│").collect();
        assert_eq!(separators, [60]);
    }
    assert!(layout.event_panel.y >= layout.list.bottom());
}

#[test]
fn center_layout_large_splits_content_by_golden_ratio_across_sizes() {
    for (width, height) in [(200, 60), (200, 61), (180, 40), (160, 50), (150, 50)] {
        assert_content_shares_buffer(width, height, "");
    }
}

#[test]
fn center_layout_large_golden_content_buffer_at_200x45() {
    assert_content_shares_buffer(200, 45, "");
}

#[test]
fn center_layout_content_shares_with_growing_input() {
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
            assert_content_shares_buffer(width, height, &vec!["input"; lines].join("\n"));
        }
        assert_content_shares_buffer(width, height, &"wrapped input ".repeat(100));
    }
}

#[test]
fn center_layout_medium_balances_inner_content_across_sizes() {
    for (width, height) in [(120, 60), (120, 61), (100, 40), (149, 50)] {
        assert_content_shares_buffer(width, height, "");
    }
}

#[test]
fn center_layout_medium_equal_content_buffer_at_100x45() {
    assert_content_shares_buffer(100, 45, "");
}

fn assert_content_shares_buffer(width: u16, height: u16, input: &str) {
    let (mut view, now) = event_fixture();
    view.input_box.editor.set_text(input);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let layout = view.center_layout(area);
    let top = (0..height)
        .filter(|&y| buf[(layout.gap.x + 1, y)].symbol() == "│")
        .count() as u16;
    let large = super::render::ScreenLayout::for_width(width) == super::render::ScreenLayout::Large;
    let available = height
        - layout.header.height
        - layout.footer.height
        - layout.prompt.height
        - 2
        - u16::from(!large);
    let detail = layout.event_detail.height.saturating_sub(1);
    // Count output content between its two rules.
    assert_eq!(detail, layout.footer.y - layout.event_detail.y - 2);
    let heights = if large {
        vec![top, detail]
    } else {
        vec![top, layout.event_panel.height, detail]
    };
    assert_eq!(heights.iter().sum::<u16>() + 1, available);
    assert!(heights.windows(2).all(|pair| pair[0] >= pair[1]));
    if !large {
        assert!(heights[0] - heights[heights.len() - 1] <= 2);
    }
    let expected = if large {
        let upper = (u32::from(available) * 618 / 1000) as u16;
        (upper, upper, available - upper - 1)
    } else {
        (
            available.div_ceil(3),
            (available + 1) / 3,
            available / 3 - 1,
        )
    };
    println!(
        "{width}x{height}, input {} rows: top {top}, log {}, detail {detail}",
        layout.prompt.height, layout.event_panel.height
    );
    assert_eq!((top, layout.event_panel.height, detail), expected);
    assert_eq!(layout.task_details.height, top);
    assert_eq!(layout.prompt.y, layout.task_details.bottom() + 1);
    let status_left = layout.gap.x + 1;
    let status_right = if large {
        layout.event_panel.x - 1
    } else {
        area.right()
    };
    assert_eq!(
        buf[(
            status_left + (status_right - status_left - 9) / 2 + 1,
            layout.header.y + 1,
        )]
            .symbol(),
        "W"
    );
    assert_eq!(
        buf[(layout.task_details.x, layout.event_detail.y)].symbol(),
        "─"
    );
    if large {
        assert_eq!(layout.prompt.bottom(), layout.event_detail.y);
    } else {
        assert_eq!(layout.prompt.bottom() + 1, layout.event_panel.y);
        assert_eq!(layout.event_panel.bottom(), layout.event_detail.y);
        assert_eq!(layout.event_detail.bottom() + 1, layout.footer.y);
        assert!(buf[(0, layout.event_panel.y)].symbol() != "│");
    }
}

#[test]
fn center_layout_large_buffer_ranges_at_200_columns() {
    assert_large_buffer_ranges(200, [1..49, 52..100, 103..199]);
}

#[test]
fn center_layout_large_buffer_ranges_at_151_columns() {
    assert_large_buffer_ranges(151, [1..36, 39..74, 77..150]);
}

fn assert_large_buffer_ranges(width: u16, expected: [std::ops::Range<u16>; 3]) {
    let (view, now) = event_fixture();
    let area = Rect::new(0, 0, width, 60);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let layout = view.center_layout(area);
    assert_eq!(
        [layout.list, layout.task_details, layout.event_panel].map(|rect| rect.x..rect.right()),
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
    let (view, _) = event_fixture();
    for (
        width,
        height,
        top,
        event_height,
        list_width,
        task_details_width,
        body_height,
        list_height,
    ) in [
        (100, 24, 17, 6, 48, 47, 6, 6),
        (120, 40, 27, 12, 58, 57, 11, 11),
        (200, 60, 37, 22, 48, 48, 33, 33),
        (100, 24, 17, 6, 48, 47, 6, 6),
    ] {
        let area = Rect::new(5, 7, width, height);
        let layout = view.center_layout(area);
        assert_eq!(layout.header, Rect::new(5, 7, width, 2));
        assert_eq!(layout.list, Rect::new(6, 9, list_width, body_height));
        let bottom = if super::render::ScreenLayout::for_width(width)
            == super::render::ScreenLayout::Large
        {
            assert_eq!(layout.event_panel.y, layout.list.y);
            assert_eq!(layout.event_panel.width, width - 8 - 2 * list_width);
            layout.event_detail
        } else {
            assert_eq!(
                layout.event_panel,
                Rect::new(6, 9 + body_height + 3, width - 2, list_height)
            );
            layout.event_detail
        };
        assert_eq!(bottom, Rect::new(5, 7 + top, width, event_height - 1));
        assert_eq!(layout.footer, Rect::new(5, 7 + height - 1, width, 1));
        if task_details_width == 0 {
            assert_eq!(layout.task_details, Rect::default());
            assert_eq!(layout.gap, Rect::default());
        } else {
            assert_eq!(layout.gap, Rect::new(6 + list_width, 9, 3, body_height));
            assert_eq!(
                layout.task_details,
                Rect::new(9 + list_width, 9, task_details_width, body_height)
            );
        }
    }
}

#[test]
fn center_layout_event_and_task_details_hide_thresholds() {
    let (mut view, _) = event_fixture();
    for height in 0..19 {
        let layout = view.center_layout(Rect::new(0, 0, 100, height));
        assert_eq!(layout.event_panel.height, 0, "height {height}");
    }
    let layout = view.center_layout(Rect::new(0, 0, 100, 19));
    assert_eq!(layout.event_panel.height, 4);
    assert_eq!(layout.list.height, 4);
    assert_eq!(layout.event_detail.height, 4);
    assert!(layout.task_details.width > 0);
    assert!(
        !view
            .center_layout(Rect::new(0, 0, 100, 20))
            .task_details
            .is_empty()
    );
    let panels = |view: &View, height| {
        let layout = view.center_layout(Rect::new(0, 0, 120, height));
        [
            layout.list,
            layout.task_details,
            layout.event_panel,
            layout.prompt,
            layout.event_detail,
        ]
    };
    let with_transcript = [24, 40, 60].map(|height| panels(&view, height));
    view.rows[view.selected].transcript = None;
    for (height, expected) in [24, 40, 60].into_iter().zip(with_transcript) {
        assert_eq!(panels(&view, height), expected, "height {height}");
    }
}

#[test]
fn task_event_count_follows_height() {
    let (mut view, now) = event_fixture();
    let event = view.event_panel[0].clone();
    view.event_panel = (0..30)
        .map(|index| {
            let mut event = event.clone();
            event.body = format!("event-{index:02}");
            event
        })
        .collect();
    view.select_event(None);
    for (height, count) in [(24, 6), (40, 11), (60, 18), (18, 0), (24, 6)] {
        let rendered = event_list_screen(&view, 120, height, now);
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
                            .event_panel
                            .bottom()
                            - 1
                    ))
                    .unwrap()
                    .contains("event-")
            );
        }
    }
}

#[path = "section_scroll_tests.rs"]
mod section_scroll_tests;

#[test]
fn task_details_status_shows_only_the_directory_name() {
    let task = session("path", "Path task", SessionStatus::Ready, 0);
    let directory = crate::test_project();
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![task], false);
    let rendered = screen(&view, 200, 30);
    let viewport = view.viewport.borrow();
    let (rect, path) = viewport
        .task_details_links
        .iter()
        .find(|(_, path)| **path == directory)
        .unwrap();
    assert_eq!(rect.y, viewport.task_details.bottom());
    assert_eq!(path, &directory);
    let rule = rendered.lines().nth(rect.y as usize).unwrap();
    assert!(rule.contains("project"), "{rule}");
    assert!(!rule.contains("agents-tests"));
}

#[test]
fn group_header_shows_the_directory_basename() {
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
                .map(|cell| cell_text(cell.symbol()))
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .find(|row| row.contains("agents-panel"))
        .unwrap();
    assert_eq!(heading.trim(), "agents-panel");
}

mod event_path_abbrev {
    use super::*;

    #[test]
    fn read_path_is_abbreviated_to_the_available_body_width() {
        let (mut view, now) = event_fixture();
        let mut event = view.event_panel[2].clone();
        let home = std::env::var_os("HOME").expect("HOME");
        let path = std::path::Path::new(&home).join("Users/workspace/project/main.rs");
        event.body = format!("Read {}", path.display());
        event.paths.push(5..event.body.len());
        view.event_panel = vec![event];
        view.select_event(None);
        let area = Rect::new(0, 0, 43, 4);
        let mut buf = Buffer::empty(area);
        view.render_event(area, &mut buf, now);
        let row = buf
            .content
            .chunks(43)
            .next()
            .unwrap()
            .iter()
            .map(|cell| cell_text(cell.symbol()))
            .collect::<String>();
        assert!(row.contains("Read ~/…/project/main.rs"), "{row}");
    }
}

mod event_help {
    use super::*;

    #[test]
    fn help_has_tasks_events_and_navigation_columns() {
        let mut view = simple_fixture();
        key(&mut view, KeyCode::Char('?'));
        let rendered = screen(&view, 120, 24);
        let headings = rendered
            .lines()
            .find(|line| line.contains("Tasks"))
            .unwrap();
        assert_eq!(
            headings.split_whitespace().collect::<Vec<_>>(),
            ["Tasks", "Events", "Navigate"]
        );
        if std::env::var_os("UPDATE_HELP_SNAPSHOTS").is_some() {
            std::fs::write(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/ui_snapshots/live_center_help_event.txt"
                ),
                &rendered,
            )
            .unwrap();
        }
        assert_eq!(
            rendered,
            include_str!("../../tests/ui_snapshots/live_center_help_event.txt")
        );
    }
}

mod event_search {
    use super::*;
    use ratatui::style::Modifier;

    fn open(view: &mut View) {
        {
            view.active_panel = Panel::EventList;
            view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        };
    }

    fn panel(view: &View, now: chrono::DateTime<Utc>) -> Buffer {
        let area = Rect::new(0, 0, 100, 12);
        let mut buf = Buffer::empty(area);
        view.render_event(area, &mut buf, now);
        buf
    }

    fn text(buf: &Buffer) -> String {
        buf.content
            .iter()
            .map(|cell| cell_text(cell.symbol()))
            .collect()
    }

    #[test]
    fn leaving_list_search_preserves_event_and_detail_search() {
        for mouse_exit in [false, true] {
            let (mut view, now) = event_fixture();
            view.select_event(Some(1));
            let selected = view.visible_event().unwrap().clone();
            open(&mut view);
            for c in "files".chars() {
                key(&mut view, KeyCode::Char(c));
            }
            assert_eq!(view.visible_event(), Some(&selected));
            screen(&view, 120, 50);
            let detail = view.viewport.borrow().event_detail;
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
                view.active_panel = Panel::EventDetail;
            }
            assert_eq!(view.event_list.search.active(), mouse_exit);
            assert_eq!(view.event_list.search.editing, mouse_exit);
            assert_eq!(view.visible_event(), Some(&selected));
            let area = Rect::new(0, 0, 100, 40);
            let mut buf = Buffer::empty(area);
            view.render_event_detail(area, &mut buf, now);
            assert!(text(&buf).contains("I will check the files"));
            key(&mut view, KeyCode::Char('/'));
            for c in "files".chars() {
                key(&mut view, KeyCode::Char(c));
            }
            assert!(view.event_detail.search.editing);
            let mut buf = Buffer::empty(area);
            view.render_event_detail(area, &mut buf, now);
            assert!(text(&buf).contains("I will check the files"));
            assert!(text(&buf).contains("Search ❯ files"));
        }
    }

    #[test]
    fn input_live_match_highlight_keep_and_escape() {
        let _guard = crate::theme::TEST_LOCK.lock().unwrap();
        let (mut view, now) = event_fixture();
        let template = view.event_panel[0].clone();
        view.event_panel = [
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
        assert_eq!(view.center_footer_hints().to_string(), "? help");
        for c in "time out".chars() {
            if c == ' ' {
                view.handle_paste(" ");
            } else {
                key(&mut view, KeyCode::Char(c));
            }
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
        use crate::event::Kind;
        let (mut view, now) = event_fixture();
        let template = view.event_panel[0].clone();
        view.event_panel = [
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
            view.handle_paste(query);
            let matches: Vec<_> = (0..view.event_panel.len())
                .filter(|&i| view.event_matches(i))
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
        let (mut view, now) = event_fixture();
        open(&mut view);
        key(&mut view, KeyCode::Char('a'));
        screen(&view, 120, 40);
        let rect = view.viewport.borrow().event_panel;
        view.handle_mouse(mouse(
            MouseEventKind::ScrollUp,
            rect.x,
            rect.y + super::event_list::EVENT_TOP,
        ));
        assert_eq!(view.visible_event(), view.event_panel.get(5));
        let mut second = view.rows[0].clone();
        second.id = session_key("second");
        view.set_sessions(vec![view.rows[0].clone(), second], false);
        let index = view
            .rows
            .iter()
            .position(|row| row.id == session_key("second"))
            .unwrap();
        view.set_selected(index);
        view.event_session = Some(session_key("second"));
        let buf = panel(&view, now);
        assert!(!text(&buf).contains("Search ❯ "));
        assert_eq!(view.center_footer_hints().to_string(), "? help");
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
    fn event_search_caret_follows_cjk_inset_text() {
        let (mut view, now) = event_fixture();
        {
            view.active_panel = Panel::EventList;
            view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        };
        for c in "你好".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let y = view.center_layout(area).event_panel.bottom() - 1;
        assert_eq!(cell_text(buf[(10, y)].symbol()), "你");
        assert_eq!(cell_text(buf[(12, y)].symbol()), "好");
        assert_eq!(view.cursor_pos(area), Some((14, y)));
    }

    #[test]
    fn rename_inserts_in_the_middle() {
        let mut view = fixture();
        key(&mut view, KeyCode::Char('r'));
        view.set_active_panel(Panel::TaskDetails);
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
        let (mut task, now) = event_fixture();
        key(&mut task, KeyCode::Char('/'));
        task.search.set_text(query);
        let (mut event_panel, _) = event_fixture();
        {
            event_panel.active_panel = Panel::EventList;
            event_panel.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
        };
        event_panel.event_list.search.query.set_text(query);
        if left {
            task.search.left();
            event_panel.event_list.search.query.left();
        }
        let mut task_buf = Buffer::empty(area);
        task.render_at(area, &mut task_buf, now);
        let mut event_buf = Buffer::empty(area);
        event_panel.render_at(area, &mut event_buf, now);
        let task_row = task.center_layout(area).search;
        let event_row = super::super::event_list::search_input_rect(
            event_panel.center_layout(area).event_panel,
        );
        (task_buf, task_row, event_buf, event_row, task, event_panel)
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
                    let (task, task_row, mut event_panel, mut event_row, _, event_view) =
                        rendered_inputs(width, "你好 query", false);
                    Clear.render(event_row, &mut event_panel);
                    event_row.width = task_row.width;
                    super::super::render::render_input_row(
                        "Search ❯ ",
                        &event_view.event_list.search.query,
                        event_row,
                        &mut event_panel,
                    );
                    assert_eq!(task_row.x, event_row.x);
                    for x in 0..task_row.width {
                        let task_cell = &task[(task_row.x + x, task_row.y)];
                        let event_cell = &event_panel[(event_row.x + x, event_row.y)];
                        assert_eq!(task_cell, event_cell, "width={width}, column={x}");
                        assert_eq!(task_cell.bg, Color::Reset);
                    }
                }
            });
        }
    }

    #[test]
    fn search_prompt_keeps_cyan_without_a_row_background_for_wide_text() {
        use super::super::style::{selection_style_for, with_selection_style};
        let light = selection_style_for(false, true, true);
        with_selection_style(light, || {
            let (task, row, _, _, _, _) = rendered_inputs(24, "你好", false);
            assert_eq!(task[(row.x, row.y)].fg, Color::Cyan);
            for x in row.x..row.right() {
                assert_eq!(task[(x, row.y)].bg, Color::Reset);
            }
        });
    }

    #[test]
    fn search_carets_follow_the_same_visible_window() {
        for width in [8, 14, 24, 88] {
            for query in ["", "abc", "你好 query that scrolls past the edge"] {
                for left in [false, true] {
                    let (_, task_row, _, event_row, task, event_panel) =
                        rendered_inputs(width, query, left);
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
                    let event_expected = (9 + event_panel
                        .event_list
                        .search
                        .query
                        .window(usize::from(event_row.width.saturating_sub(9)))
                        .1)
                        .min(event_row.width.saturating_sub(1));
                    assert_eq!(
                        event_panel.cursor_pos(area),
                        Some((event_row.x + event_expected, event_row.y))
                    );
                }
            }
        }
    }
}

#[test]
fn task_event_search_active_snapshot() {
    let (mut view, now) = event_fixture();
    {
        view.active_panel = Panel::EventList;
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE))
    };
    for c in "tests".chars() {
        key(&mut view, KeyCode::Char(c));
    }
    assert_eq!(
        event_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_event_search_active.txt")
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
                    .map(|c| cell_text(c.symbol()))
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
                                .map(|x| cell_text(buf[(x, y)].symbol()))
                                .collect::<String>()
                                .contains(&format!("Child {index}"))
                        })
                        .unwrap();
                    let prefix = (list.x..list.x + 3)
                        .map(|x| cell_text(buf[(x, y)].symbol()))
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
        assert_eq!(ids(&view), ["parent", "agent-older", "agent-newer"]);
    }

    #[test]
    fn status_tabs_filter_each_child_and_count_visible_ancestors() {
        for provider in [ProviderId::Claude, ProviderId::Codex] {
            let mut parent = session("parent", "Parent task", SessionStatus::Working, 120);
            parent.id.provider = provider;
            let mut working = session("working-child", "Working child", SessionStatus::Working, 60);
            working.id.provider = provider;
            working.parent = Some(parent.id.clone());
            let mut inactive = session(
                "inactive-child",
                "Inactive child",
                SessionStatus::Inactive,
                0,
            );
            inactive.id.provider = provider;
            inactive.parent = Some(parent.id.clone());
            let mut view = test_view(Grouping::Project, false);
            view.set_sessions(
                vec![parent.clone(), working.clone(), inactive.clone()],
                false,
            );
            for (tab, child) in [(0, &working), (1, &inactive)] {
                set_filter(&mut view, tab);
                assert_eq!(ids(&view), ["parent", child.id.id.as_str()]);
                let rendered = screen(&view, 160, 30);
                assert!(rendered.contains("Active 2"), "{rendered}");
                assert!(rendered.contains("Inactive 2"), "{rendered}");
                for selected in [&parent, child] {
                    let index = view
                        .rows
                        .iter()
                        .position(|row| row.id == selected.id)
                        .unwrap();
                    view.select(index);
                    assert_eq!(view.status_filter, tab);
                    view.set_sessions(view.rows.clone(), false);
                    assert_eq!(view.status_filter, tab);
                    assert_eq!(view.selected_id(), Some(&selected.id));
                }
            }
        }
    }

    #[test]
    fn status_tabs_keep_inactive_ancestors_of_working_grandchildren() {
        for provider in [ProviderId::Claude, ProviderId::Codex] {
            let mut parent = session("parent", "Parent", SessionStatus::Inactive, 120);
            parent.id.provider = provider;
            let mut middle = session("middle", "Middle", SessionStatus::Inactive, 60);
            middle.id.provider = provider;
            middle.parent = Some(parent.id.clone());
            let mut grandchild = session("grandchild", "Grandchild", SessionStatus::Working, 0);
            grandchild.id.provider = provider;
            grandchild.parent = Some(middle.id.clone());
            let mut view = test_view(Grouping::Project, false);
            view.set_sessions(vec![parent, middle, grandchild], false);
            assert_eq!(ids(&view), ["parent", "middle", "grandchild"]);
            let rendered = screen(&view, 160, 30);
            assert!(rendered.contains("Active 3"), "{rendered}");
            assert!(rendered.contains("Inactive 2"), "{rendered}");
            set_filter(&mut view, 1);
            assert_eq!(ids(&view), ["parent", "middle"]);
        }
    }

    #[test]
    fn inactive_ancestors_keep_their_own_status_and_time() {
        for provider in [ProviderId::Claude, ProviderId::Codex] {
            for status in [SessionStatus::Working, SessionStatus::Ready] {
                let mut parent = session("parent", "Parent task", status, 120);
                parent.id.provider = provider;
                let reference: chrono::DateTime<Utc> = "2026-10-13T12:00:00Z".parse().unwrap();
                parent.updated_at = reference - Duration::minutes(2);
                let mut child = session("child", "Finished child", SessionStatus::Inactive, 60);
                child.id.provider = provider;
                child.updated_at = reference - Duration::minutes(1);
                child.parent = Some(parent.id.clone());
                let mut view = test_view(Grouping::Project, false);
                view.set_sessions(vec![parent, child], false);
                let active = screen(&view, 200, 30);
                let parent_line = active
                    .lines()
                    .find(|line| line.contains("Parent task"))
                    .unwrap();
                assert!(
                    parent_line
                        .split('│')
                        .next()
                        .unwrap()
                        .contains(status.label()),
                    "{active}"
                );
                set_filter(&mut view, 1);
                let inactive = screen(&view, 200, 30);
                let parent_line = inactive
                    .lines()
                    .find(|line| line.contains("Parent task"))
                    .unwrap();
                assert!(
                    parent_line
                        .split('│')
                        .next()
                        .unwrap()
                        .contains(status.label()),
                    "{inactive}"
                );
                assert!(parent_line.contains("2m ago"), "{inactive}");
                assert_eq!(ids(&view), ["parent", "child"]);
                if provider == ProviderId::Claude && status == SessionStatus::Working {
                    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/ui_snapshots/inactive_working_parent.txt");
                    if std::env::var_os("UPDATE_INACTIVE_TREE_SNAPSHOT").is_some() {
                        std::fs::write(&path, &inactive).unwrap();
                    }
                    assert_eq!(inactive, std::fs::read_to_string(path).unwrap());
                }
                let child_line = inactive
                    .lines()
                    .find(|line| line.contains("Finished child"))
                    .unwrap();
                assert!(
                    child_line.split('│').next().unwrap().contains("Inactive"),
                    "{inactive}"
                );
            }
        }
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
    fn inactive_root_keeps_matching_children_in_active_tab() {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        rows.iter_mut()
            .find(|row| row.id == session_key("parent"))
            .unwrap()
            .status = SessionStatus::Inactive;
        rows.push(session("other", "Other task", SessionStatus::Working, 0));
        view.set_sessions(rows, false);
        set_filter(&mut view, 0);
        assert_eq!(
            ids(&view),
            ["other", "parent", "agent-newer", "agent-older"]
        );
        key(&mut view, KeyCode::Home);
        assert_eq!(view.selected_id(), Some(&session_key("other")));
        assert!(screen(&view, 120, 30).contains("Parent task"));
        set_filter(&mut view, 1);
        assert_eq!(ids(&view), ["parent"]);
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
                view.set_pinned(&crate::state::TaskPins {
                    active: pins.clone(),
                    inactive: vec![],
                });
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
        view.set_pinned(&crate::state::TaskPins {
            active: vec![session_key("parent")],
            inactive: vec![],
        });
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
            view.set_pinned(&crate::state::TaskPins {
                active: vec![],
                inactive: vec![],
            });
            set_filter(&mut view, 0);
            assert_eq!(
                ids(&view),
                ["other", "parent", "agent-newer", "agent-older"]
            );
            view.set_pinned(&crate::state::TaskPins {
                active: vec![session_key("parent")],
                inactive: vec![],
            });
            assert_eq!(
                ids(&view),
                ["parent", "agent-newer", "agent-older", "other"]
            );
            set_filter(&mut view, 1);
            assert_eq!(ids(&view), ["parent"]);
            assert!(!screen(&view, 120, 30).contains("Pinned"));
        }
    }

    #[test]
    fn keyboard_and_mouse_select_child_and_show_task_details() {
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
        assert!(!header.contains("Check files"), "{header}");
        let bottom = rendered
            .lines()
            .nth(usize::from(view.viewport.borrow().task_details.bottom()))
            .unwrap();
        assert!(bottom.contains("Check files"), "{bottom}");
        let rendered = screen(&view, 120, 30);
        assert!(rendered.contains("child-model"));
        assert!(!rendered.contains("Needs input"));
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
    fn children_disappear_with_missing_parent() {
        let mut view = fixture();
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
            assert_eq!(cell_text(buf[(5, row)].symbol()), "界");
            if width >= 57 {
                let text: String = (0..width)
                    .map(|x| cell_text(buf[(x, row)].symbol()))
                    .collect();
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
        assert!(view.pinned.active.is_empty());
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
                    if let Some(notice) = &notice {
                        view.show_error(notice.clone());
                    }
                    let before = subagent_rows::screen(&view, 120, 30);
                    assert_eq!(
                        view.handle_key(KeyEvent::new($code, $modifiers)),
                        Some(Action::Unsupported($label))
                    );
                    assert_eq!(
                        view.feedback.clone().map(|(feedback, _)| feedback),
                        notice.map(footer::Feedback::Error)
                    );
                    assert_unchanged(&view, &before);
                }
            }
        };
    }

    #[test]
    fn control_l() {
        for notice in [None, Some("Keep this notice".to_owned())] {
            let mut view = fixture();
            if let Some(notice) = &notice {
                view.show_error(notice.clone());
            }
            let before = subagent_rows::screen(&view, 120, 30);
            assert_eq!(
                view.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
                None
            );
            assert_eq!(
                view.feedback.clone().map(|(feedback, _)| feedback),
                notice.map(footer::Feedback::Error)
            );
            assert_unchanged(&view, &before);
        }
    }
    unsupported_key!(rename, KeyCode::Char('r'), KeyModifiers::NONE, "Rename");
    unsupported_key!(fork, KeyCode::Char('f'), KeyModifiers::NONE, "Fork");
    unsupported_key!(stop, KeyCode::Char('x'), KeyModifiers::NONE, "Stop");
    unsupported_key!(delete, KeyCode::Backspace, KeyModifiers::NONE, "Delete");
    unsupported_key!(archive, KeyCode::Char('a'), KeyModifiers::NONE, "Archive");
    #[test]
    fn pin() {
        let mut view = fixture();
        assert_eq!(
            key(&mut view, KeyCode::Char('p')),
            Some(Action::TogglePin {
                tab: 0,
                id: session_key("agent-older"),
                pinned: true,
            })
        );
    }

    #[test]
    fn open_keys_leave_disabled_subagent_on_list() {
        for code in [KeyCode::Enter, KeyCode::Char('o')] {
            let mut view = fixture();
            assert_eq!(key(&mut view, code), None);
            assert_eq!(view.active_panel, Panel::TaskList);
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
            assert_eq!(view.handle_key(event), None);
            assert!(view.searching);
            assert_eq!(view.search.text(), "Parent task");
            assert_eq!(subagent_rows::screen(&view, 120, 30), before);
        }
    }

    #[test]
    fn footer_snapshot() {
        let view = fixture();
        assert_eq!(view.center_footer_hints().to_string(), "? help");
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
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.active_panel, Panel::Prompt);
        view.set_active_panel(Panel::TaskList);
    }
    for (code, name) in [
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
fn unsupported_provider_mouse_selection_then_open_focuses_prompt() {
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
    assert!(!view.prompt_focused());
    key(&mut view, KeyCode::Esc);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.active_panel, Panel::Prompt);
}

#[test]
fn missing_provider_capabilities_do_not_block_prompt_focus() {
    let mut view = fixture();
    view.set_capabilities(HashMap::new());
    assert_eq!(key(&mut view, KeyCode::Char('o')), None);
    assert_eq!(view.active_panel, Panel::Prompt);
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
        view.render_task_details(area, &mut buf, now);
        buf.content
            .chunks(160)
            .map(|row| {
                let mut text = String::new();
                let mut cells = row.iter();
                while let Some(cell) = cells.next() {
                    text.push_str(cell_text(cell.symbol()));
                    for _ in 1..cell_text(cell.symbol()).width() {
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
            "Needs input\n{}",
            row.pending_questions
                .iter()
                .map(|question| format!("                {}", question.replace('`', "")))
                .collect::<Vec<_>>()
                .join("\n")
        )),
        "{pending}"
    );
    assert!(!pending.contains("Open task to review."));
    assert_eq!(
        pending,
        include_str!("../../tests/ui_snapshots/task_details_questions_pending.txt")
    );
    row.pending_questions.clear();
    view.set_sessions(vec![row.clone()], false);
    let answered = render(&view);
    assert!(answered.contains("Needs input\n                                                                   Open task to review.\n                                                                   Waiting for your response."));
    assert_eq!(
        answered,
        include_str!("../../tests/ui_snapshots/task_details_questions_answered.txt")
    );
    row.status = SessionStatus::Working;
    row.pending_questions.push("cd 要不要显示？".into());
    view.set_sessions(vec![row], false);
    assert!(!render(&view).contains("Needs input"));
}

#[test]
fn pending_question_details_use_model_color_and_bold_attention_title() {
    let mut row = session("questions", "Question task", SessionStatus::NeedsInput, 0);
    row.pending_questions = vec!["Pick a color?".into()];
    row.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::event::Kind::Assistant,
        label: "Pick a color?".into(),
        at: Utc::now(),
    });
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 0;
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 100, 40);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, Utc::now());
    let mut questions = 0;
    for cells in buf.content.chunks(100) {
        let text = cells
            .iter()
            .map(|cell| cell_text(cell.symbol()))
            .collect::<String>();
        if text.trim_start().starts_with("Pick a color?") {
            questions += 1;
            assert_eq!(cells[43].fg, Color::Yellow);
        }
        if text.starts_with("Needs input") {
            assert_eq!(cells[0].fg, Color::LightRed);
            assert!(cells[0].modifier.contains(ratatui::style::Modifier::BOLD));
        }
    }
    assert_eq!(questions, 2);
}

#[test]
fn pending_question_event_uses_model_color() {
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
        let (mut view, now) = event_fixture();
        let row = &mut view.rows[0];
        let path = root.join(fixture);
        row.transcript = Some(path.clone());
        let events = if fixture.contains("codex-input") {
            codex.event_panel(row, 8, false).unwrap()
        } else {
            crate::providers::claude::event_reader::tail(&path, 8).unwrap()
        };
        let question = events.last().unwrap();
        assert_eq!(
            style::event_styles(&question.kind).1.fg,
            Some(Color::Yellow)
        );
        view.event_panel = events;
        let area = Rect::new(0, 0, 160, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let viewport = view.viewport.borrow();
        let event_panel = viewport.event_panel;
        let y = event_panel.y;
        let x = (event_panel.x..event_panel.right() - 10)
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

fn click_event_text(view: &mut View, screen: &str, text: &str, button: MouseButton) {
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
fn task_event_click_keeps_list_and_detail_right_click_keeps_selection() {
    let (mut view, now) = event_fixture();
    view.event_panel[6].body = "A long response with **bold** words whose body must be cut before the right edge of the log\n\nA second paragraph".into();
    let list = event_screen(&view, 100, 45, now);
    let event_panel = view.viewport.borrow().event_panel;
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            event_panel.x + 30,
            event_panel.y + 6,
        ),
    );
    assert_eq!(view.visible_event(), view.event_panel.get(6));
    let selected = event_screen(&view, 100, 45, now);
    assert!(selected.contains("A second paragraph"));
    click_event_text(&mut view, &list, "A long response", MouseButton::Right);
    assert_eq!(view.visible_event(), view.event_panel.last());
    click_event_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = event_screen(&view, 100, 45, now);
    assert_eq!(
        open,
        include_str!("../../tests/ui_snapshots/task_event_open.txt")
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
        view.event_redraw_in(now),
        Some(std::time::Duration::from_secs(1))
    );
}

fn event_detail_parts(event: &crate::event::Event) -> Vec<super::tool_detail::Part> {
    super::tool_detail::event_detail_content(event, String::new(), Style::new().dim())
        .sections
        .into_iter()
        .flat_map(|section| section.groups)
        .flat_map(|group| group.parts)
        .collect()
}

/// The calls of a merged Bash + Read row, the Read still running.
fn tool_calls() -> Vec<crate::event::ToolCall> {
    use crate::event::{Section, ToolCall};
    vec![
        ToolCall {
            agent_message: None,
            started_at: None,
            hide_output: false,
            model_settings: None,
            agent_thread_id: None,
            agent_path: None,
            event_detail_left: "Bash".into(),
            event_detail_title: "Bash".into(),
            show_event_detail: false,
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
            agent_message: None,
            started_at: None,
            hide_output: false,
            model_settings: None,
            agent_thread_id: None,
            agent_path: None,
            event_detail_left: "Read".into(),
            event_detail_title: "Read".into(),
            show_event_detail: false,
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

/// The event list fixture with its running "run tests" row carrying `tool_calls`, opened.
fn open_tool_fixture() -> (View, chrono::DateTime<Utc>) {
    let (mut view, now) = event_fixture();
    view.event_panel[7].calls = tool_calls();
    view.select_fixture_event(Some(view.event_panel[7].clone()));
    (view, now)
}

#[test]
fn task_event_click_opens_a_whole_row_that_has_calls() {
    let (mut view, now) = event_fixture();
    view.event_panel[7].calls = tool_calls();
    let list = event_screen(&view, 100, 25, now);
    click_event_text(&mut view, &list, "run tests", MouseButton::Left);
    assert_eq!(
        view.visible_event().cloned(),
        Some(view.event_panel[7].clone())
    );
    assert!(event_screen(&view, 100, 25, now).contains("cargo test"));
}

#[test]
fn merged_tool_titles_copy_only_titles_and_arrows_center_on_panel() {
    for (width, title) in [
        (100, "Bash"),
        (99, "看探针结果"),
        (30, "  Read 界面  Read 界面  Read 界面"),
    ] {
        let (mut view, now) = open_tool_fixture();
        let mut event = view.event_panel[7].clone();
        event.calls[0].title = title.into();
        event.calls[0].event_detail_title = title.into();
        view.select_fixture_event(Some(event));
        let height = if width < 100 { 90 } else { 48 };
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        view.render_at(buf.area, &mut buf, now);
        for title in [title, "Read"] {
            let (rows, body) = {
                let viewport = view.viewport.borrow();
                let region = &viewport.event_text;
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
            let panel = view.viewport.borrow().event_detail;
            assert_eq!(
                body.x + start,
                super::centering::centered_x(panel, body, end - start)
            );
            if start > 1 {
                assert_ne!(buf[(body.x + start - 2, row.y)].symbol(), "─");
            }
            if end < body.width - 1 {
                let last = " ";
                assert_eq!(cell_text(buf[(body.right() - 1, row.y)].symbol()), last);
            }
            if title != "Read" {
                let viewport = view.viewport.borrow();
                assert_eq!(row.y, viewport.event_text.rows.first().unwrap().y);
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
                    text: row
                        .cells
                        .iter()
                        .map(|(columns, _)| buf[(body.x + columns.start, row.y)].symbol())
                        .collect(),
                    label: "Copied message".into()
                })
            );
        }
    }
}

#[test]
fn shell_commands_share_center_and_copy_display_lines_without_connector_hits() {
    let command = "cat /workspace/a && printf longer && echo done; pwd\n";
    let (mut view, now) = open_tool_fixture();
    let mut event = view.event_panel[7].clone();
    event.calls.truncate(1);
    event.calls[0].input = vec![
        crate::event::Section::Shell {
            prefix: 0,
            text: command.into(),
        },
        crate::event::Section::Structured {
            label: "input".into(),
            text: r#"{"timeout":1000}"#.into(),
        },
    ];
    view.select_fixture_event(Some(event));
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 48));
    view.render_at(buf.area, &mut buf, now);
    let (body, rows) = {
        let viewport = view.viewport.borrow();
        let region = &viewport.event_text;
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
            viewport.event_detail,
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
            assert_eq!(cell_text(buf[(prefix_x, row.y)].symbol()), glyph);
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
                .event_links
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
    // A drag copies display lines; a whole-section copy also retains its trailing newline.
    assert_eq!(
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x2, last.y)),
        Some(Action::Copy {
            text: "cat /workspace/a\nprintf longer\necho done\npwd".into(),
            label: "Copied message".into(),
        })
    );
    let full_screen = event_screen(&view, 100, 48, now);
    assert_eq!(
        double_click_text(&mut view, &full_screen, "printf longer"),
        Some(Action::Copy {
            text: format!("{command}\n{{\n  \"timeout\": 1000\n}}\n\nrunning 3 tests\n..."),
            label: "Copied message".into(),
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
        include_str!("../../tests/ui_snapshots/task_event_shell_commands.txt")
    );
}

#[test]
fn running_tool_redraws_cached_detail_only_while_visible() {
    let (view, _) = open_tool_fixture();
    let start = chrono::DateTime::UNIX_EPOCH;
    for (millis, frame) in [(0, "•··"), (300, "·•·"), (600, "··•"), (900, "•··")] {
        let now = start + Duration::milliseconds(millis);
        let screen = event_screen(&view, 100, 48, now);
        assert!(screen.lines().any(|line| line.trim() == frame));
        let viewport = view.viewport.borrow();
        let region = &viewport.event_text;
        let row = region.rows.last().unwrap();
        assert!(row.cells.is_empty());
        let text = screen.lines().nth(usize::from(row.y)).unwrap();
        assert_eq!(
            text.find(frame),
            Some(usize::from(super::centering::centered_x(
                viewport.event_detail,
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
    event_screen(&view, 100, 25, start);
    assert_eq!(
        view.tool_redraw_in(start),
        None,
        "running row is below the viewport"
    );
    let (view, _) = event_fixture();
    event_screen(&view, 100, 48, start);
    assert_eq!(
        view.tool_redraw_in(start),
        None,
        "plain tool rows do not animate"
    );
}

#[test]
fn task_event_selected_tool_snapshot() {
    let (view, now) = open_tool_fixture();
    let open = event_screen(&view, 100, 48, now);
    assert_eq!(
        open,
        include_str!("../../tests/ui_snapshots/task_event_open_tool.txt")
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
        super::event_list::open_text(&view.event_panel[7]),
        "Bash\ncargo test\n\nrunning 3 tests\n...\n\nRead\nagents/src/log.rs:1-40"
    );
}

#[test]
fn task_event_selected_tool_highlighting_refreshes_when_theme_changes() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let events =
        crate::providers::claude::event_reader::tail(&root.join("claude-tool-calls.jsonl"), 100)
            .unwrap();
    let event = events
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Bash"))
        .unwrap();
    let (mut view, now) = event_fixture();
    view.select_fixture_event(Some(event));
    let render = || {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 48));
        view.render_at(buf.area, &mut buf, now);
        let area = view.viewport.borrow().event_text.area;
        let mut cells = Vec::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                cells.push((cell_text(buf[(x, y)].symbol()).to_owned(), buf[(x, y)].fg));
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
fn task_event_selected_tool_layout_is_cached_until_selection_or_width_changes() {
    let (mut view, now) = open_tool_fixture();
    assert!(view.viewport.borrow().event_detail_layout.is_none());
    let open = event_screen(&view, 100, 28, now);
    assert!(view.viewport.borrow().event_detail_layout.is_some());
    assert_eq!(event_screen(&view, 100, 28, now), open);
    let small = event_screen(&view, 60, 28, now);
    assert_ne!(small, open);
    assert!(small.contains('…'));
    assert!(
        view.viewport
            .borrow()
            .event_text
            .sources
            .iter()
            .any(|source| source.text.contains("running 3 tests"))
    );
    view.select_fixture_event(Some(view.event_panel[6].clone()));
    assert!(view.viewport.borrow().event_detail_layout.is_none());
    assert_eq!(view.visible_event(), view.event_panel.get(6));
}

#[test]
fn task_event_selected_tool_gains_its_output_when_the_event_refreshes() {
    let (mut view, now) = open_tool_fixture();
    let open = event_screen(&view, 100, 48, now);
    let mut events = view.event_panel.clone();
    events[7].calls[1].output = Some(vec![crate::event::Section::Text {
        label: "content".into(),
        text: "use chrono::{DateTime, Utc};".into(),
    }]);
    view.set_event(events.clone());
    assert_eq!(view.visible_event().cloned(), Some(events[7].clone()));
    let refreshed = event_screen(&view, 100, 48, now);
    assert_ne!(refreshed, open);
    assert!(refreshed.contains("use chrono::{DateTime, Utc};"));
    assert!(!refreshed.contains(super::tool_detail::running_frame(now)));
    view.set_event(view.event_panel[..7].to_vec());
    assert_eq!(view.visible_event(), view.event_panel.last());
}

#[test]
fn task_event_selected_tool_copies_a_dragged_section() {
    let (mut view, now) = open_tool_fixture();
    let open = event_screen(&view, 100, 48, now);
    let (x1, y1) = rendered_cell(&open, "cargo test");
    let (x2, y2) = rendered_cell(&open, "running 3 tests");
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x1, y1))
            .is_none()
    );
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x2 + 7, y2));
    event_screen(&view, 100, 48, now);
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
fn task_event_selected_event_header_centers_kind_and_keeps_time_right() {
    for body in ["A long response", "The newest reply"] {
        let (mut view, now) = event_fixture();
        // The newest row reads its time as "30s ago".
        view.event_panel[7].kind = crate::event::Kind::Assistant;
        view.event_panel[7].body = format!("The newest reply {}", "runs long ".repeat(20));
        let list = event_screen(&view, 100, 25, now);
        let row = list.lines().find(|line| line.contains(body)).unwrap();
        let time = row.chars().skip(91).take(8).collect::<String>();
        let time = time.trim();
        click_event_text(&mut view, &list, body, MouseButton::Left);
        let open = event_screen(&view, 100, 25, now);
        let detail = view.viewport.borrow().event_detail;
        let columns = super::event_detail::event_detail_columns(
            detail,
            view.center_layout(Rect::new(0, 0, 100, 25)).event_panel,
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
fn task_event_click_opens_row_with_shortened_path() {
    let (mut view, now) = event_fixture();
    let path = "/private/workspace/a-very-long-directory-name/another-long-directory/and-one-more-level/report.md";
    let event = &mut view.event_panel[7];
    event.kind = crate::event::Kind::Error;
    event
        .paths
        .push(event.body.len() + 1..event.body.len() + 1 + path.len());
    event.body = format!("{} {path}", event.body);
    view.set_event(view.event_panel.clone());
    let list = event_list_screen(&view, 100, 25, now);
    assert!(!list.contains(path), "{list}");
    click_event_text(&mut view, &list, "run tests", MouseButton::Left);
    let open = event_screen(&view, 100, 25, now);
    // The open event wraps the full path within the message-column width.
    assert!(
        open.contains("/private/workspace/a-very-long-directory-name"),
        "{open}"
    );
    assert!(open.contains("report.md"), "{open}");
    assert!(
        view.viewport
            .borrow()
            .event_text
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
fn missing_rooted_paths_stay_shortened_without_links_on_event_and_task_details() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let missing = dir
        .path()
        .join("a-very-long-directory-name/another-long-directory/report.md");
    for path in [missing.to_str().unwrap(), "~/C…/.w…/a…/s…/l…/plan.md"] {
        let filename = path.rsplit('/').next().unwrap();
        for kind in [crate::event::Kind::Assistant, crate::event::Kind::Tool] {
            let (mut view, now) = event_fixture();
            view.event_panel[7].kind = kind;
            view.event_panel[7].body = format!("Report: {path}");
            view.event_panel[7].paths.clear();
            view.event_panel[7].paths.push(8..8 + path.len());
            let list = event_screen(&view, 100, 25, now);
            assert!(list.contains(filename), "{list}");
            assert!(view.viewport.borrow().event_links.is_empty());
            assert_eq!(click_text(&mut view, &list, filename), None);
            view.select_fixture_event(Some(view.event_panel[7].clone()));
            event_screen(&view, 100, 25, now);
            assert!(view.viewport.borrow().event_links.is_empty());
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

fn temp_event_path(filename: &str) -> (tempfile::TempDir, PathBuf) {
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
fn task_event_click_on_a_shortened_tool_path_opens_the_full_path() {
    let (mut view, now) = event_fixture();
    let (_dir, target) = temp_event_path("report.md");
    let path = target.to_str().unwrap();
    let event = &mut view.event_panel[7];
    event
        .paths
        .push(event.body.len() + 1..event.body.len() + 1 + path.len());
    event.body = format!("{} {path}", event.body);
    let list = event_list_screen(&view, 100, 25, now);
    assert!(!list.contains(path), "{list}");
    assert_eq!(
        click_text(&mut view, &list, "report.md"),
        Some(Action::OpenPath(path.into()))
    );
    assert_eq!(view.visible_event(), view.event_panel.get(7));
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
    assert!(view.event_list.cursor.is_some());
}

#[test]
fn task_event_click_on_a_path_in_free_tool_text_opens_the_full_path() {
    let (mut view, now) = event_fixture();
    let (_dir, target) = temp_event_path("plan.md");
    let path = target.to_str().unwrap();
    view.event_panel[7].body = format!("start Implement workstream A of {path} (Claude provider)");
    let list = event_list_screen(&view, 100, 25, now);
    assert!(!list.contains(path), "{list}");
    assert_eq!(
        click_text(&mut view, &list, "plan.md"),
        Some(Action::OpenPath(path.into()))
    );
}

#[test]
fn task_event_click_on_an_image_placeholder_opens_the_image() {
    let (mut view, now) = event_fixture();
    let (_dir, image) = temp_event_path("2.png");
    view.event_panel[0].body = "Please look at [Image #1] and [Image #2]".into();
    view.event_panel[0].images = vec![crate::event::Image {
        label: "[Image #2]".into(),
        path: image.clone(),
    }];
    let list = event_screen(&view, 100, 40, now);
    assert!(list.contains("[Image #1] and [Image #2]"), "{list}");
    assert_eq!(click_text(&mut view, &list, "[Image #1]"), None);
    assert_eq!(
        click_text(&mut view, &list, "[Image #2]"),
        Some(Action::OpenPath(image))
    );
}

#[test]
fn open_event_links_its_wrapped_path() {
    let (mut view, now) = event_fixture();
    let (_dir, target) = temp_event_path("report.md");
    let path = target.to_str().unwrap();
    view.event_panel[6].body = format!(
        "A long response with **bold** words whose body must be cut before the right edge of the log, see {path} for details"
    );
    let list = event_screen(&view, 100, 25, now);
    click_event_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = event_screen(&view, 100, 25, now);
    assert!(open.contains("another-long-directory"), "{open}");
    assert_eq!(
        click_text(&mut view, &open, "another-long-directory"),
        Some(Action::OpenPath(path.into()))
    );
    assert_eq!(click_text(&mut view, &open, "see"), None);
}

#[test]
fn task_details_prompt_links_its_shortened_path_and_image_placeholder() {
    let mut view = fixture();
    let (_dir, target) = temp_event_path("report.md");
    let path = target.to_str().unwrap();
    let image = target.with_file_name("1.png");
    std::fs::write(&image, "").unwrap();
    let selected = &mut view.rows[view.selected];
    selected.prompt = format!("Look at [Image #1] and the report in {path} please");
    selected.prompt_images = vec![crate::event::Image {
        label: "[Image #1]".into(),
        path: image.clone(),
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
        Some(Action::OpenPath(image))
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
fn task_details_relative_paths_link_only_when_they_exist() {
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
fn task_details_project_lines_link_the_pull_request_and_the_directory() {
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
        .task_details_links
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
        .task_details_links
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
        .task_details_links
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
        Some(Action::OpenPath(crate::test_project()))
    );
    // Below 100 columns task details move below the task list.
    screen(&view, 99, 40);
    assert_eq!(click_mouse(&mut view, cwd_cell), None);
}

#[test]
fn project_group_header_of_a_missing_directory_is_not_a_link() {
    let mut task = session("gone", "Gone task", SessionStatus::Ready, 0);
    task.cwd = "/nonexistent/build".into();
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![task], false);
    let rendered = screen(&view, 120, 40);
    assert!(rendered.contains(" build "), "{rendered}");
    assert!(view.viewport.borrow().list_links.is_empty());
}

#[test]
fn project_group_header_opens_its_directory() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let (x, y) = rendered_cell(&rendered, " project  ");
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y)
        ),
        None
    );
    assert_eq!(
        view.flush_pending_click(Instant::now() + view.double_click_interval),
        Some(Action::OpenPath(crate::test_project()))
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
fn task_event_selected_event_closes_when_selection_changes() {
    let (mut view, now) = event_fixture();
    let list = event_screen(&view, 100, 25, now);
    click_event_text(&mut view, &list, "A long response", MouseButton::Left);
    assert!(view.event_list.cursor.is_some());
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
    assert!(view.event_list.cursor.is_none());
}

fn double_click_text(view: &mut View, screen: &str, text: &str) -> Option<Action> {
    let (x, y) = rendered_cell(screen, text);
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(click_mouse(view, click).is_none());
    click_mouse(view, click)
}

#[test]
fn double_click_on_open_event_body_copies_the_whole_event() {
    let (mut view, now) = event_fixture();
    let body = "A long response with **bold** words whose body must be cut before the right edge of the log\n\nA second paragraph";
    view.event_panel[6].body = body.into();
    let list = event_screen(&view, 100, 25, now);
    click_event_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = event_screen(&view, 100, 25, now);
    assert_eq!(
        double_click_text(&mut view, &open, "A second paragraph"),
        Some(Action::Copy {
            text: body.into(),
            label: "Copied message".into()
        })
    );
    assert_eq!(
        double_click_text(&mut view, &open, "Assistant"),
        Some(Action::Copy {
            text: body.into(),
            label: "Copied message".into()
        })
    );
}

#[test]
fn event_detail_left_click_has_no_deadline() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 100, 45, now);
    let area = view.viewport.borrow().event_detail;
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
fn double_click_event_detail_copies_the_message() {
    let (mut view, now) = event_fixture();
    view.event_panel[6].body = "A long response with **bold** words whose body must be cut before the right edge of the log\n\nA second paragraph".into();
    let list = event_screen(&view, 100, 25, now);
    click_event_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = event_screen(&view, 100, 25, now);
    assert_eq!(
        double_click_text(&mut view, &open, "A second paragraph"),
        Some(Action::Copy {
            text: view.event_panel[6].body.clone(),
            label: "Copied message".into(),
        }),
    );
}

#[test]
fn double_click_on_task_details_prompt_or_last_message_copies_it() {
    let mut view = overflowing_task_details();
    let message = "First reply row\n\nSecond reply row";
    view.rows[view.selected].last_message = Some(message.into());
    let prompt = view.rows[view.selected].prompt.clone();
    let now = view.rows[view.selected].updated_at;
    let screen = event_screen(&view, 120, 40, now);
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
    let mut view = overflowing_task_details();
    let now = view.rows[view.selected].updated_at;
    let screen = event_screen(&view, 120, 40, now);
    for text in ["A long prompt", "First reply row"] {
        let (x, y) = rendered_cell(&screen, text);
        let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert!(click_mouse(&mut view, click).is_none());
    }
}

#[test]
fn copy_confirmation_sits_at_footer_right_for_one_second() {
    let (mut view, now) = event_fixture();
    let start = Instant::now();
    let hints = event_screen(&view, 100, 25, now)
        .lines()
        .last()
        .unwrap()
        .to_owned();
    view.show_copied("Copied prompt".into(), start);
    let footer = event_screen(&view, 100, 25, now)
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
        event_screen(&view, 100, 25, now).lines().last().unwrap(),
        hints
    );
}

#[test]
fn error_sits_at_footer_right_in_the_error_color_for_five_seconds() {
    let (mut view, now) = event_fixture();
    // Keep logical time behind wall time so mixing the clocks fails deterministically.
    let start = Instant::now() - std::time::Duration::from_secs(10);
    view.show_feedback_at(
        footer::Feedback::Error("Resume is not supported".into()),
        start,
    );
    let area = Rect::new(0, 0, 100, 25);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, now);
    let footer = (0..area.width)
        .map(|x| buffer[(x, area.bottom() - 1)].symbol().to_owned())
        .collect::<String>();
    assert!(
        footer.trim_end().ends_with("Resume is not supported"),
        "{footer}"
    );
    let x = footer.trim_end().width() as u16 - 1;
    assert_eq!(
        buffer[(x, area.bottom() - 1)].fg,
        style::error_style().fg.unwrap()
    );
    assert_eq!(
        view.expire_messages(start + std::time::Duration::from_secs(1)),
        Some(std::time::Duration::from_secs(4))
    );
    assert_eq!(
        view.expire_messages(start + std::time::Duration::from_secs(5)),
        None
    );
    assert!(!event_screen(&view, 100, 25, now).contains("Resume is not supported"));
}

#[test]
fn footer_feedback_grows_over_the_hints_and_stays_on_one_row() {
    let (mut view, now) = event_fixture();
    let hints = event_screen(&view, 100, 25, now)
        .lines()
        .last()
        .unwrap()
        .trim_end()
        .to_owned();
    let long = "x".repeat(95);
    view.show_error(long.clone());
    let rendered = event_screen(&view, 100, 25, now);
    let footer = rendered.lines().last().unwrap();
    assert!(footer.trim_end().ends_with(&long), "{footer}");
    assert!(!footer.contains(&hints), "{footer}");
    view.show_error("y".repeat(200));
    let rendered = event_screen(&view, 100, 25, now);
    let footer = rendered.lines().last().unwrap();
    assert!(
        footer.trim().starts_with('y') && footer.trim_end().ends_with('…'),
        "{footer}"
    );
    assert_eq!(rendered.lines().count(), 25);
}

#[test]
fn selected_prompt_failure_shows_at_footer_right() {
    let (mut view, now) = event_fixture();
    view.input_box.state = prompt_box::SendState::Failed("attach timed out".into());
    let rendered = event_screen(&view, 100, 25, now);
    assert!(
        rendered
            .lines()
            .last()
            .unwrap()
            .trim_end()
            .ends_with("attach timed out"),
        "{rendered}"
    );
    view.input_box.state = prompt_box::SendState::Ready;
    assert!(!event_screen(&view, 100, 25, now).contains("attach timed out"));
}

#[test]
fn review_refresh_cancels_open_selection() {
    let (mut view, now) = open_tool_fixture();
    let open = event_screen(&view, 100, 48, now);
    let (x, y) = rendered_cell(&open, "cargo test");
    view.begin_text_selection(x, y);
    view.extend_text_selection(x + 4, y);
    let mut events = view.event_panel.clone();
    events[7].calls[1].output = Some(vec![]);
    view.set_event(events);
    assert!(!view.text_selection_dragging());
    assert!(view.finish_text_selection(x + 4, y).is_none());
}

#[test]
fn review_running_call_refreshes_to_error_or_denied() {
    for kind in [crate::event::Kind::Error, crate::event::Kind::Denied] {
        let (mut view, now) = open_tool_fixture();
        event_screen(&view, 100, 48, now);
        let mut events = view.event_panel.clone();
        events[7].kind = kind.clone();
        events[7].calls[1].output = Some(vec![]);
        view.set_event(events);
        assert_eq!(view.visible_event().unwrap().kind, kind);
    }
}

#[test]
fn review_cached_layout_shares_source_bytes() {
    let (view, now) = open_tool_fixture();
    event_screen(&view, 100, 48, now);
    let sources = view.viewport.borrow().event_text.sources.clone();
    event_screen(&view, 100, 48, now);
    let viewport = view.viewport.borrow();
    for (cached, drawn) in sources.iter().zip(&viewport.event_text.sources) {
        assert_eq!(cached.text.as_ptr(), drawn.text.as_ptr());
        assert_eq!(drawn.text.as_ptr(), drawn.clone().text.as_ptr());
    }
}

#[test]
fn open_tool_layout_titles_and_arrows() {
    let (mut view, now) = open_tool_fixture();
    view.event_panel[7].calls.truncate(1);
    view.event_panel[7].calls[0].title = "Run tests".into();
    view.event_panel[7].calls[0].event_detail_title = "Run tests".into();
    view.select_fixture_event(Some(view.event_panel[7].clone()));
    let open = event_screen(&view, 100, 48, now);
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
    let area = view.viewport.borrow().event_text.area;
    assert_eq!(
        x,
        super::centering::centered_x(view.viewport.borrow().event_detail, area, 1)
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
        crate::event::Kind::Tool,
        crate::event::Kind::Error,
        crate::event::Kind::Denied,
        crate::event::Kind::Assistant,
        crate::event::Kind::User,
        crate::event::Kind::Agent,
        crate::event::Kind::Session,
        crate::event::Kind::QueuedUser,
        crate::event::Kind::QueuedSystem,
    ] {
        let (mut view, now) = event_fixture();
        let mut event = view.event_panel[7].clone();
        event.kind = kind;
        event.body = "{\"answer\":[1,2]}".into();
        event.calls.clear();
        if event.kind == crate::event::Kind::Tool {
            assert!(super::event_list::open_text(&event).is_empty());
            continue;
        }
        view.select_fixture_event(Some(event));
        let open = event_screen(&view, 100, 48, now);
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
    let (mut view, now) = event_fixture();
    let mut event = view.event_panel[1].clone();
    event.body = (0..80)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    view.select_fixture_event(Some(event));
    event_screen(&view, 100, 25, now);
    let area = view.viewport.borrow().event_text.area;
    let layout = view.center_layout(Rect::new(0, 0, 100, 25));
    assert_eq!(area.right() + 12, layout.event_detail.right());
    let scrollbar_x = layout.event_detail.right() - 1;
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(buf.area, &mut buf, now);
    assert_eq!(buf[(scrollbar_x, area.top())].symbol(), "▕");
    assert_eq!(buf[(scrollbar_x, area.top())].fg, Color::Reset);
    assert_eq!(buf[(scrollbar_x, area.bottom() - 1)].symbol(), " ");
    assert_eq!(buf[(scrollbar_x, layout.event_detail.top())].symbol(), "─");
    assert_eq!(
        buf[(scrollbar_x, layout.event_detail.bottom())].symbol(),
        "─"
    );
    for y in area.top()..area.bottom() {
        assert_ne!(buf[(area.right() - 1, y)].symbol(), "▕");
    }
    view.event_detail.scroll.borrow_mut().offset = usize::MAX;
    event_screen(&view, 100, 25, now);
    view.render_at(buf.area, &mut buf, now);
    assert_eq!(buf[(scrollbar_x, area.bottom() - 1)].symbol(), "▕");
}

#[test]
fn real_tool_headers_show_description_agent_skill_and_recipient() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let calls =
        crate::providers::claude::event_reader::tail(&root.join("claude-tool-calls.jsonl"), 100)
            .unwrap();
    let messages = crate::providers::claude::event_reader::tail_session(
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
        let title = call.event_detail_title.clone();
        let left = call.event_detail_left.clone();
        for kind in [
            crate::event::Kind::Tool,
            crate::event::Kind::Error,
            crate::event::Kind::Denied,
        ] {
            event.kind = kind;
            let (mut view, now) = event_fixture();
            view.select_fixture_event(Some(event.clone()));
            let open = event_screen(&view, 120, 48, now);
            let header = open
                .lines()
                .nth(usize::from(view.viewport.borrow().event_detail.y + 1))
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
fn real_tabs_copy_as_displayed_spaces_and_allow_partial_selection() {
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
    let mut event = view.event_panel[7].clone();
    event.calls.truncate(1);
    event.calls[0].input = vec![crate::event::Section::Code {
        label: "command".into(),
        lang: "text".into(),
        text: source.into(),
    }];
    event.calls[0].output = None;
    view.select_fixture_event(Some(event.clone()));
    let open = event_screen(&view, 100, 48, now);
    assert!(super::event_list::open_text(&event).contains(source));
    let (_, arrow_y) = rendered_cell(&open, "↓");
    let (_, running_y) = rendered_cell(&open, super::tool_detail::running_frame(now));
    assert_eq!(running_y, arrow_y + 1);
    let (x, y) = rendered_cell(&open, "45    # terminal");
    view.begin_text_selection(x, y);
    view.extend_text_selection(x + source.replace('\t', "    ").len() as u16, y);
    event_screen(&view, 100, 48, now);
    assert_eq!(
        view.finish_text_selection(x + source.replace('\t', "    ").len() as u16, y),
        Some(Action::Copy {
            text: source.replace('\t', "    "),
            label: "Copied message".into()
        })
    );
    view.begin_text_selection(x + 2, y);
    view.extend_text_selection(x + 4, y);
    assert_eq!(
        view.finish_text_selection(x + 4, y),
        Some(Action::Copy {
            text: "  ".into(),
            label: "Copied message".into(),
        })
    );
    // Each space is selectable even though all four map to the same source byte.
    view.begin_text_selection(x + 2, y);
    view.extend_text_selection(x + 6, y);
    assert_eq!(
        view.finish_text_selection(x + 6, y),
        Some(Action::Copy {
            text: "    ".into(),
            label: "Copied message".into()
        })
    );
}

#[test]
fn small_scrolled_tool_label_drag_stays_inside_the_buffer() {
    let (mut view, now) = open_tool_fixture();
    let mut event = view.event_panel[7].clone();
    event.calls[0].input = vec![crate::event::Section::Diff {
        lang: "rs".into(),
        label: "/workspace/very/long/path/to/source.rs".repeat(6),
        text: "+中文".into(),
    }];
    view.select_fixture_event(Some(event));
    let open = event_screen(&view, 30, 60, now);
    let (x, y) = rendered_cell(&open, "/workspace");
    view.begin_text_selection(x, y);
    view.extend_text_selection(29, y + 4);
    event_screen(&view, 30, 60, now);
    let viewport = view.viewport.borrow();
    for row in &viewport.event_text.rows {
        assert!(
            row.cells
                .iter()
                .all(|(range, _)| range.end <= viewport.event_text.area.width)
        );
    }
}

#[test]
fn open_tool_without_calls_keeps_its_row_title() {
    let (mut view, now) = event_fixture();
    let event = view.event_panel[7].clone();
    assert!(event.calls.is_empty());
    view.select_fixture_event(Some(event));
    let open = event_screen(&view, 100, 25, now);
    assert!(
        open.lines()
            .any(|line| line.trim_start().starts_with("run tests") && line.ends_with("35s")),
        "{open}"
    );
}

#[test]
fn tool_outputs_and_markdown_use_shared_json_rendering() {
    for output in [
        crate::event::Section::Text {
            label: "stdout".into(),
            text: "{\"answer\":[1,2]}".into(),
        },
        crate::event::Section::Markdown {
            label: "output".into(),
            text: "{\"answer\":[1,2]}".into(),
        },
        crate::event::Section::Code {
            label: "output".into(),
            lang: "text".into(),
            text: "{\"answer\":[1,2]}".into(),
        },
    ] {
        let (mut view, now) = open_tool_fixture();
        let mut event = view.event_panel[7].clone();
        event.calls.truncate(1);
        event.calls[0].output = Some(vec![output]);
        view.select_fixture_event(Some(event.clone()));
        let open = event_screen(&view, 100, 48, now);
        if matches!(
            event.calls[0].output.as_ref().unwrap()[0],
            crate::event::Section::Code { .. }
        ) {
            assert!(open.contains("{\"answer\":[1,2]}"));
            assert!(!super::event_list::open_text(&event).contains("\n  \"answer\": ["));
        } else {
            assert!(open.contains("\"answer\": ["));
            assert!(super::event_list::open_text(&event).contains("\n  \"answer\": [\n    1,"));
        }
    }
    let (mut view, now) = open_tool_fixture();
    let mut event = view.event_panel[7].clone();
    event.calls.truncate(1);
    event.calls[0].output = Some(vec![]);
    view.select_fixture_event(Some(event));
    let open = event_screen(&view, 100, 48, now);
    assert!(open.contains("(no output)"));
    assert!(open.lines().any(|line| line.trim() == "↓"));
    let area = view.viewport.borrow().event_text.area;
    assert_eq!(area.width, 87);
    let lines: Vec<&str> = open.lines().collect();
    assert!(
        (area.top()..area.bottom()).all(|y| {
            lines[usize::from(y)].chars().nth(usize::from(area.right())) != Some('│')
        })
    );
}

#[test]
fn pretty_json_body_remaps_provider_path_ranges() {
    let (mut view, now) = event_fixture();
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let path = dir.path().join("report.md");
    std::fs::write(&path, "report").unwrap();
    let path_text = path.to_str().unwrap();
    let mut event = view.event_panel[7].clone();
    event.kind = crate::event::Kind::QueuedSystem;
    event.body = serde_json::json!({"note": "中文", "path": path_text}).to_string();
    let start = event.body.find(path_text).unwrap();
    event.paths.clear();
    event.paths.push(start..start + path_text.len());
    view.select_fixture_event(Some(event));
    event_screen(&view, 100, 48, now);
    assert!(
        view.viewport
            .borrow()
            .event_links
            .iter()
            .any(|(_, target)| target == &path)
    );
}

#[test]
fn commit_sha_links_open_vscode_from_event_and_task_details() {
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
    let (mut view, now) = event_fixture();
    view.rows[view.selected].cwd = dir.path().into();
    view.rows[view.selected].prompt = format!("已提交 `{sha}`，推送了");
    view.event_panel[7].body = format!("commits {sha} deadbeef {}", "more text ".repeat(30));
    view.event_panel[7].paths.clear();
    for kind in [crate::event::Kind::Assistant, crate::event::Kind::Tool] {
        view.event_panel[7].kind = kind;
        view.set_event(view.event_panel.clone());
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let rendered = loop {
            let rendered = event_screen(&view, 120, 40, now);
            if view
                .viewport
                .borrow()
                .event_links
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
        assert_eq!(view.visible_event(), view.event_panel.get(7));
        assert_eq!(click_text(&mut view, &rendered, "deadbeef"), None);
        assert!(view.event_list.cursor.is_some());
        let rendered = event_screen(&view, 120, 40, now);
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
    view.event_panel[7].body = format!("commit {full}");
    view.select_fixture_event(Some(view.event_panel[7].clone()));
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let rendered = event_screen(&view, 120, 40, now);
        let link = view
            .viewport
            .borrow()
            .event_links
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
        let mut event = view.event_panel[7].clone();
        if calls {
            event.calls.truncate(1);
            event.calls[0].input.clear();
            event.calls[0].output = Some(vec![crate::event::Section::Text {
                label: "stdout".into(),
                text: output.into(),
            }]);
        } else {
            event.calls.clear();
            event.kind = crate::event::Kind::Error;
            event.body = output.into();
        }
        view.select_fixture_event(Some(event.clone()));
        let screen = event_screen(&view, 100, 64, now);
        assert!(screen.contains("Result"));
        assert!(screen.contains("\"answer\": ["));
        assert!(screen.contains("finished"));
        assert!(
            screen
                .lines()
                .skip(usize::from(view.viewport.borrow().event_detail.y))
                .any(|line| line.contains("**finished**"))
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 64));
        view.render_at(buf.area, &mut buf, now);
        let event_detail_y = view.viewport.borrow().event_detail.y;
        let event_detail_screen = screen
            .lines()
            .skip(usize::from(event_detail_y))
            .collect::<Vec<_>>()
            .join("\n");
        let (x, y) = rendered_cell(&event_detail_screen, "\"answer\"");
        let y = y + event_detail_y;
        assert!(matches!(
            buf[(x, y)].fg,
            Color::Rgb(..) | Color::Indexed(..)
        ));
        let viewport = view.viewport.borrow();
        let json = viewport
            .event_text
            .sources
            .iter()
            .find(|source| source.text.starts_with('{'))
            .unwrap();
        assert_eq!(
            json.text.as_ref(),
            "{\n  \"answer\": [\n    1,\n    2\n  ]\n}"
        );
        for row in viewport
            .event_text
            .rows
            .iter()
            .filter(|row| row.segment == json.id)
        {
            for (cells, bytes) in &row.cells {
                let drawn: String = (cells.start..cells.end)
                    .map(|offset| cell_text(buf[(row.x + offset, row.y)].symbol()))
                    .collect();
                assert_eq!(drawn, &json.text[bytes.clone()]);
            }
        }
        drop(viewport);
        let (start_x, start_y) = rendered_cell(&event_detail_screen, "Result");
        let start_y = start_y + event_detail_y;
        let (end_x, end_y) = rendered_cell(&event_detail_screen, "finished");
        let end_y = end_y + event_detail_y;
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
        event_screen(&view, 100, 64, now);
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
fn event_new_button_keeps_right_alignment_and_target() {
    for width in [60, 100, 149, 150, 200] {
        for unseen in [1, 3] {
            let (mut view, now) = event_fixture();
            let mut event = view.event_panel[6].clone();
            event.body = (0..100).map(|n| format!("row {n}\n\n")).collect();
            event.calls.clear();
            view.event_panel[6] = event;
            view.select_event(Some(6));
            event_screen(&view, width, 40, now);
            view.event_detail.scroll.borrow_mut().offset = 1;
            view.event_list.unseen.set(unseen);
            let screen = event_screen(&view, width, 40, now);
            let layout = view.center_layout(Rect::new(0, 0, width, 40));
            let viewport = view.viewport.borrow();
            let latest = viewport.event_jump;
            let border_y = layout.event_panel.bottom();
            assert_eq!(latest.right() - 1, layout.event_panel.right() - 2);
            assert_eq!(latest.y, border_y);
            let label = format!(" ↓ {unseen} New");
            assert_eq!(
                rendered_cell(&screen, label.trim_end()),
                (latest.x, latest.y)
            );
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
            assert!(view.following_event());
            assert_eq!(view.event_list.unseen.get(), 0);
        }
    }
}

#[test]
fn wrapped_senders_stay_in_list_and_move_to_open_header() {
    let file = crate::providers::claude::event_reader::tests::wrapped_fixture();
    let events = crate::providers::claude::event_reader::tail(file.path(), 20).unwrap();
    let (mut view, now) = event_fixture();
    for (index, sender, body) in [
        (2, "a113c3cb9c1482c7a", "Report:"),
        (3, "a113c3cb9c1482c7a", "Report:"),
        (4, "translate-pdf", "translate-pdf pilots ready for review."),
        (5, "translate-pdf", "translate-pdf pilots ready for review."),
    ] {
        let mut event = events[index].clone();
        event.started_at = now - Duration::seconds(120);
        assert_eq!(
            super::event_list::body_line(&event, 200, &view.rows[0].cwd).to_string(),
            format!("{sender} → message")
        );
        assert!(event.body.starts_with(body));
        let clock = event.calls[0]
            .started_at
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%H:%M:%S")
            .to_string();
        let mut later = event.clone();
        later.started_at = now;
        view.set_event(vec![event.clone(), later]);
        view.select_fixture_event(Some(event));
        let rendered = event_screen(&view, 160, 25, now);
        let header = rendered
            .lines()
            .nth(usize::from(view.viewport.borrow().event_detail.y + 1))
            .unwrap();
        assert!(
            header.trim_start().starts_with("Message"),
            "sender: {header}"
        );
        assert!(header.contains(sender), "kind: {header}");
        assert!(header.ends_with(&clock), "time: {header}");
        assert!(
            !super::event_list::open_text(view.visible_event().unwrap())
                .contains(&format!("{sender} → "))
        );
    }
}

#[test]
fn wrapped_messages_keep_list_text_and_open_paragraphs() {
    let file = crate::providers::claude::event_reader::tests::wrapped_fixture();
    let events = crate::providers::claude::event_reader::tail(file.path(), 20).unwrap();
    let (mut view, now) = event_fixture();
    for event in &events[2..] {
        for width in [25, 60, 200, 2000] {
            let row = super::event_list::body_line(event, width, &view.rows[0].cwd);
            assert!(!row.to_string().contains('\n'));
            assert!(!row.to_string().trim().is_empty());
        }
    }
    for event in &events[6..12] {
        view.event_panel = vec![event.clone()];
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, 2000, 25);
        let mut buffer = Buffer::empty(area);
        view.render_event_detail(area, &mut buffer, now);
        let lines: Vec<String> = buffer
            .content
            .chunks(2000)
            .map(|row| {
                row.iter()
                    .map(|cell| cell_text(cell.symbol()))
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
    let (mut view, now) = event_fixture();
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
                    .map(|x| cell_text(buf[(x, y)].symbol()).to_owned())
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
fn usage_chart_uses_assistant_color_for_selected_usage() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    use crate::usage::{Bin, bin_of};

    let (mut view, now) = event_fixture();
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
    let area = Rect::new(0, 0, 100, 25);

    for use_colors in [true, false] {
        view.use_colors = use_colors;
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        let x = layout.list.right() - 1;
        let chart = &buf[(x, layout.list.bottom() - 1)];
        assert!(
            cell_text(chart.symbol())
                .chars()
                .all(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
        );
        assert_eq!(
            chart.fg,
            if use_colors {
                Color::Yellow
            } else {
                Color::Reset
            },
            "use_colors={use_colors}"
        );
    }
}

#[test]
fn usage_chart_stacks_cells_at_rounded_boundaries_and_preserves_both_segments() {
    use crate::usage::Bin;
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (mut view, now) = event_fixture();
    view.cell_aspect = 1.0;
    let area = Rect::new(0, 0, 8, 4);
    let [left, right] = super::timeline::cell_bins(area, 7, now);
    for (tokens, others, blue_rows) in [(16, 10, 3), (16, 9, 2), (1600, 1599, 3), (1600, 1, 1)] {
        view.set_usage_totals(
            [(left, tokens / 2), (right, tokens / 2)]
                .map(|(bin, input)| (bin, Bin { input, output: 0 }))
                .into(),
            [
                (left, others.min(tokens / 2)),
                (right, others.saturating_sub(tokens / 2)),
            ]
            .map(|(bin, input)| (bin, Bin { input, output: 0 }))
            .into(),
        );
        for use_colors in [true, false] {
            view.use_colors = use_colors;
            let mut buf = Buffer::empty(area);
            view.render_usage_chart(area, &mut buf, now);
            for y in 0..4 {
                assert_eq!(
                    buf[(7, y)].symbol(),
                    "⣿",
                    "both columns fill the same height"
                );
                assert_eq!(
                    buf[(7, y)].fg,
                    if !use_colors {
                        Color::Reset
                    } else if y >= 4 - blue_rows {
                        crate::theme::inline_code_color()
                    } else {
                        Color::Yellow
                    },
                    "tokens={tokens}, others={others}, y={y}, use_colors={use_colors}"
                );
            }
        }
    }
}

#[test]
fn usage_chart_combines_bin_pairs_and_keeps_its_outline_when_selection_changes() {
    use crate::usage::Bin;
    let (mut view, now) = event_fixture();
    view.cell_aspect = 1.0;
    let area = Rect::new(0, 0, 8, 4);
    let [left, right] = super::timeline::cell_bins(area, 7, now);
    let totals = [(left, 8), (right, 4), (left - 1, 1), (left - 2, 3)]
        .map(|(bin, input)| (bin, Bin { input, output: 0 }));
    let mut outlines = Vec::new();
    for others in [0_u64, 1, 4, 12] {
        view.set_usage_totals(
            totals.into(),
            [(left, others.min(8)), (right, others.saturating_sub(8))]
                .map(|(bin, input)| (bin, Bin { input, output: 0 }))
                .into(),
        );
        let mut buf = Buffer::empty(area);
        view.render_usage_chart(area, &mut buf, now);
        assert_eq!(buf[(7, 0)].symbol(), "⣿");
        assert_eq!(buf[(6, 2)].symbol(), "⣤");
        assert_eq!(buf[(6, 3)].symbol(), "⣿");
        for cell in &buf.content {
            if let Some(ch) = cell
                .symbol()
                .chars()
                .next()
                .filter(|ch| ('\u{2800}'..='\u{28ff}').contains(ch))
            {
                let bits = ch as u32 - 0x2800;
                for (left_bit, right_bit) in
                    [(0x40, 0x80), (0x04, 0x20), (0x02, 0x10), (0x01, 0x08)]
                {
                    assert_eq!(
                        bits & left_bit != 0,
                        bits & right_bit != 0,
                        "{}",
                        cell.symbol()
                    );
                }
            }
        }
        outlines.push(
            buf.content
                .iter()
                .map(|cell| cell.symbol().to_owned())
                .collect::<Vec<_>>(),
        );
    }
    assert!(outlines.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn usage_chart_keeps_paired_heights_and_colors_when_time_advances_five_seconds() {
    use crate::usage::Bin;
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let (mut view, _) = event_fixture();
    view.cell_aspect = 1.0;
    let now = chrono::DateTime::from_timestamp(1000, 0).unwrap();
    let area = Rect::new(0, 0, 8, 4);
    let [left, right] = super::timeline::cell_bins(area, 7, now);
    view.set_usage_totals(
        [(left, 8), (right, 4), (left - 1, 1), (left - 2, 3)]
            .map(|(bin, input)| (bin, Bin { input, output: 0 }))
            .into(),
        [(left, 8), (left - 1, 1)]
            .map(|(bin, input)| (bin, Bin { input, output: 0 }))
            .into(),
    );
    let render = |at| {
        let mut buf = Buffer::empty(area);
        view.render_usage_chart(area, &mut buf, at);
        buf
    };
    let first = render(now);
    assert_eq!(first, render(now + Duration::seconds(5)));
    let moved = render(now + Duration::seconds(10));
    for y in 0..area.height {
        for x in 1..area.width {
            assert_eq!(first[(x, y)], moved[(x - 1, y)]);
        }
    }
}

#[test]
fn usage_chart_shows_total_usage_without_a_selected_task() {
    let (mut view, now) = event_fixture();
    view.set_sessions(Vec::new(), false);
    let bin = crate::usage::bin_of(now);
    let bins = [(
        bin,
        crate::usage::Bin {
            input: 1,
            output: 0,
        },
    )]
    .into();
    view.set_usage_totals(bins, Default::default());
    assert!(view.shows_usage());
    let area = Rect::new(0, 0, 8, 2);
    let mut buf = Buffer::empty(area);
    view.render_usage_chart(area, &mut buf, now);
    assert!(buf.content.iter().any(|cell| cell.symbol() != " "));
    view.set_usage_totals(
        [(bin, crate::usage::Bin::default())].into(),
        Default::default(),
    );
    assert!(!view.shows_usage());
}

#[test]
fn task_rule_has_no_activity_dots_without_usage() {
    let (view, now) = event_fixture();
    let area = Rect::new(0, 0, 100, 25);
    let layout = view.center_layout(area);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    for x in layout.list.x..layout.list.right() {
        assert!(
            !buf[(x, layout.prompt_top_rule.y)]
                .symbol()
                .chars()
                .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
        );
    }
    assert!(view.timeline_redraw_in(now).is_none());
}

#[path = "click_event_tests.rs"]
mod click_event_tests;

#[path = "prompt_view_tests.rs"]
mod prompt_view_tests;

#[path = "task_details_group_tests.rs"]
mod task_details_group_tests;

#[path = "task_details_order_tests.rs"]
mod task_details_order;

#[test]
fn task_details_activity_click_selects_source() {
    task_details_click_selects_source(7, "Copied last activity");
}

#[test]
fn task_details_update_click_selects_source() {
    task_details_click_selects_source(6, "Copied update");
}

#[test]
fn task_details_prompt_click_selects_source() {
    task_details_click_selects_source(0, "Copied prompt");
}

fn task_details_click_selects_source(index: usize, label: &'static str) {
    let (mut view, now) = event_fixture();
    for (index, event) in view.event_panel.iter_mut().enumerate() {
        event.source = Some(format!("fixture:{index}"));
    }
    let activity = view.event_panel[7].clone();
    let update = view.event_panel[6].clone();
    let prompt = view.event_panel[0].clone();
    let row = &mut view.rows[view.selected];
    row.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: activity.kind.clone(),
        label: activity.body.clone(),
        at: activity.started_at,
    });
    row.last_message = Some(update.body.clone());
    row.last_message_at = Some(update.started_at);
    row.prompt = prompt.body.clone();
    row.prompt_at = Some(prompt.started_at);
    row.task_details_sources = [
        activity.source.clone(),
        update.source.clone(),
        prompt.source.clone(),
    ];
    let expected = view.event_panel[index].clone();
    view.select_fixture_event(Some(view.event_panel[1].clone()));
    let rendered = event_screen(&view, 160, 45, now);
    let rows = view.viewport.borrow().task_details_copy.clone();
    let y = rows
        .iter()
        .find(|(_, _, value)| *value == label)
        .unwrap()
        .0
        .start;
    let x = view.viewport.borrow().task_details.x;
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(click_mouse(&mut view, click), None);
    assert_eq!(view.visible_event(), Some(&expected), "{label}\n{rendered}");
    assert!(view.pending_click.is_none());
    event_screen(&view, 160, 45, now);
    assert!(
        view.viewport
            .borrow()
            .event_rows
            .contains(&EventRow::Timeline(index))
    );
    assert_eq!(view.visible_event() == view.event_panel.last(), index == 7);
    view.set_event(Vec::new());
    event_screen(&view, 160, 45, now);
    assert_eq!(click_mouse(&mut view, click), None);
    assert!(view.event_list.cursor.is_none());
}

#[test]
fn task_notification_detail_hides_task_id_and_shows_output_content() {
    let dir = tempfile::tempdir().unwrap();
    let events = super::event_list::event_markdown::notification_events(
        dir.path(),
        Some("all 3 tests passed"),
    );
    let (mut view, now) = event_fixture();
    for event in events {
        let summary = event.body.clone();
        view.event_panel = vec![event.clone(), event.clone()];
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, 160, 15);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 40));
        view.render_event_detail(area, &mut buffer, now);
        let lines: Vec<_> = buffer
            .content
            .chunks(160)
            .take(usize::from(area.height))
            .map(|row| {
                let mut text = String::new();
                let mut column = 0;
                while column < row.len() {
                    let symbol = cell_text(row[column].symbol());
                    text.push_str(symbol);
                    column += unicode_width::UnicodeWidthStr::width(symbol).max(1);
                }
                text
            })
            .collect();
        let detail =
            super::tool_detail::event_detail_content(&event, String::new(), Style::new().dim());
        assert_eq!(detail.sections[0].title.left.as_deref(), None);
        let header = 1;
        assert!(
            !lines[header + 1..]
                .iter()
                .any(|line| line.contains(&summary))
        );
        assert!(!lines[header].contains("Task b9wi31ww0"));
        assert_eq!(
            lines
                .iter()
                .map(|line| line.trim_end())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
            include_str!("../../tests/ui_snapshots/task_notification_event_detail.txt")
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

#[path = "task_details_tasks_tests.rs"]
mod task_details_tasks;

#[test]
fn large_separators_join_the_rules_above_and_below() {
    let (view, now) = column_padding_fixture();
    for width in [150, 200, 240] {
        let area = Rect::new(0, 0, width, 60);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let layout = view.center_layout(area);
        for (x, top, bottom) in [
            (layout.gap.x + 1, layout.gap.y, layout.gap.bottom()),
            (
                layout.event_panel.x - 2,
                layout.event_panel.y,
                layout.event_panel.bottom(),
            ),
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
        let y = layout.task_details.bottom();
        let joint = layout.gap.x + 1;
        assert_eq!(buf[(joint, y)].symbol(), "┴");
        let cell = &buf[(joint + 1, y)];
        assert_eq!(cell.symbol(), "╴", "width {width}");
        assert_eq!(cell.style(), buf[(joint, y)].style());
        assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
        if crate::ui::render::ScreenLayout::for_width(width)
            == crate::ui::render::ScreenLayout::Large
        {
            let joint = layout.event_panel.x - 2;
            for (x, y, symbol) in [
                (joint - 1, layout.event_panel.y - 1, "╶"),
                (joint - 1, layout.event_panel.bottom(), "╶"),
                (joint + 1, layout.event_panel.bottom(), "─"),
            ] {
                let cell = &buf[(x, y)];
                assert_eq!(
                    cell_text(cell.symbol()),
                    symbol,
                    "width {width}, x {x}, y {y}"
                );
                assert_eq!(cell.style(), buf[(joint, y)].style());
                assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
            }
        }
        let bottom: String = (0..width)
            .map(|x| cell_text(buf[(x, y)].symbol()))
            .collect();
        assert!(!bottom.contains(" Working "), "{bottom}");
        let top: String = (0..width)
            .map(|x| cell_text(buf[(x, layout.header.y + 1)].symbol()))
            .collect();
        assert!(top.contains(" Working "), "{top}");
        assert!(bottom.contains(" Task log example "), "{bottom}");
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
        let row: String = (0..width)
            .map(|x| cell_text(buf[(x, model.y)].symbol()))
            .collect();
        assert!(row.contains("opus ") && row.contains(" medium"), "{row}");
        for x in [model.x - 1, width - 2] {
            assert!(
                buf[(x, model.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::DIM)
            );
        }
        assert_eq!(buf[(model.x - 1, model.y)].symbol(), "╴", "{row}");
    }
}

mod event_navigation {
    use super::*;

    fn check(code: KeyCode, modifiers: KeyModifiers, forward: bool, edge: bool) {
        for search in [false, true] {
            for editing in [false, true] {
                if editing && !search {
                    continue;
                }
                let (mut view, _) = event_fixture();
                view.active_panel = Panel::EventList;
                if search {
                    view.prepare_event_search();
                    view.event_list.search.query.set_text("a");
                    view.rematch_event();
                    view.event_list.search.editing = editing;
                }
                screen(&view, 200, 45);
                let rect = view.viewport.borrow().event_panel;
                for kind in [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left),
                ] {
                    view.handle_mouse(mouse(kind, rect.x + 1, rect.y));
                }
                assert!(view.active_panel == Panel::EventList);
                view.viewport.borrow_mut().event_panel.height = 40;
                let events: Vec<_> = view.shown_events().map(|(_, e)| e.clone()).collect();
                assert!(events.len() >= 3);
                let current = events.len() / 2;
                view.select_fixture_event(Some(events[current].clone()));
                let key = KeyEvent::new(code, modifiers);
                let mut editor = super::super::line_edit::LineEdit::default();
                editor.set_text("a");
                let consumed = editing && editor.handle(key);
                if consumed {
                    view.handle_key(key);
                    assert_eq!(view.event_list.search.query.text(), editor.text());
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
                    view.visible_event(),
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
fn task_border_double_clicks_select_both_ends_in_both_layouts() {
    for width in [100, 149, 150, 200] {
        for bottom in [false, true] {
            for tab in [0, 1] {
                let mut view = simple_fixture();
                view.set_sessions(
                    (0..80)
                        .map(|i| {
                            session(
                                &format!("border-{i}"),
                                &format!("Task {i}"),
                                if i % 2 == 0 {
                                    SessionStatus::Ready
                                } else {
                                    SessionStatus::Inactive
                                },
                                i,
                            )
                        })
                        .collect(),
                    false,
                );
                if tab == 1 {
                    key(&mut view, KeyCode::Tab);
                }
                // Loading more is not a task row at the bottom of the inactive tab.
                view.has_more = true;
                let indices = view.visible_indices();
                view.select(indices[indices.len() / 2]);
                screen(&view, width, 30);
                let list = view.viewport.borrow().list;
                assert!(view.viewport.borrow().scroll > 0);
                assert!(view.viewport.borrow().scroll < view.viewport.borrow().task_scroll_max);
                let click = mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    list.x + list.width / 3,
                    if bottom { list.bottom() } else { list.y - 1 },
                );
                let before = view.selected;
                assert_eq!(click_mouse(&mut view, click), None);
                assert_eq!(view.selected, before, "single click, width {width}");
                assert_eq!(click_mouse(&mut view, click), None);
                let expected = if bottom {
                    *indices.last().unwrap()
                } else {
                    indices[0]
                };
                assert_eq!(
                    view.selected, expected,
                    "width {width}, bottom {bottom}, tab {tab}"
                );
                screen(&view, width, 30);
                let viewport = view.viewport.borrow();
                assert_eq!(
                    viewport.scroll,
                    if bottom { viewport.task_scroll_max } else { 0 }
                );
                assert_eq!(view.status_filter, tab);
                assert!(!view.prompt_focused());
            }
        }
    }
}

#[test]
fn event_border_double_clicks_respect_shared_output_border() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = event_fixture();
        event_screen(&view, width, 20, now);
        let event_panel = view.viewport.borrow().event_panel;
        let click = |view: &mut View, y| {
            click_mouse(
                view,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: event_panel.x + event_panel.width / 3,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
            )
        };
        let newest = view.shown_events().last().unwrap().1.clone();
        let oldest = view.shown_events().next().unwrap().1.clone();
        assert_eq!(click(&mut view, event_panel.y - 1), None);
        assert_eq!(view.visible_event(), Some(&newest));
        assert_eq!(click(&mut view, event_panel.y - 1), None);
        assert_eq!(view.visible_event(), Some(&oldest), "width {width}");
        event_screen(&view, width, 20, now);
        assert!(
            view.viewport
                .borrow()
                .event_rows
                .contains(&EventRow::Timeline(0))
        );
        assert_eq!(view.event_scroll(), 0);
        assert!(view.viewport.borrow().event_jump.is_empty());
        click(&mut view, event_panel.bottom());
        assert_eq!(view.visible_event(), Some(&oldest));
        click(&mut view, event_panel.bottom());
        if super::render::ScreenLayout::for_width(width) != super::render::ScreenLayout::Large {
            assert_eq!(view.visible_event(), Some(&oldest));
            assert_eq!(view.event_scroll(), 0);
            assert!(view.active_panel == Panel::EventDetail);
        } else {
            assert_eq!(view.visible_event(), Some(&newest));
            assert_eq!(view.event_scroll(), view.event_max_scroll());
        }
        event_screen(&view, width, 20, now);
        assert!(view.viewport.borrow().event_jump.is_empty());
        // New arrivals provide a single-click return path after jumping to the oldest.
        click(&mut view, event_panel.y - 1);
        click(&mut view, event_panel.y - 1);
        view.event_list.unseen.set(1);
        event_screen(&view, width, 20, now);
        let jump = view.viewport.borrow().event_jump;
        click_mouse(
            &mut view,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: jump.x,
                row: jump.y,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(view.visible_event(), Some(&newest));
    }
}

#[test]
fn event_border_fields_keep_click_actions_without_jumping() {
    for width in [100, 200] {
        for field in [Field::Model, Field::Effort] {
            let (mut view, now) = event_fixture();
            view.rows[view.selected].model = Some("opus".into());
            view.rows[view.selected].background_job = Some(crate::model::BackgroundJob {
                id: "e772d00b".into(),
                needs: None,
                awaits_prompt: false,
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
            let selected = view.event_panel[3].clone();
            view.select_fixture_event(Some(selected.clone()));
            event_screen(&view, width, 45, now);
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
            assert_eq!(view.visible_event(), Some(&selected));
            assert_eq!(view.selector.as_ref().unwrap().field(), field);
            let model = view.selected_row().unwrap().model.clone();
            let effort = view.selected_row().unwrap().effort.clone();
            event_screen(&view, width, 45, now);
            click_mouse(&mut view, mouse);
            assert_eq!(view.visible_event(), Some(&selected));
            assert!(view.selector.is_none());
            assert_eq!(view.selected_row().unwrap().model, model);
            assert_eq!(view.selected_row().unwrap().effort, effort);
        }
    }
}

#[test]
fn event_border_hides_missing_model_and_effort() {
    let (mut view, now) = event_fixture();
    let row = &mut view.rows[view.selected];
    row.background_job = Some(crate::model::BackgroundJob {
        id: "e772d00b".into(),
        needs: None,
        awaits_prompt: false,
    });
    row.process.as_mut().unwrap().label = Some("bg".into());
    row.model = None;
    row.effort = None;
    let config = tempfile::tempdir().unwrap();
    crate::providers::claude::write_fixture_cache(config.path());
    let provider = crate::providers::claude::ClaudeProvider::new(config.path().into());
    view.set_input_choices(HashMap::from([(
        ProviderId::Claude,
        crate::providers::Provider::input_choices(&provider),
    )]));
    let screen = event_screen(&view, 100, 45, now);
    assert!(!screen.contains("Unknown"), "{screen}");
    assert!(!screen.contains("Default"), "{screen}");
    assert!(view.viewport.borrow().field_targets.is_empty());
}

#[test]
fn help_lines_fit() {
    let view = View::default();
    for width in [40, 54, 58, 100, 200] {
        let lines = view.center_help_lines(width);
        assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
    }
}

#[test]
fn event_border_bottom_double_click_selects_latest_only_when_not_shared() {
    for width in [100, 149, 150, 200] {
        let (mut view, now) = event_fixture();
        view.select_fixture_event(Some(view.event_panel[0].clone()));
        event_screen(&view, width, 20, now);
        let event_panel = view.viewport.borrow().event_panel;
        let mouse = mouse(
            MouseEventKind::Down(MouseButton::Left),
            event_panel.x + event_panel.width / 3,
            event_panel.bottom(),
        );
        click_mouse(&mut view, mouse);
        click_mouse(&mut view, mouse);
        if super::render::ScreenLayout::for_width(width) != super::render::ScreenLayout::Large {
            assert_eq!(
                view.visible_event(),
                view.event_panel.first(),
                "width {width}"
            );
            assert!(view.active_panel == Panel::EventDetail);
        } else {
            assert_eq!(
                view.visible_event(),
                view.event_panel.last(),
                "width {width}"
            );
            assert_eq!(view.event_scroll(), view.event_max_scroll());
        }
    }
}

#[path = "index_selection_tests.rs"]
mod index_selection;

#[test]
fn claude_agent_display_from_real_records() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude-agent-display.jsonl");
    let events = crate::providers::claude::event_reader::tail(&path, 100).unwrap();
    for (id, label, metadata) in [
        (
            "toolu_015h3Ngbytx63sMddkJwjDX7",
            "Spawn agent attach-probe",
            "general-purpose",
        ),
        (
            "toolu_01X69UrWoFVNnNKno4yavSre",
            "Spawn agent 接手边框和标题行改动",
            "feature:ux-implementer",
        ),
        (
            "toolu_01YMEgiGEMc9cjoePq6j2E69",
            "Spawn agent 滚动条挪到最右边",
            "fork",
        ),
    ] {
        let event = events
            .iter()
            .find(|e| e.calls.iter().any(|c| c.id == id))
            .unwrap();
        assert_eq!(event.summary(), label);
        assert_eq!(event.calls[0].title, label);
        let (mut view, now) = event_fixture();
        view.select_fixture_event(Some(event.clone()));
        let screen = event_screen(&view, 200, 45, now);
        let rows: Vec<_> = screen.lines().collect();
        let detail = view.viewport.borrow().event_detail;
        let header = usize::from(detail.y + 1);
        assert!(!rows[header].contains(metadata), "{screen}");
        assert!(
            !rows[header + 1..].iter().any(|row| row.trim() == metadata),
            "{screen}"
        );
        assert!(!label.is_empty());
        let description = view.display_event(event).calls[0]
            .event_detail_title
            .clone();
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
            crate::event::Kind::Tool,
            crate::event::Kind::Error,
            crate::event::Kind::Denied,
        ] {
            let mut event = event.clone();
            event.kind = kind;
            view.event_panel = vec![event.clone(), event.clone()];
            view.select_fixture_event(Some(event));
            let mut buffer = Buffer::empty(Rect::new(0, 0, 200, 45));
            view.render_at(buffer.area, &mut buffer, now);
            let title = view.viewport.borrow().event_text.sources[0]
                .text
                .to_string();
            assert_eq!(title, description);
            assert!((0..200).any(|x| cell_text(buffer[(x, header as u16)].symbol()) == "S"));
        }
        let description = event.calls[0].event_detail_title.clone();
        assert!(
            description.is_empty()
                || !rows[header + 1..]
                    .iter()
                    .any(|row| row.replace(' ', "") == description.replace(' ', "")),
            "{screen}"
        );
    }
}

#[test]
fn session_search_overlays_bottom_of_unchanged_usage_chart() {
    for (width, height) in [(200, 50), (120, 45), (80, 12), (60, 6)] {
        let (mut view, now) = event_fixture();
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
                    cell_text(before[(x, y)].symbol())
                        .chars()
                        .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
                })
            })
            .unwrap_or(baseline.bottom());
        view.searching = true;
        view.search.insert("Task");
        let layout = view.center_layout(area);
        assert_eq!(layout.usage_chart, baseline);
        if baseline.is_empty() {
            assert!(layout.search.is_empty());
            assert!(layout.list.is_empty());
            continue;
        }
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
                cell_text(after[(x, layout.search.y)].symbol())
                    .chars()
                    .all(|c| !('\u{2801}'..='\u{28ff}').contains(&c))
            }),
            "search row must not contain chart dots: {width}x{height}"
        );
        view.show_error("Notice".into());
        view.render_at(area, &mut after, now);
        let input: String = (layout.search.x..layout.search.right())
            .map(|x| cell_text(after[(x, layout.search.y)].symbol()))
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
        view.set_active_panel(Panel::TaskDetails);
        let rename = view.center_layout(area);
        assert!(rename.search.is_empty());
        assert_eq!(rename.list, baseline);
        assert_eq!(rename.usage_chart, layout.usage_chart);
        view.render_at(area, &mut after, now);
        let title = view.viewport.borrow().title_input;
        if !title.is_empty() {
            assert_eq!(view.cursor_pos(area).unwrap().1, title.y);
        }
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

#[test]
fn pending_archive_preserves_tab_and_nearby_selection_after_refresh() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        for position in 0..3 {
            let mut view = fixture();
            let mut rows = view.rows.iter().take(3).cloned().collect::<Vec<_>>();
            for row in &mut rows {
                row.id.provider = provider;
                row.status = SessionStatus::Ready;
                row.parent = None;
            }
            view.set_sessions(rows, false);
            let visible = view.selectable_indices();
            let target = view.rows[visible[position]].id.clone();
            view.select(visible[position]);
            view.begin_removal(&target);
            let next = view.selected_id().cloned();
            assert!(next.is_some());
            let mut rows = view.rows.clone();
            rows.iter_mut().find(|row| row.id == target).unwrap().status = SessionStatus::Inactive;
            // Filesystem refreshes can arrive before the worker completion.
            view.set_sessions(rows.clone(), false);
            assert_eq!(view.selected_id(), next.as_ref());
            view.finish_removal(&target);
            view.set_sessions(rows, false);
            assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
            assert_eq!(view.selected_id(), next.as_ref());
            key(&mut view, KeyCode::Tab);
            assert_eq!(view.selected_id(), Some(&target));
            let rendered = screen(&view, 120, 40);
            assert!(rendered.contains("Inactive"));
        }
    }
}

#[path = "structured_ui_tests.rs"]
mod structured_ui_tests;

#[path = "event_agent_tests.rs"]
mod event_agent_tests;

fn update_event_snapshot_body(path: &Path, rendered: &str) {
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        let previous = std::fs::read_to_string(path).unwrap();
        let footer = previous.lines().last().unwrap();
        let (body, _) = rendered.rsplit_once(" ? help").unwrap();
        std::fs::write(path, format!("{body}{footer}\n")).unwrap();
    }
}

#[test]
fn task_details_model_has_no_prefix() {
    let mut row = session("weekly", "Weekly task", SessionStatus::Ready, 0);
    row.model = Some("claude-opus".into());
    row.effort = Some("high".into());
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 100, 20);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, Utc::now());
    let lines = buffer
        .content
        .chunks(100)
        .map(|row| {
            row.iter()
                .map(|cell| cell_text(cell.symbol()))
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("claude-opus ") && line.contains(" high")),
        "{lines:?}"
    );
}

fn weekly_usage_view() -> View {
    let mut view = fixture();
    let hub = crate::sessions::Hub::new(vec![
        Box::new(crate::providers::codex::CodexProvider::new(
            PathBuf::new(),
            PathBuf::new(),
        )),
        Box::new(crate::providers::claude::ClaudeProvider::new(PathBuf::new())),
    ]);
    view.provider_names = hub.display_names();
    view.weekly_usage = HashMap::from([
        (
            ProviderId::Claude,
            crate::providers::WeeklyUsage {
                used_percent: 21.0,
                reset_credits: None,
                resets_at: None,
            },
        ),
        (
            ProviderId::Codex,
            crate::providers::WeeklyUsage {
                used_percent: 5.0,
                reset_credits: None,
                resets_at: None,
            },
        ),
    ]);
    view
}

fn weekly_header(view: &View, width: u16) -> String {
    let text = screen(view, width, 40);
    let y = view.viewport.borrow().tab_bar.y;
    text.lines().nth(usize::from(y)).unwrap().to_owned()
}

#[test]
fn task_header_weekly_usage_mirrored_modes_and_widths() {
    for width in [200, 160, 150, 149, 80, 39, 40] {
        for mode in [
            crate::state::UsageView::Compact,
            crate::state::UsageView::Detailed,
        ] {
            let mut view = weekly_usage_view();
            view.usage_view = mode;
            let text = weekly_header(&view, width);
            let viewport = view.viewport.borrow();
            let target = viewport
                .field_targets
                .iter()
                .find(|(_, f)| *f == Field::WeeklyUsage)
                .map(|(r, _)| *r);
            if width <= 40 {
                assert!(target.is_none(), "{width}: {text}");
                assert!(!text.contains('%'));
                continue;
            }
            let target = target.unwrap_or_else(|| panic!("{width}, {mode:?}: {text}"));
            assert!(
                viewport
                    .tabs
                    .iter()
                    .all(|(_, tab)| target.right() < tab.x || target.x > tab.right())
            );
            if mode == crate::state::UsageView::Compact {
                assert!(text.contains("Claude · Codex"), "{text}");
                assert!(text.contains("21%") && text.contains("5%"), "{text}");
                let budget =
                    if render::ScreenLayout::for_width(width) == render::ScreenLayout::Large {
                        width / 2
                    } else {
                        width * 3 / 4
                    };
                assert!(target.width <= budget);
                assert_eq!(target.x + target.width / 2, width / 2);
                if width >= 149 {
                    assert_eq!(target.width, (budget - 1) / 2 * 2 + 1);
                }
                if width == 80 {
                    assert!(target.width > width / 2);
                }
            } else {
                assert!(text.contains("Claude · Codex"), "{text}");
                assert_eq!(text.chars().nth(usize::from(width / 2)), Some('·'));
            }
        }
    }
    let view = weekly_usage_view();
    let snapshot = weekly_header(&view, 160).trim_end().to_owned() + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/task_details_weekly_usage.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
    let mut view = weekly_usage_view();
    view.usage_view = crate::state::UsageView::Detailed;
    view.weekly_usage
        .get_mut(&ProviderId::Claude)
        .unwrap()
        .reset_credits = Some(1);
    view.weekly_usage
        .get_mut(&ProviderId::Codex)
        .unwrap()
        .reset_credits = Some(2);
    for usage in view.weekly_usage.values_mut() {
        usage.resets_at = Some("2026-10-23T16:00:00Z".parse().unwrap());
    }
    let snapshot = weekly_header(&view, 160).trim_end().to_owned() + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/task_details_weekly_usage_detailed.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn task_header_weekly_usage_symmetric_bars_and_styles() {
    for width in [200, 160, 150, 149, 80] {
        for used in [0.0, 5.0, 23.0, 50.0, 95.0, 96.0, 99.0, 100.0] {
            for colors in [false, true] {
                let mut view = weekly_usage_view();
                view.use_colors = colors;
                for usage in view.weekly_usage.values_mut() {
                    usage.used_percent = used;
                }
                let area = Rect::new(0, 0, width, 40);
                let mut buffer = Buffer::empty(area);
                view.render_at(area, &mut buffer, Utc::now());
                let viewport = view.viewport.borrow();
                let y = viewport.tab_bar.y;
                let target = viewport
                    .field_targets
                    .iter()
                    .find(|(_, f)| *f == Field::WeeklyUsage)
                    .unwrap()
                    .0;
                for (_, tab) in &viewport.tabs {
                    assert!(target.right() < tab.x || target.x > tab.right());
                    let gap = if tab.x < target.x {
                        tab.right()
                    } else {
                        target.right()
                    };
                    assert_eq!(buffer[(gap, y)].symbol(), " ");
                    assert_eq!(buffer[(gap, y)].bg, buffer[(width / 2, y)].bg);
                }
                let half = (target.width - 1) / 2;
                let number = format!("{used:.0}%");
                let capacity = half - 8;
                let dots = (f64::from(capacity * 2) * used / 100.0).round() as u16;
                let center = width / 2;
                assert_eq!(buffer[(center, y)].symbol(), "·");
                let mut starts = [0; 2];
                for (side, start) in starts.iter_mut().enumerate() {
                    let limit = half - 6 - 3 - number.len() as u16;
                    let visible = dots.min(limit * 2);
                    let cells = visible.div_ceil(2);
                    let anchor = if side == 0 {
                        target.x
                    } else {
                        target.right() - 1
                    };
                    *start = if side == 0 {
                        anchor + cells + u16::from(cells > 0)
                    } else {
                        anchor + 1 - cells - u16::from(cells > 0) - number.len() as u16
                    };
                    for dot in 0..visible {
                        let x = if side == 0 {
                            anchor + dot / 2
                        } else {
                            anchor - dot / 2
                        };
                        let bit = if (dot % 2 == 0) == (side == 1) {
                            0xb8
                        } else {
                            0x47
                        };
                        let cell = &buffer[(x, y)];
                        let bits = cell_text(cell.symbol()).chars().next().unwrap() as u32;
                        assert!((0x2800..=0x28ff).contains(&bits));
                        assert_eq!((bits - 0x2800) & bit, bit, "{width}, {used}, {dot}");
                        let bar_style = if used > 95.0 {
                            style::error_style()
                        } else {
                            style::timeline_style(colors)
                        };
                        assert_eq!(cell.style(), buffer[(0, y)].style().patch(bar_style));
                    }
                    if visible % 2 == 1 {
                        let x = if side == 0 {
                            anchor + cells - 1
                        } else {
                            anchor + 1 - cells
                        };
                        assert_eq!(
                            cell_text(buffer[(x, y)].symbol()),
                            if side == 0 { "⡇" } else { "⢸" }
                        );
                    }
                    let outside = if side == 0 {
                        *start + number.len() as u16..center - 8
                    } else {
                        center + 7..*start
                    };
                    for x in outside {
                        assert_eq!(buffer[(x, y)].symbol(), " ");
                    }
                }
                let [left_number, right_number] = starts;
                for start in [left_number, right_number] {
                    for (i, c) in number.chars().enumerate() {
                        let cell = &buffer[(start + i as u16, y)];
                        assert_eq!(cell_text(cell.symbol()), c.to_string());
                        let expected = if used > 95.0 {
                            style::error_style()
                        } else {
                            ratatui::style::Style::default()
                        };
                        assert_eq!(cell.style(), buffer[(0, y)].style().patch(expected));
                    }
                }
                let text = (0..width)
                    .map(|x| cell_text(buffer[(x, y)].symbol()))
                    .collect::<String>();
                assert!(text.contains("Claude · Codex"), "{text}");
            }
        }
    }
}

#[test]
fn task_header_weekly_usage_single_provider() {
    for width in [200, 80] {
        for mode in [
            crate::state::UsageView::Compact,
            crate::state::UsageView::Detailed,
        ] {
            for missing in [ProviderId::Claude, ProviderId::Codex] {
                let mut view = weekly_usage_view();
                view.usage_view = mode;
                view.weekly_usage.remove(&missing);
                let text = weekly_header(&view, width);
                assert!(!text.contains('·'), "{text}");
                let cells = text.chars().collect::<Vec<_>>();
                let center = usize::from(width / 2);
                let viewport = view.viewport.borrow();
                let target = viewport
                    .field_targets
                    .iter()
                    .find(|(_, f)| *f == Field::WeeklyUsage)
                    .unwrap()
                    .0;
                let (present, absent) = if missing == ProviderId::Codex {
                    ("Claude", "Codex")
                } else {
                    ("Codex", "Claude")
                };
                assert!(text.contains(present) && !text.contains(absent), "{text}");
                if mode == crate::state::UsageView::Compact {
                    assert_eq!(target.x + target.width / 2, width / 2);
                    let empty = if missing == ProviderId::Codex {
                        center..usize::from(target.right())
                    } else {
                        usize::from(target.x)..center + 1
                    };
                    assert!(cells[empty].iter().all(|c| *c == ' '));
                } else if missing == ProviderId::Codex {
                    assert_eq!(cells[center - 2], 'e');
                } else {
                    assert_eq!(cells[center + 2], 'C');
                }
            }
        }
    }
}

#[test]
fn task_header_weekly_usage_details_and_compact_reset() {
    let mut view = weekly_usage_view();
    for (provider, count) in [(ProviderId::Claude, 1), (ProviderId::Codex, 2)] {
        let usage = view.weekly_usage.get_mut(&provider).unwrap();
        usage.used_percent = 97.0;
        usage.resets_at = Some("2026-10-23T16:00:00Z".parse().unwrap());
        usage.reset_credits = Some(count);
    }
    for width in [160, 80] {
        let text = weekly_header(&view, width);
        assert_eq!(text.matches("97%").count(), 2, "{text}");
        assert!(text.contains("Claude · Codex"));
        assert!(!text.contains("credit") && !text.contains("until") && !text.contains("resets"));
        view.usage_view = crate::state::UsageView::Detailed;
        let text = weekly_header(&view, width);
        if width == 160 {
            assert!(
                text.contains("1 credit, resets ") && text.contains("Claude · Codex"),
                "{text}"
            );
            assert!(text.contains("2 credits"));
            assert_eq!(text.chars().nth(usize::from(width / 2)), Some('·'));
        } else {
            assert!(!text.contains("resets"), "{text}");
        }
        view.usage_view = crate::state::UsageView::Compact;
    }
}

#[test]
fn task_header_weekly_usage_click_targets_cover_the_whole_visible_label() {
    for width in [160, 80] {
        for mode in [
            crate::state::UsageView::Compact,
            crate::state::UsageView::Detailed,
        ] {
            let mut view = weekly_usage_view();
            view.usage_view = mode;
            weekly_header(&view, width);
            let target = view
                .viewport
                .borrow()
                .field_targets
                .iter()
                .find(|(_, f)| *f == Field::WeeklyUsage)
                .unwrap()
                .0;
            for x in target.x..target.right() {
                view.usage_view = mode;
                view.handle_mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: x,
                    row: target.y,
                    modifiers: KeyModifiers::NONE,
                });
                let action = view.handle_mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: x,
                    row: target.y,
                    modifiers: KeyModifiers::NONE,
                });
                assert_eq!(
                    action,
                    Some(Action::PersistUsageView(
                        if mode == crate::state::UsageView::Compact {
                            crate::state::UsageView::Detailed
                        } else {
                            crate::state::UsageView::Compact
                        }
                    )),
                    "x={x}"
                );
            }
        }
    }
}

#[test]
fn task_header_weekly_usage_absent_or_too_wide() {
    let mut view = weekly_usage_view();
    for width in [25, 28, 30] {
        let text = weekly_header(&view, width);
        assert!(!text.contains('%') && !text.contains('·'), "{text}");
        assert!(
            !view
                .viewport
                .borrow()
                .field_targets
                .iter()
                .any(|(_, f)| *f == Field::WeeklyUsage)
        );
    }
    view.weekly_usage.clear();
    assert!(!weekly_header(&view, 160).contains('%'));
}
#[test]
fn task_details_weekly_usage_keeps_model_and_effort_targets() {
    let mut view = weekly_usage_view();
    let mut row = session("weekly", "Weekly task", SessionStatus::Ready, 0);
    row.model = Some("claude-opus".into());
    row.effort = Some("high".into());
    view.set_sessions(vec![row], false);
    let area = Rect::new(5, 3, 100, 40);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, Utc::now());
    let targets = &view.viewport.borrow().field_targets;
    let layout = view.center_layout(area);
    for (field, x, width) in [
        (Field::Model, area.x + 2, 11),
        (Field::Effort, layout.gap.x - 4, 4),
    ] {
        let rect = targets
            .iter()
            .find(|(_, candidate)| *candidate == field)
            .unwrap()
            .0;
        assert_eq!((rect.x, rect.width), (x, width));
        let line = (area.x..area.right())
            .map(|x| cell_text(buffer[(x, rect.y)].symbol()))
            .collect::<String>();
        assert!(
            line.contains("claude-opus ") && line.contains(" high"),
            "{line}"
        );
        assert!(
            !line.contains("weekly") && !line.contains("Model:"),
            "{line}"
        );
    }
}

#[test]
#[ignore = "reads local credentials and live provider usage; requires the running Codex daemon"]
fn task_header_weekly_usage_live_accounts() {
    let (mut hub, _) = crate::providers::configured().unwrap();
    let sessions = hub.refresh().sessions;
    let jobs = hub.weekly_usage_jobs(std::time::Instant::now());
    let workers = jobs
        .into_iter()
        .map(|(provider, job)| (provider, std::thread::spawn(job)))
        .collect::<Vec<_>>();
    let mut errors = Vec::new();
    for (provider, worker) in workers {
        let result = worker.join().unwrap();
        if let Err(error) = &result {
            errors.push(format!("{provider}: {error}"));
        }
        hub.weekly_usage_complete(provider, result);
    }
    let mut lines = Vec::new();
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        if !hub.weekly_usage().contains_key(&provider) {
            errors.push(format!("{provider}: live weekly usage unavailable"));
            continue;
        }
        let row = sessions
            .iter()
            .find(|row| row.id.provider == provider && row.model.is_some())
            .expect("real session with model")
            .clone();
        let mut view = test_view(Grouping::Project, false);
        view.weekly_usage = hub.weekly_usage().clone();
        view.provider_names = hub.display_names();
        view.status_filter = usize::from(row.status == SessionStatus::Inactive);
        view.set_sessions(vec![row], false);
        let area = Rect::new(0, 0, 160, 80);
        let mut buffer = Buffer::empty(area);
        view.render_at(area, &mut buffer, Utc::now());
        let text = buffer
            .content
            .chunks(160)
            .map(|row| {
                row.iter()
                    .map(|cell| cell_text(cell.symbol()))
                    .collect::<String>()
            })
            .find(|line| line.contains('%') && line.contains("Active "))
            .expect("rendered live usage line");
        assert!(!text.contains("Model: "));
        println!("{provider}: {}", text.trim());
        lines.push(text.trim().to_owned());
    }
    if let Some(path) = std::env::var_os("AGENTS_USAGE_PROBE_RESULT") {
        std::fs::write(
            path,
            serde_json::to_vec(&serde_json::json!({
                "verdict": if errors.is_empty() { "passed" } else { "failed" },
                "lines": lines,
                "errors": errors,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    assert!(errors.is_empty(), "{}", errors.join("; "));
}

#[test]
fn session_models_group_by_catalog_display_name() {
    let config = tempfile::tempdir().unwrap();
    crate::providers::claude::write_fixture_cache(config.path());
    let provider = crate::providers::claude::ClaudeProvider::new(config.path().into());
    let choices = crate::providers::Provider::input_choices(&provider);
    let model = choices.model("opus").unwrap().id.clone();
    let mut view = test_view(Grouping::Model, false);
    view.set_input_choices(HashMap::from([(ProviderId::Claude, choices)]));
    let mut first = session("catalog", "Catalog", SessionStatus::Ready, 0);
    first.model = Some(model.clone());
    let mut second = first.clone();
    second.id = session_key("alias");
    second.model = Some(format!("{model}[1m]"));
    view.set_sessions(vec![first, second], false);
    assert!(view.same_group(Grouping::Model, 0, 1));
    assert_eq!(
        view.session_model_text(&view.rows[0]),
        view.session_model_text(&view.rows[1])
    );
}

#[test]
fn recorded_agent_settings_use_the_picker_catalog_and_shared_effort() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("models_cache.json"),
        include_str!("../../tests/fixtures/codex-models-cache.json"),
    )
    .unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        directory.path().into(),
        directory.path().join("manager"),
    );
    let mut row = session("settings", "Settings", SessionStatus::Ready, 0);
    row.id.provider = ProviderId::Codex;
    row.transcript = Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex-interaction/parent.jsonl"),
    );
    let mut reader = crate::providers::event_reader(&row).unwrap();
    let events = reader.refresh(usize::MAX).unwrap().events;
    let event = events
        .iter()
        .find(|event| event.calls.iter().any(|call| call.model_settings.is_some()))
        .unwrap();
    let mut view = test_view(Grouping::Model, false);
    view.set_input_choices(HashMap::from([(
        ProviderId::Codex,
        crate::providers::Provider::input_choices(&provider),
    )]));
    let call = event
        .calls
        .iter()
        .find(|call| call.model_settings.is_some())
        .unwrap();
    let settings = call.model_settings.as_ref().unwrap();
    assert_eq!(settings.model.as_deref(), Some("gpt-6.1-sol"));
    assert!(!call.event_detail_title.contains("GPT-6.1-Sol"));
    let display = view.display_event(event);
    assert!(
        display
            .calls
            .iter()
            .any(|call| call.event_detail_title.contains("GPT-6.1-Sol · low"))
    );
    let mut missing = event.clone();
    for call in &mut missing.calls {
        if let Some(settings) = &mut call.model_settings {
            settings.effort = None;
        }
    }
    let display = view.display_event(&missing);
    assert!(
        display
            .calls
            .iter()
            .any(|call| call.event_detail_title.ends_with("GPT-6.1-Sol"))
    );
}

#[path = "codex_background_tests.rs"]
mod codex_background_tests;

#[test]
fn context_usage_on_task_details_border_keeps_status_and_elapsed() {
    for (provider, fixture, percent) in [
        (ProviderId::Claude, "transcript.jsonl", Some(5)),
        (ProviderId::Codex, "codex-latest-activity.jsonl", Some(9)),
    ] {
        let (mut view, now) = event_fixture();
        view.rows[view.selected].id.provider = provider;
        view.rows[view.selected].started_at = Some(now - Duration::seconds(90));
        let id = view.selected_id().unwrap().clone();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let mut usage = crate::usage::Series::default();
        usage.update(&path, provider).unwrap();
        view.set_context_usage(&id, usage.context_percent());
        for width in [100, 120, 200] {
            let area = Rect::new(0, 0, width, 40);
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, now);
            let task_details = view.center_layout(area).task_details;
            let border: String = (task_details.x..task_details.right())
                .map(|x| cell_text(buf[(x, task_details.y - 1)].symbol()))
                .collect();
            if let Some(percent) = percent {
                assert!(border.starts_with(&format!("{percent}% ")), "{border}");
                assert!(
                    border.find('%').unwrap() < border.find("Working").unwrap(),
                    "{border}"
                );
            } else {
                assert!(!border.contains('%'), "{border}");
            }
            assert!(border.contains("Working"), "{border}");
            assert!(border.contains("1m 30s"), "{border}");
        }
        view.set_context_usage(&session_key("other-session"), Some(80));
        assert!(!screen(&view, 120, 40).lines().nth(1).unwrap().contains('%'));
        view.set_context_usage(&id, None);
        assert!(!screen(&view, 120, 40).lines().nth(1).unwrap().contains('%'));
    }
}

#[test]
fn claude_context_usage_over_a_quarter_uses_error_color() {
    for (provider, percent, red) in [
        (ProviderId::Claude, 25, false),
        (ProviderId::Claude, 26, true),
        (ProviderId::Codex, 80, false),
    ] {
        let (mut view, now) = event_fixture();
        view.rows[view.selected].id.provider = provider;
        let id = view.selected_id().unwrap().clone();
        view.set_context_usage(&id, Some(percent));
        let area = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let task_details = view.center_layout(area).task_details;
        let cell = &buf[(task_details.x, task_details.y - 1)];
        assert_eq!(cell_text(cell.symbol()), &percent.to_string()[..1]);
        assert_eq!(
            cell.fg == style::error_style().fg.unwrap(),
            red,
            "{provider:?} {percent}"
        );
    }
}

#[path = "summary_consistency_tests.rs"]
mod summary_consistency_tests;

#[path = "overlay_tests.rs"]
mod overlay_tests;

#[path = "window_navigation_tests.rs"]
mod window_navigation_tests;

#[path = "active_panel_tests.rs"]
mod active_panel_tests;

#[path = "task_tree_collapse_tests.rs"]
mod task_tree_collapse_tests;

#[path = "transcript_tests.rs"]
mod transcript_tests;

#[test]
fn selector_hides_task_details_rules_through_panel_padding() {
    use super::draft_controls::{Selector, SelectorKind, TaskKey};
    for width in [100, 120, 149, 150] {
        for effort in [false, true] {
            let mut view = View::new(Grouping::Project, false);
            let parent = session("parent", "Parent", SessionStatus::Working, 0);
            let mut child = session("child", "Child", SessionStatus::Working, 0);
            child.parent = Some(parent.id.clone());
            child.started_at = Some(parent.updated_at);
            view.set_sessions(vec![parent.clone(), child], false);
            view.selected = view
                .rows
                .iter()
                .position(|row| row.id == parent.id)
                .unwrap();
            let area = Rect::new(0, 0, width, 40);
            let now = parent.updated_at;
            let mut buffer = Buffer::empty(area);
            view.render_at(area, &mut buffer, now);
            let details = view.center_layout(area).task_details;
            let rules: Vec<_> = (details.y..details.bottom())
                .filter(|&y| (details.x..details.right()).any(|x| buffer[(x, y)].symbol() == "●"))
                .collect();
            if width < 150 {
                assert!(
                    !rules.is_empty(),
                    "fixture must draw a background indicator at {width}"
                );
            }
            let values = vec![
                "low".into(),
                "medium".into(),
                "high".into(),
                "xhigh".into(),
                "max".into(),
            ];
            let target = TaskKey::Session(parent.id.clone());
            view.selector = Some(Selector::new(
                if effort {
                    SelectorKind::Effort {
                        target,
                        labels: values.clone(),
                        values,
                    }
                } else {
                    SelectorKind::Model {
                        target,
                        labels: values.clone(),
                        families: vec!["Models".into(); values.len()],
                        values,
                    }
                },
                0,
            ));
            view.render_at(area, &mut buffer, now);
            for y in rules {
                assert_eq!(
                    buffer[(details.right(), y)].symbol(),
                    " ",
                    "residue at ({}, {y}), width {width}, effort {effort}",
                    details.right()
                );
            }
            assert!(view.viewport.borrow().task_details_text.rows.is_empty());
        }
    }
}
