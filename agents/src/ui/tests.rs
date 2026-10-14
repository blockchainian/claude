fn session_key(id: &str) -> SessionKey {
    SessionKey {
        provider: crate::providers::ProviderId::Claude,
        id: id.into(),
    }
}

use super::*;
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
    assert_eq!(view.status_filter, idx);
    assert_eq!(view.viewport.borrow().scroll, 0);
    assert_eq!(view.selected_row().unwrap().status, SessionStatus::Ready);
}

#[test]
fn mouse_click_outside_tabs_changes_nothing() {
    let mut view = fixture();
    screen(&view, 120, 40);
    view.viewport.borrow_mut().scroll = 7;
    let selected = view.selected;
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0))
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
            view.handle_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                rect.x,
                rect.y
            ))
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
    selected.status = SessionStatus::NeedsInput;
    selected.prompt = "A long prompt that wraps across several rows. ".repeat(8);
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
    let before = log_screen(&view, 100, 25, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 25)).log;
    assert!(before.contains("35s Tool      run tests"), "{before}");
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x + 2, rect.y + 1));
    let after = log_screen(&view, 100, 25, now);
    assert!(after.contains("Older event"), "{after}");
    assert!(!after.contains("run tests"));
    for y in rect.y + 1..rect.bottom() - 3 {
        assert_eq!(
            before.lines().nth(y as usize + 1),
            after.lines().nth(y as usize)
        );
    }
    // A tool at the visible edge must use its recorded duration, even if unfinished.
    view.log[7] = view.log[8].clone();
    view.log[7].body = "Earlier unfinished tool".into();
    let after = log_screen(&view, 100, 25, now);
    assert!(
        after.contains("5s Tool      Earlier unfinished tool"),
        "{after}"
    );
    assert!(!after.contains("35s Tool"));
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y + 1));
    assert!(log_screen(&view, 100, 25, now).contains("35s Tool      run tests"));
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
            event
        })
        .collect();
    view.log.last_mut().unwrap().previous_end = None;
    let before = log_screen(&view, 100, 25, now);
    let rect = view.viewport.borrow().log;
    assert!(before.contains("11:59:30 Tool      Event 11"), "{before}");
    assert!(before.contains("12s Tool      Event 10"), "{before}");
    let (before_x, before_y) = rendered_cell(&before, "Event 10");
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x + 2, rect.y + 1));
    let after = log_screen(&view, 100, 25, now);
    assert!(!after.contains("Event 11"), "{after}");
    let (after_x, after_y) = rendered_cell(&after, "Event 10");
    assert_eq!(after_y + 1, before_y);
    assert_eq!(after_x, before_x);
}

#[test]
fn mouse_log_wheel_up_at_newest_stays_at_newest() {
    let (mut view, now) = log_fixture();
    let before = log_screen(&view, 100, 25, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 25)).log;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y + 1));
    assert_eq!(before, log_screen(&view, 100, 25, now));
}

#[test]
fn mouse_log_scroll_tracks_selected_session_identity() {
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    assert_eq!(view.log_scroll(), 1);
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
    assert_eq!(view.log_scroll(), 1);
    rows.retain(|row| row.id != id);
    for row in &mut rows {
        row.transcript = Some(row.cwd.join("session.jsonl"));
    }
    view.set_sessions(rows, false);
    assert_eq!(view.log_scroll(), 0);
    details_panel(&view);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    assert_eq!(view.log_scroll(), 1);
    key(&mut view, KeyCode::Down);
    assert_eq!(view.log_scroll(), 0);
}

#[test]
fn mouse_details_scroll_resets_with_selection_and_filter() {
    for code in [KeyCode::Down, KeyCode::End, KeyCode::PageDown, KeyCode::Tab] {
        let mut view = overflowing_details();
        details_panel(&view);
        let rect = view.viewport.borrow().details;
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
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
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    let (_, tab) = view.viewport.borrow().tabs[3];
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), tab.x, tab.y));
    assert_eq!(view.selected_id(), Some(&id));
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
    let full_area = Rect::new(7, 4, 43, 60);
    let mut full = Buffer::empty(full_area);
    view.render_details(full_area, &mut full, now);
    let top = (full_area.y..full_area.bottom())
        .find(|y| full[(full_area.x, *y)].symbol() == "─")
        .unwrap();
    let area = Rect::new(7, 4, 43, 10);
    view.viewport.borrow_mut().details_scroll.offset = usize::from(top - full_area.y);
    let mut clipped = Buffer::empty(area);
    view.render_details(area, &mut clipped, now);
    assert_eq!(clipped[(area.x + 2, area.y + 1)].symbol(), "B");
    let scroll = view.viewport.borrow().details_scroll.offset as u16;
    for y in area.y + 1..area.bottom() - 1 {
        for x in area.x..area.right() {
            assert_eq!(clipped[(x, y)], full[(x, y + scroll)]);
        }
    }
    view.viewport.borrow_mut().details_scroll.offset = usize::MAX;
    view.render_details(area, &mut clipped, now);
    let link = clipped
        .content
        .iter()
        .find(|cell| cell.symbol().contains("https://example.com/pull/833"))
        .expect("visible pull request hyperlink");
    assert!(full.content.contains(link));
    assert!(matches!(
        link.diff_option,
        ratatui::buffer::CellDiffOption::ForcedWidth(_)
    ));
}

#[test]
fn details_scroll_tracks_session_identity_across_refreshes() {
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
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
fn mouse_list_click_selects_and_double_click_opens_second_task() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let second = view.selectable_indices()[1];
    let id = view.rows[second].id.clone();
    let (x, y) = rendered_cell(&rendered, view.rows[second].display_title());
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(view.handle_mouse(click).is_none());
    assert_eq!(view.selected, second);
    assert!(matches!(
        view.handle_mouse(click),
        Some(Action::Open(opened)) if opened == id
    ));
}

#[test]
fn mouse_list_slow_or_split_clicks_only_select() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let [first, second] = [0, 1].map(|i| view.selectable_indices()[i]);
    let [first_click, second_click] = [first, second].map(|row| {
        let (x, y) = rendered_cell(&rendered, view.rows[row].display_title());
        mouse(MouseEventKind::Down(MouseButton::Left), x, y)
    });
    assert!(view.handle_mouse(first_click).is_none());
    assert!(view.handle_mouse(second_click).is_none());
    assert_eq!(view.selected, second);
    view.last_click.as_mut().unwrap().1 -= DOUBLE_CLICK_INTERVAL;
    assert!(view.handle_mouse(second_click).is_none());
    assert_eq!(view.selected, second);
}

#[test]
fn mouse_list_group_and_gap_clicks_do_nothing() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let selected = view.selected;
    let (x, y) = rendered_cell(&rendered, "Pinned");
    let (_, next_group) = rendered_cell(&rendered, "/tmp/project");
    for row in [y, next_group - 1] {
        assert!(
            view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, row))
                .is_none()
        );
        assert_eq!(view.selected, selected);
    }
}

#[test]
fn mouse_list_wheel_uses_keyboard_selection() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let (x, y) = rendered_cell(&rendered, "Tasks");
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
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y)),
        Some(Action::ShowMore)
    ));
    assert_eq!(view.selected, usize::MAX);
    assert!(view.loading);
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y))
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
    let (x, y) = rendered_cell(&rendered, view.rows[target].display_title());
    assert!(view.viewport.borrow().scroll > 0);
    view.viewport.borrow_mut().scroll = 0;
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(view.handle_mouse(click).is_none());
    assert_eq!(view.selected, target);
    assert!(
        matches!(view.handle_mouse(click), Some(Action::Open(id)) if id == view.rows[target].id)
    );
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
        let rendered = screen(&view, 120, 40);
        let second = view.selectable_indices()[1];
        let (x, y) = rendered_cell(&rendered, view.rows[second].display_title());
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
        queued: Vec::new(),
        prompt_at: None,
        turn_messages: 0,
        last_record_at: None,
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
        pending_questions: Vec::new(),
        background: Vec::new(),
        last_prompt: None,
        transcript: None,
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
    view.selected = view
        .rows
        .iter()
        .position(|s| s.id == session_key("42"))
        .unwrap();
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
    // At 56 columns the filled tab strip hides Inactive; the shortcut columns still fit.
    let narrow = screen(&view, 56, 24);
    let wide = screen(&view, 60, 24);
    assert_eq!(
        narrow.lines().skip(3).collect::<Vec<_>>(),
        wide.lines().skip(3).collect::<Vec<_>>()
    );
}
#[test]
fn active_tab_lists_all_sessions_except_inactive() {
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
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.visible_indices().len(), 5);
    let filters = [
        ("Ready", SessionStatus::Ready, 2),
        ("Working", SessionStatus::Working, 1),
        ("Needs you", SessionStatus::NeedsInput, 2),
        ("Inactive", SessionStatus::Inactive, 1),
    ];
    for (label, status, count) in filters {
        key(&mut view, KeyCode::Tab);
        assert_eq!(TASK_FILTERS[view.status_filter], (label, &[status][..]));
        assert_eq!(view.visible_indices().len(), count);
        assert!(
            view.visible_indices()
                .iter()
                .all(|&index| view.rows[index].status == status)
        );
    }
    key(&mut view, KeyCode::Tab);
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    for (label, status, _) in filters.into_iter().rev() {
        view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
        assert_eq!(TASK_FILTERS[view.status_filter], (label, &[status][..]));
        assert_eq!(view.selected_row().unwrap().status, status);
    }
    key(&mut view, KeyCode::BackTab);
    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
}
#[test]
fn refresh_follows_selected_ready_session_to_working_tab() {
    let mut view = fixture();
    key(&mut view, KeyCode::Tab);
    assert_eq!(view.selected_id(), Some(&session_key("42")));
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Working;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Working");
    assert_eq!(view.selected_id(), Some(&session_key("42")));
}

#[test]
fn refresh_follows_selected_needs_input_session_to_ready_tab() {
    let mut view = fixture();
    for _ in 0..3 {
        key(&mut view, KeyCode::Tab);
    }
    assert_eq!(view.selected_id(), Some(&session_key("44")));
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("44"))
        .unwrap()
        .status = SessionStatus::Ready;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Ready");
    assert_eq!(view.selected_id(), Some(&session_key("44")));
}

#[test]
fn refresh_returns_to_active_when_selected_working_session_becomes_inactive() {
    let mut view = fixture();
    for _ in 0..2 {
        key(&mut view, KeyCode::Tab);
    }
    assert_eq!(view.selected_id(), Some(&session_key("46")));
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("46"))
        .unwrap()
        .status = SessionStatus::Inactive;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some(&session_key("42")));
    assert_eq!(view.selected, view.visible_indices()[0]);
}

#[test]
fn refresh_returns_to_active_when_selected_working_session_disappears() {
    let mut view = fixture();
    for _ in 0..2 {
        key(&mut view, KeyCode::Tab);
    }
    assert_eq!(view.selected_id(), Some(&session_key("46")));
    let rows = view
        .rows
        .iter()
        .filter(|s| s.id != session_key("46"))
        .cloned()
        .collect();

    view.set_sessions(rows, true);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some(&session_key("42")));
    assert_eq!(view.selected, view.visible_indices()[0]);
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
fn refresh_selects_first_active_session_when_selection_becomes_inactive() {
    let mut view = fixture();
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Inactive;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some(&session_key("44")));
    assert_eq!(view.selected, view.visible_indices()[0]);
}

#[test]
fn refresh_keeps_tab_when_selected_session_is_hidden_only_by_search() {
    let mut view = fixture();
    key(&mut view, KeyCode::Tab);
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

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Ready");
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
    assert_eq!(
        screen(&view, 88, 16),
        include_str!("../../tests/ui_snapshots/live_center_search_active.txt")
    );
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 88, 16)), Some((15, 3)));
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

    assert_eq!(view.selected, view.visible_indices()[0]);
    assert_eq!(view.selected_id(), Some(&session_key("42")));
}

#[test]
fn empty_active_tab_does_not_page_inactive_sessions() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![session("42", "Recent task", SessionStatus::Inactive, 120)],
        true,
    );
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(!view.loading);
}

#[test]
fn live_center_show_more() {
    let mut view = View {
        status_filter: 4,
        ..test_view(Grouping::Project, true)
    };
    view.set_sessions(
        vec![session("42", "Recent task", SessionStatus::Inactive, 120)],
        true,
    );
    key(&mut view, KeyCode::Down);
    assert_eq!(
        screen(&view, 80, 12),
        include_str!("../../tests/ui_snapshots/live_center_show_more.txt")
    );
    assert_eq!(key(&mut view, KeyCode::Enter), Some(Action::ShowMore));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(screen(&view, 80, 12).contains("Loading more…"));
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
    assert_eq!(
        screen(&view, 100, 24),
        include_str!("../../tests/ui_snapshots/agents_overview_model_grouping.txt")
    );
}
#[test]
fn agents_overview_recent_sessions_twelve_rows_delta() {
    let mut view = View {
        status_filter: 4,
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
    let rendered = screen(&view, 80, 24);
    assert!(rendered.contains("Active 0"));
    assert!(rendered.contains("Inactive 12"));
    assert!(rendered.contains("/  12"));
    let expected = include_str!("../../tests/ui_snapshots/agents_overview_recent_sessions.txt");
    // The reference loads ten rows. Twelve rows change only counts and append Task 2 / Task 1.
    for (actual, expected) in rendered
        .lines()
        .skip(5)
        .take(10)
        .zip(expected.lines().skip(5).take(10))
    {
        assert_eq!(
            actual.split("Inactive").next(),
            expected.split("Inactive").next()
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
        Some(Action::Resume(session_key("42")))
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
    assert!(screen(&view, 88, 16).contains("Rename › Needs an answer"));
    key(&mut view, KeyCode::Esc);

    assert_eq!(
        key(&mut view, KeyCode::Char('g')),
        Some(Action::PersistGrouping(Grouping::Status))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('n')),
        Some(Action::New(Some("/tmp/project".into())))
    );
}
#[test]
fn paging_tracks_headings_and_never_wraps() {
    let mut view = View {
        status_filter: 4,
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
    screen(&view, 110, 18);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), Some(&session_key("10")));
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
    view.status_filter = 3;
    view.set_sessions(vec![row], false);
    let rendered = screen(&view, 160, 50);
    for label in [
        "Model: claude-opus",
        "Project · main",
        "main",
        "Needs attention",
        "Open task to review.",
        "Waiting for your response.",
        "Last message",
    ] {
        assert!(rendered.contains(label), "{label}");
    }
    let position = |label: &str| rendered.lines().position(|line| line.contains(label));
    // "Project" names the grouping in the header and the path heads the task
    // list, so find the details line that ends with the path.
    let project = rendered
        .lines()
        .position(|line| line.trim_end().ends_with("/tmp/project"));
    assert!(position("Prompt") < position("Last message"));
    assert!(position("Last message") < position("Needs attention"));
    assert!(position("Needs attention") < position("Project ·"));
    assert!(position("Project ·") < project);
    assert!(project < position("Model: claude-opus"));
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("Model: claude-opus · high"))
    );
    assert!(!rendered.contains("Reasoning"));
    let area = Rect::new(0, 0, 38, 22);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    assert_eq!(buf[(0, 21)].symbol(), "M");
    let title_cell = &buf[(14, 0)];
    assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));
    assert_eq!(title_cell.fg, Color::Reset);
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
    assert!(screen(&view, 80, 12).ends_with("  Notice"));
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
    let area = Rect::new(0, 0, 80, 12);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    let (x, y) = view.marker_pos().unwrap();
    assert_eq!(buf[(x, y)].symbol(), "›");
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    view.render(area, &mut buf);
    let (x2, y2) = view.marker_pos().unwrap();
    assert_eq!((x2, y2), (x, y + 1));
    assert_eq!(buf[(x2, y2)].symbol(), "›");
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
    assert!(screen(&view, 88, 16).contains("Rename › Current task"));
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 88, 16)), Some((23, 3)));
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
    assert!(screen(&view, 88, 16).contains("Rename ›"));
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
        .position(|(label, _)| *label == "Ready")
        .unwrap();
    key(&mut view, KeyCode::Char('r'));
    let mut rows = view.rows.clone();
    rows.iter_mut()
        .find(|s| s.id == session_key("42"))
        .unwrap()
        .status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some(&session_key("42")));
    assert!(screen(&view, 88, 16).contains("Rename › Current task"));
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
    assert_eq!(buf[(2, 3)].fg, Color::Cyan);
    assert!(
        buf[(2, 3)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert_eq!(
        buf[(80, 3)].style(),
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
    assert!(!view.delete_dialog.as_ref().unwrap().confirm);
    assert_eq!(
        view.center_footer_hints().to_string(),
        "↑/↓ move  enter select  esc back"
    );
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(view.delete_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete(session_key("42")))
    );
    assert!(view.delete_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    key(&mut view, KeyCode::Esc);
    assert!(view.delete_dialog.is_none());
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::Char('2')), None);
    assert!(view.delete_dialog.as_ref().unwrap().confirm);
    for code in [
        KeyCode::Char('o'),
        KeyCode::Char('n'),
        KeyCode::Right,
        KeyCode::Char(' '),
    ] {
        assert_eq!(key(&mut view, code), None);
        assert!(view.delete_dialog.is_some());
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
    assert!(view.delete_dialog.is_some());
    assert!(screen(&view, 120, 24).contains("This stops any running work in this task"));
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete(session_key("42")))
    );
    view.has_more = true;
    view.selected = usize::MAX;
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.delete_dialog.is_none());
}

#[test]
fn live_center_delete_dialog() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 4;
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
        buf[(6, 6)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(buf[(6, 7)].modifier.contains(ratatui::style::Modifier::DIM));
    // A painted cell reports unset colours as Reset, so compare against a painted default cell.
    let mut expected = ratatui::buffer::Cell::default();
    expected.set_style(super::style::selection_style());
    assert_eq!(buf[(6, 10)].style(), expected.style());
    for width in 0..100 {
        for height in 0..16 {
            let area = Rect::new(5, 8, width, height);
            view.render(area, &mut Buffer::empty(area));
        }
    }
}

#[test]
fn delete_dialog_number_shortcuts_follow_codex() {
    let mut view = fixture();
    key(&mut view, KeyCode::Backspace);
    assert!(view.delete_dialog.is_some());
    assert_eq!(key(&mut view, KeyCode::Char('1')), None);
    assert!(view.delete_dialog.is_none(), "1 accepts Cancel at once");
    key(&mut view, KeyCode::Backspace);
    assert_eq!(key(&mut view, KeyCode::Char('2')), None);
    assert!(
        view.delete_dialog.is_some(),
        "2 only selects the delete item"
    );
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete(session_key("42")))
    );
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
fn details_branch_and_pull_request_use_session_title_color() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let mut row = session("pr", "PR task", SessionStatus::Inactive, 120);
    row.git_branch = Some("main".into());
    row.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    let color = style::thread_color(0);
    let mut view = test_view(Grouping::Project, true);
    view.status_filter = 4;
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, 18);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    let project_y = (0..area.height)
        .find(|y| {
            (0..area.width)
                .map(|x| buf[(x, *y)].symbol())
                .collect::<String>()
                .starts_with("Project · ")
        })
        .expect("Project header");
    for (offset, symbol) in "main".chars().enumerate() {
        let cell = &buf[(10 + offset as u16, project_y)];
        assert_eq!(cell.symbol(), symbol.to_string());
        assert_eq!(cell.fg, color);
    }
    for x in 17..21 {
        assert_eq!(buf[(x, project_y)].fg, color);
    }
}

#[test]
fn details_branch_pull_request_snapshot() {
    let mut row = session("pr", "PR task", SessionStatus::Inactive, 120);
    row.git_branch = Some("main".into());
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 4;
    view.set_sessions(vec![row.clone()], false);
    let area = Rect::new(0, 0, 40, 18);
    let render = |view: &View| {
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
        buf.content
            .chunks(40)
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        let symbol = cell.symbol();
                        let symbol = symbol
                            .rsplit("\x1b\\")
                            .find(|part| !part.is_empty() && !part.starts_with("\x1b]8"))
                            .unwrap_or("");
                        symbol.split("\x1b]8").next().unwrap().to_owned()
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end_matches('\n')
            .to_owned()
            + "\n\n"
    };
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_branch.txt")
    );
    row.pr = Some(crate::model::PullRequest {
        number: 833,
        url: "https://github.com/0xbabedead/chadwallet/pull/833".into(),
    });
    view.set_sessions(vec![row.clone()], false);
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_branch_pr.txt")
    );
    for width in [13, 15, 20, 21, 40] {
        let area = Rect::new(5, 8, width, 18);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
        let link_cells = buf
            .content
            .iter()
            .enumerate()
            .filter(|(_, cell)| {
                cell.symbol()
                    .contains("\x1b]8;;https://github.com/0xbabedead/chadwallet/pull/833")
            })
            .map(|(index, _)| index % usize::from(width))
            .collect::<Vec<_>>();
        assert_eq!(link_cells, if width >= 21 { vec![17] } else { vec![] });
    }
    row.git_branch = None;
    view.set_sessions(vec![row.clone()], false);
    assert!(render(&view).contains("Project · #833"));
    row.pr = None;
    view.set_sessions(vec![row], false);
    assert!(render(&view).contains("\nProject\n/tmp/project\n"));
}

#[test]
fn details_activity_follows_the_clock() {
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
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_activity_working.txt")
    );
    assert!(rendered.contains("Latest activity · 7s ago"));
    assert!(!rendered.contains("Last message"));
    row.activity.as_mut().unwrap().label =
        "Run checks on every changed surface in the project".into();
    view.set_sessions(vec![row.clone()], false);
    let clipped = render(&view);
    assert!(clipped.contains("Run checks on every changed surface in…"));
    assert!(!clipped.contains("surface in the project"));
    // The slot follows the clock, not the session status: a newer tool call
    // still shows once the session is idle, and a newer reply replaces it.
    row.activity.as_mut().unwrap().label = "Run checks".into();
    row.status = SessionStatus::Ready;
    view.set_sessions(vec![row.clone()], false);
    assert!(render(&view).contains("Latest activity · 7s ago"));
    assert_eq!(
        render(&view),
        include_str!("../../tests/ui_snapshots/details_activity_ready.txt")
    );
    row.status = SessionStatus::Working;
    row.last_message_at = Some(now - Duration::seconds(3));
    view.set_sessions(vec![row.clone()], false);
    let replied = render(&view);
    assert!(!replied.contains("Latest activity"));
    assert_eq!(
        replied,
        include_str!("../../tests/ui_snapshots/details_activity_replied.txt")
    );
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
    let header = "Latest activity · 2m ago";
    let y = (0..area.height)
        .find(|y| buf[(0, *y)].symbol() == "L")
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
        assert_eq!(cell.fg, Color::Blue);
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
    let header = "Last message · 2m ago";
    let y = (0..area.height)
        .find(|y| buf[(0, *y)].symbol() == "L")
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
fn prompt_header_counts_singular_zero_and_clamps_elapsed_time() {
    let now: chrono::DateTime<Utc> = "2026-10-12T12:00:00Z".parse().unwrap();
    for (status, prompt_at, last_record_at, count, expected) in [
        (
            SessionStatus::Working,
            Some(now - Duration::seconds(45)),
            None,
            1,
            "Prompt · 1 message · 45s",
        ),
        (
            SessionStatus::Ready,
            Some(now),
            Some(now - Duration::seconds(1)),
            0,
            "Prompt · 0 messages · 0s",
        ),
        (
            SessionStatus::Working,
            Some(now + Duration::seconds(1)),
            None,
            0,
            "Prompt · 0 messages · 0s",
        ),
        (SessionStatus::Ready, None, Some(now), 0, "Prompt"),
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
                    let area = Rect::new(0, 0, 80, 15);
                    for advance in [0, 86401] {
                        let mut buf = Buffer::empty(area);
                        view.render_details(area, &mut buf, now + Duration::seconds(advance));
                        let header = buf.content[..80]
                            .iter()
                            .map(|c| c.symbol())
                            .collect::<String>();
                        if advance == 0 {
                            assert_eq!(
                                header.trim_end(),
                                format!("Working · Fix login bug · {expected}")
                            );
                        } else {
                            assert!(header.contains("d "), "{header}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn status_precedes_title() {
        let row = session("header", "Header task", SessionStatus::Working, 0);
        let rendered = queued_details(row, 15);
        assert_eq!(rendered.lines().next().unwrap(), "Working · Header task");
    }

    #[test]
    fn separator_is_dim_and_status_keeps_its_style() {
        for status in [
            SessionStatus::Working,
            SessionStatus::Ready,
            SessionStatus::NeedsInput,
            SessionStatus::Inactive,
        ] {
            let mut view = test_view(Grouping::Project, true);
            if status == SessionStatus::Inactive {
                view.status_filter = 4;
            }
            view.set_sessions(vec![session("header", "Header task", status, 0)], false);
            let area = Rect::new(0, 0, 80, 15);
            let mut buf = Buffer::empty(area);
            view.render_details(area, &mut buf, Utc::now());
            let label = status.label();
            let header = buf.content[..80]
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert_eq!(header.trim_end(), format!("{label} · Header task"));
            for cell in &buf.content[..label.len()] {
                assert_eq!(cell.fg, ratatui::style::Color::Reset);
                assert!(!cell.modifier.contains(Modifier::DIM));
            }
            for cell in &buf.content[label.len()..label.len() + 3] {
                assert!(cell.modifier.contains(Modifier::DIM));
            }
        }
    }

    #[test]
    fn truncated_title_keeps_elapsed_time() {
        let now = "2026-10-13T12:00:00Z".parse().unwrap();
        let mut row = session(
            "header",
            "A very long session title that exceeds the panel width",
            SessionStatus::Working,
            0,
        );
        row.prompt = "Current prompt".into();
        row.started_at = Some(now - Duration::seconds(11520));
        assert_eq!(
            details_at(row, 15, now),
            include_str!("../../tests/ui_snapshots/details_header_elapsed.txt")
        );
    }

    #[test]
    fn truncates_title_and_preserves_leading_status() {
        let mut row = session(
            "header",
            "A very long session title that exceeds the panel width",
            SessionStatus::Working,
            0,
        );
        row.prompt = "Current prompt".into();
        let rendered = queued_details(row, 15);
        let header = rendered.lines().next().unwrap();
        let title = header.strip_prefix("Working · ").expect("visible status");
        assert!(title.ends_with('…'), "{title}");
        assert_eq!(header, "Working · A very long session title tha…");
        assert_eq!(
            rendered,
            include_str!("../../tests/ui_snapshots/details_header_truncated.txt")
        );
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
    assert_eq!(lines[3], "a".repeat(40));
    assert_eq!(lines[4], "b".repeat(40));
    assert_eq!(lines[5], "c".repeat(40));
    assert_eq!(lines[6], "");
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
    assert_eq!(line(&buf, 3), wrapped[0]);
    assert_eq!(line(&buf, 4), wrapped[1]);
    assert_eq!(line(&buf, 5), "…");
    for expected in &wrapped[2..] {
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, area.x, area.y));
        buf.reset();
        view.render_details(area, &mut buf, now);
        assert_eq!(line(&buf, 4), *expected);
        assert_eq!(line(&buf, 5), "…");
    }
    let total = view.viewport.borrow().details_scroll.fallback_max.unwrap();
    for _ in 0..total {
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, area.x, area.y));
    }
    buf.reset();
    view.render_details(area, &mut buf, now);
    assert_eq!(line(&buf, 5), "Model: Unknown");
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
    assert!(rendered.find("Last message").unwrap() < rendered.find("Background\n").unwrap());
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_background.txt")
    );
    row.background[1].description = "A very long description ".repeat(5);
    assert_eq!(
        queued_details(row.clone(), 22),
        include_str!("../../tests/ui_snapshots/details_background_long_title.txt")
    );
    let clipped = queued_details(row, 14);
    assert!(clipped.contains("Background\n─"));
    assert!(clipped.contains("Last message"));
    assert!(!clipped.contains("Project"));
    assert!(clipped.ends_with("\n…"));
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
    assert!(!view.has_background());
    buf.reset();
    view.render_details(area, &mut buf, now);
    assert!(!buf.content.chunks(40).any(|row| {
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
    assert!(text.contains("Worker command · 0s"));
    assert!(!text.contains("Nested agent"));
    assert!(!text.contains("Review files"));
}

#[test]
fn background_box_dims_border_but_not_title_or_output() {
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
    assert_eq!(buf[(2, top)].symbol(), "R");
    assert!(!buf[(2, top)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(2, top + 1)].symbol(), "t");
    assert!(!buf[(2, top + 1)].modifier.contains(Modifier::DIM));
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
    assert_eq!(buffer[(4, 3)].fg, crate::theme::inline_code_color());
    assert!(
        buffer[(14, 3)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert!(
        buffer[(2, 4)]
            .modifier
            .contains(ratatui::style::Modifier::ITALIC)
    );
    assert_eq!(buffer[(6, 9)].fg, crate::theme::link_color());
    assert_eq!(buffer[(0, 11)].fg, Color::Cyan);
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
        kind, body: body.into(), path: None, started_at: now - Duration::seconds(age),
        ended_at: end.map(|age| now - Duration::seconds(age)),
        previous_end: Some(now - Duration::seconds(age + 5)),
    }).collect();
    let mut previous_end = None;
    for event in &mut view.log {
        if event.kind == Kind::Assistant {
            event.previous_end = previous_end;
        }
        previous_end = if matches!(event.kind, Kind::Tool | Kind::Error) {
            event.ended_at
        } else {
            Some(event.started_at)
        };
    }
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

#[test]
fn task_log_labels_are_left_aligned_in_nine_columns() {
    let (view, now) = log_fixture();
    let rendered = log_screen(&view, 100, 25, now);
    assert!(
        rendered
            .lines()
            .nth(22)
            .unwrap()
            .starts_with("  11:58:00 User      Please")
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
    assert_eq!(
        log_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_log_denied.txt")
    );
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
    assert_eq!(
        log_screen(&view, 100, 25, now),
        include_str!("../../tests/ui_snapshots/task_log_finished.txt")
    );
}

#[test]
fn task_log_short_terminal_keeps_ten_log_rows() {
    let mut view = fixture();
    view.rows[view.selected].transcript = Some("/tmp/task-log.jsonl".into());
    let (log, _) = log_fixture();
    view.log = log.log;
    view.log_session = view.selected_id().cloned();
    let layout = view.center_layout(Rect::new(0, 0, 160, 22));
    assert_eq!(layout.log.height, 10);
    assert!(screen(&view, 160, 22).contains("Please inspect"));
}

#[test]
fn task_log_requires_room_for_header_and_six_body_rows() {
    let (mut view, now) = log_fixture();
    let hidden = log_screen(&view, 100, 19, now);
    assert!(!hidden.contains("Please inspect"));
    view.rows[view.selected].transcript = None;
    assert_eq!(hidden, log_screen(&view, 100, 19, now));
}

#[test]
fn task_log_top_rule_matches_header_and_footer_gap_is_empty() {
    use ratatui::style::Modifier;
    let (mut view, now) = log_fixture();
    for loaded in [true, false] {
        if !loaded {
            view.log_session = None;
        }
        let area = Rect::new(0, 0, 100, 25);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        for x in 0..100 {
            assert_eq!(buf[(x, 14)].symbol(), buf[(x, 2)].symbol());
            assert_eq!(buf[(x, 14)].modifier, buf[(x, 2)].modifier);
            assert_eq!(buf[(x, 23)].symbol(), " ");
            assert_eq!(buf[(x, 23)].modifier, Modifier::empty());
        }
        assert!(buf[(2, 14)].modifier.contains(Modifier::DIM));
    }
}

#[test]
fn task_log_last_tool_clock_freezes_for_inactive_and_background_sessions() {
    let (mut view, now) = log_fixture();
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .nth(15)
            .unwrap()
            .starts_with("       35s ")
    );
    assert!(
        log_screen(&view, 100, 25, now + Duration::seconds(1))
            .lines()
            .nth(15)
            .unwrap()
            .starts_with("       36s ")
    );
    view.rows[view.selected].process = None;
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .nth(15)
            .unwrap()
            .starts_with("       25s ")
    );
    view.log.last_mut().unwrap().body.push_str(" *");
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .nth(15)
            .unwrap()
            .starts_with("        5s ")
    );
}

#[test]
fn task_log_last_message_uses_relative_time_and_missing_transcript_hides_panel() {
    let (mut view, now) = log_fixture();
    view.log.pop();
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .nth(15)
            .unwrap()
            .starts_with("   40s ago ")
    );
    view.rows[view.selected].transcript = None;
    let rendered = log_screen(&view, 100, 25, now);
    assert!(!rendered.contains("Please inspect"));
    assert!(!rendered.lines().nth(14).unwrap().contains('─'));
    assert!(rendered.lines().nth(22).unwrap().contains('│'));
}

#[test]
fn task_log_uses_cctail_colours_and_highlights_latest_time() {
    use crate::log::Kind;
    use ratatui::style::Modifier;
    let (mut view, now) = log_fixture();
    view.log.truncate(5);
    let area = Rect::new(0, 0, 100, 25);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    for (y, colour, bold) in [
        (19, Color::Reset, true),
        (18, Color::Yellow, true),
        (17, Color::Blue, true),
        (16, Color::Red, true),
        (15, Color::Reset, true),
    ] {
        let label = &buf[(11, y)];
        let body = &buf[(21, y)];
        assert_eq!(label.fg, colour, "label row {y}");
        assert_eq!(body.fg, colour, "body row {y}");
        assert_eq!(label.modifier.contains(Modifier::BOLD), bold, "row {y}");
        assert!(!label.modifier.contains(Modifier::DIM), "label row {y}");
        assert!(
            !body.modifier.intersects(Modifier::DIM | Modifier::BOLD),
            "body row {y}"
        );
    }
    for y in 16..20 {
        assert_eq!(buf[(9, y)].fg, Color::Reset, "time row {y}");
        assert!(buf[(9, y)].modifier.is_empty(), "time row {y}");
    }
    for (kind, colour) in [
        (Kind::User, Color::Reset),
        (Kind::Assistant, Color::Yellow),
        (Kind::Agent, Color::Yellow),
        (Kind::Session, Color::Yellow),
        (Kind::Tool, Color::Blue),
        (Kind::Error, Color::Red),
        (Kind::Denied, Color::Red),
        (Kind::QueuedUser, Color::Reset),
        (Kind::QueuedSystem, Color::Blue),
    ] {
        let (label_style, body_style) = super::style::log_styles(&kind);
        let foreground = (colour != Color::Reset).then_some(colour);
        assert_eq!(label_style.fg, foreground);
        assert_eq!(body_style.fg, foreground);
        view.log.first_mut().unwrap().kind = kind.clone();
        view.log.first_mut().unwrap().previous_end = None;
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let time = &buf[(9, 19)];
        let time_style = super::style::log_time_style(&kind, false);
        assert_eq!(time_style, Style::default());
        assert_eq!(time.fg, Color::Reset);
        assert!(time.modifier.is_empty());
        let expected_label = match kind {
            Kind::Agent => "Agent",
            Kind::Session => "Session",
            _ => "",
        };
        view.log.last_mut().unwrap().kind = kind;
        if !expected_label.is_empty() {
            assert!(
                log_screen(&view, 100, 25, now)
                    .lines()
                    .nth(15)
                    .unwrap()
                    .starts_with(&format!("    1m ago {expected_label:<9} Also"))
            );
        }
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        for x in [11, 21] {
            let cell = &buf[(x, 15)];
            assert_eq!(cell.fg, colour);
            assert_eq!(cell.modifier.contains(Modifier::BOLD), x == 11);
            assert!(!cell.modifier.contains(Modifier::DIM));
        }
        let time = &buf[(9, 15)];
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
    assert_eq!(buf[(97, 16)].symbol(), "…");
    assert_eq!(buf[(2, 16)].fg, Color::Reset);
    assert!(
        !buf[(2, 16)]
            .modifier
            .intersects(Modifier::DIM | Modifier::BOLD)
    );
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
                .nth(15)
                .unwrap()
                .starts_with("   30s ago ")
        );
    }
}

#[test]
fn task_log_right_aligns_mixed_width_times_and_aligns_labels() {
    use crate::log::Kind;
    let (mut view, now) = log_fixture();
    view.log.truncate(3);
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
            .skip(15)
            .take(3)
            .map(|line| line.chars().skip(2).take(18).collect::<String>())
            .collect::<Vec<_>>(),
        [
            " 44m ago Assistant",
            "12:03:45 Assistant",
            "      3s Tool     "
        ]
    );
    for (line, (time, label)) in rendered.lines().skip(15).take(3).zip([
        ("44m ago", "Assistant"),
        ("12:03:45", "Assistant"),
        ("3s", "Tool"),
    ]) {
        assert_eq!(
            line.find(time).unwrap() + time.len(),
            10,
            "times end at the same column"
        );
        assert_eq!(
            line.find(label),
            Some(11),
            "labels start at the same column"
        );
    }
}

#[test]
fn task_log_shows_only_newest_eight_events_and_hides_without_selection() {
    let (mut view, now) = log_fixture();
    let first = crate::log::Event {
        kind: crate::log::Kind::User,
        body: "Discarded event".into(),
        path: None,
        started_at: now - Duration::seconds(180),
        ended_at: None,
        previous_end: None,
    };
    view.log.insert(0, first);
    assert!(!log_screen(&view, 100, 25, now).contains("Discarded event"));
    view.selected = usize::MAX;
    let rendered = log_screen(&view, 100, 25, now);
    assert!(!rendered.contains("Please inspect"));
    assert!(!rendered.lines().nth(14).unwrap().contains('─'));
}

#[test]
fn center_layout_golden_rects_follow_each_area() {
    let (view, _) = log_fixture();
    for (width, height, top, log_height, list_width, details_width) in [
        (80, 24, 13, 10, 76, 0),
        (120, 40, 25, 14, 70, 43),
        (200, 60, 37, 22, 119, 74),
        (80, 24, 13, 10, 76, 0),
    ] {
        let area = Rect::new(5, 7, width, height);
        let layout = view.center_layout(area);
        assert_eq!(layout.header, Rect::new(5, 7, width, 3));
        assert_eq!(layout.list, Rect::new(7, 10, list_width, top - 3));
        assert_eq!(layout.log, Rect::new(5, 7 + top, width, log_height));
        assert_eq!(layout.footer, Rect::new(5, 7 + height - 1, width, 1));
        if details_width == 0 {
            assert_eq!(layout.details, Rect::default());
            assert_eq!(layout.gap, Rect::default());
        } else {
            assert_eq!(layout.gap, Rect::new(7 + list_width, 10, 3, top - 3));
            assert_eq!(
                layout.details,
                Rect::new(10 + list_width, 10, details_width, top - 3)
            );
        }
    }
}

#[test]
fn center_layout_log_and_details_hide_thresholds() {
    let (mut view, _) = log_fixture();
    for height in 0..20 {
        let layout = view.center_layout(Rect::new(0, 0, 94, height));
        assert_eq!(layout.log.height, 0, "height {height}");
    }
    let layout = view.center_layout(Rect::new(0, 0, 94, 20));
    assert_eq!(layout.log.height, 10);
    assert_eq!(layout.list.height, 6);
    assert!(layout.details.width > 0);
    assert_eq!(
        view.center_layout(Rect::new(0, 0, 93, 20)).details,
        Rect::default()
    );
    view.rows[view.selected].transcript = None;
    for height in [24, 40, 60] {
        let layout = view.center_layout(Rect::new(0, 0, 120, height));
        assert_eq!(layout.log.height, 0);
        assert_eq!(layout.list.height, height - 4);
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
    for (height, count) in [(24, 8), (40, 12), (60, 20), (19, 0), (24, 8)] {
        let rendered = log_screen(&view, 120, height, now);
        let events: Vec<_> = rendered
            .lines()
            .filter(|line| line.contains("event-"))
            .collect();
        assert_eq!(events.len(), count, "height {height}");
        if count > 0 {
            assert!(events[0].contains("event-29"));
            assert!(
                events
                    .last()
                    .unwrap()
                    .contains(&format!("event-{:02}", 30 - count))
            );
            assert!(
                rendered
                    .lines()
                    .nth(usize::from(height - 2))
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

mod details_budget {
    use super::*;
    use crate::ui::render::allocate;

    #[test]
    fn everything_fits() {
        assert_eq!(allocate(&[12, 6, 3], 25), vec![12, 6, 3]);
    }

    #[test]
    fn shares_rows_in_display_order() {
        assert_eq!(allocate(&[12, 6, 3], 8), vec![3, 3, 2]);
        assert_eq!(allocate(&[1, 12, 3], 10), vec![1, 6, 3]);
    }

    #[test]
    fn single_section_uses_budget() {
        assert_eq!(allocate(&[12], 5), vec![5]);
    }

    #[test]
    fn budget_below_floors() {
        for budget in 0..3 {
            let rows = allocate(&[12, 6, 3], budget);
            assert!(rows.iter().all(|&rows| rows <= 1));
            assert_eq!(rows.iter().sum::<usize>(), budget);
        }
        assert_eq!(allocate(&[0, 12, 0], 2), vec![0, 2, 0]);
    }

    #[test]
    fn background_gets_space_before_bodies_and_cuts_whole_boxes() {
        use std::io::Write;
        let mut output = tempfile::NamedTempFile::new().unwrap();
        writeln!(output, "Running checks").unwrap();
        let mut row = session(
            "background-budget",
            "Background task",
            SessionStatus::NeedsInput,
            0,
        );
        row.prompt = "Prompt text\n\n".repeat(12);
        row.queued = vec!["Queued work".into()];
        for i in 0..8 {
            row.background.push(crate::model::BackgroundTask {
                description: format!("Check {i}"),
                output: output.path().into(),
                started_at: row.updated_at,
            });
        }
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        let rows = panel(&view);
        assert!(rows.iter().any(|line| line == "Queued work"));
        assert!(
            rows.iter()
                .any(|line| line.starts_with("─") && line.contains("Check 3"))
        );
        assert!(!rows.iter().any(|line| line.contains("Check 4")));
        assert_eq!(rows.iter().filter(|line| *line == "…").count(), 2);
        assert!(rows[23].starts_with("Model:"));
    }

    #[test]
    fn tiny_panel_keeps_plain_bottom_cut() {
        let mut row = session("tiny", "Tiny task", SessionStatus::NeedsInput, 0);
        row.prompt = "First row\n\nSecond row\n\nThird row".into();
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        let area = Rect::new(0, 0, 120, 6);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
        assert_eq!(buf[(0, 3)].symbol(), "F");
        assert_eq!(buf[(0, 5)].symbol(), "…");
    }

    fn panel(view: &View) -> Vec<String> {
        let area = Rect::new(0, 0, 120, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
        buf.content
            .chunks(120)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn rest_budget_and_wheel_opens_prompt() {
        let mut row = session("budget", "Budget task", SessionStatus::NeedsInput, 0);
        row.prompt = (1..=12)
            .map(|i| format!("Prompt row {i}"))
            .collect::<Vec<_>>()
            .join("  \n");
        row.last_message = Some(
            (1..=6)
                .map(|i| format!("Reply row {i}"))
                .collect::<Vec<_>>()
                .join("  \n"),
        );
        assert_eq!(super::super::markdown::render(&row.prompt, 120).len(), 12);
        assert_eq!(
            super::super::markdown::render(row.last_message.as_ref().unwrap(), 120).len(),
            6
        );
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        let rest = panel(&view);
        for header in ["Prompt", "Last message", "Needs attention"] {
            assert!(rest.iter().any(|line| line == header), "{header}: {rest:?}");
        }
        assert!(!rest[23].is_empty());
        assert_ne!(rest[23], "…");
        let message = rest.iter().position(|line| line == "Last message").unwrap();
        assert!(
            rest[..message]
                .iter()
                .rev()
                .find(|line| !line.is_empty())
                .unwrap()
                .ends_with('…')
        );
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 0));
        assert_eq!(
            view.viewport
                .borrow()
                .details_scroll
                .reveal
                .iter()
                .sum::<usize>(),
            1
        );
        let scrolled = panel(&view);
        assert_eq!(scrolled[0], rest[0]);
        assert_eq!(scrolled[23], rest[23]);
        assert_eq!(scrolled[2], "Prompt row 1");
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 0));
        assert_eq!(panel(&view), rest);
    }
}

mod details_curtain {
    use super::*;

    fn fixture() -> View {
        let mut row = session("curtain", "Curtain task", SessionStatus::NeedsInput, 0);
        row.prompt = (1..=12)
            .map(|i| format!("Prompt row {i}"))
            .collect::<Vec<_>>()
            .join("  \n");
        row.last_message = Some(
            (1..=6)
                .map(|i| format!("Reply row {i}"))
                .collect::<Vec<_>>()
                .join("  \n"),
        );
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        view
    }

    fn panel(view: &View) -> Buffer {
        let area = Rect::new(0, 0, 120, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, view.rows[view.selected].updated_at);
        buf
    }

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn targets_cut_body_or_first_cut_below() {
        use super::super::input::curtain_target;
        let bodies = [2..6, 8..12, 14..17];
        let reveal = [0, 0, 0];
        let hidden = [4, 2, 0];
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 3, true), Some(0));
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 7, true), Some(1));
        assert_eq!(
            curtain_target(&bodies, &[4, 0, 0], &hidden, 3, true),
            Some(1)
        );
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 13, true), None);
        assert_eq!(curtain_target(&[], &[], &[], 0, true), None);
    }

    #[test]
    fn targets_opened_body_then_below_then_above() {
        use super::super::input::curtain_target;
        let bodies = [2..6, 8..12, 14..17];
        let reveal = [1, 0, 2];
        let hidden = [4, 2, 2];
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 3, false), Some(0));
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 9, false), Some(2));
        assert_eq!(
            curtain_target(&bodies, &reveal, &hidden, 18, false),
            Some(2)
        );
        assert_eq!(
            curtain_target(&bodies, &[1, 0, 0], &hidden, 9, false),
            Some(0)
        );
        assert_eq!(curtain_target(&bodies, &[0, 0, 0], &hidden, 3, false), None);
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 1, false), Some(0));
    }

    #[test]
    fn fully_open_prompt_hands_off_to_message() {
        let mut view = fixture();
        panel(&view);
        let hidden = view.viewport.borrow().details_scroll.hidden[0];
        for _ in 0..hidden {
            view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 5));
            panel(&view);
        }
        let opened = panel(&view);
        let message = (0..24)
            .find(|&y| text(&opened, y) == "Last message")
            .unwrap();
        assert!((1..message).all(|y| !text(&opened, y).ends_with('…')));
        assert_eq!(text(&opened, message - 2), "Prompt row 12");
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 5));
        let after = panel(&view);
        assert_eq!(
            view.viewport.borrow().details_scroll.reveal,
            vec![hidden, 1, 0]
        );
        assert_eq!(text(&after, 23), text(&opened, 23));
    }

    #[test]
    fn queued_background_and_hyperlink_cells_stay_fixed() {
        use std::io::Write;
        let mut output = tempfile::NamedTempFile::new().unwrap();
        writeln!(output, "Background output").unwrap();
        let mut view = fixture();
        let row = &mut view.rows[view.selected];
        row.queued = vec!["Queued prompt".into()];
        row.pr = Some(crate::model::PullRequest {
            number: 833,
            url: "https://example.com/pull/833".into(),
        });
        for i in 0..8 {
            row.background.push(crate::model::BackgroundTask {
                description: format!("Check {i}"),
                output: output.path().into(),
                started_at: row.updated_at,
            });
        }
        let before = panel(&view);
        let queued = (0..24)
            .find(|&y| text(&before, y) == "Queued prompt")
            .unwrap();
        assert_eq!(view.viewport.borrow().details_scroll.fallback_max, None);
        assert!(
            before
                .content
                .iter()
                .any(|cell| cell.symbol().contains("https://example.com/pull/833"))
        );
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 3));
        let after = panel(&view);
        for y in queued..24 {
            for x in 0..120 {
                assert_eq!(after[(x, y)], before[(x, y)], "static tail {x},{y}");
            }
        }
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 3));
        assert_eq!(panel(&view), before);
    }

    #[test]
    fn content_updates_clamp_and_body_count_changes_reset() {
        let mut view = fixture();
        panel(&view);
        for _ in 0..3 {
            view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, 5));
            panel(&view);
        }
        view.rows[view.selected].prompt = (1..=8)
            .map(|i| format!("Prompt row {i}"))
            .collect::<Vec<_>>()
            .join("  \n");
        panel(&view);
        assert_eq!(view.viewport.borrow().details_scroll.reveal, vec![2, 0, 0]);
        view.rows[view.selected].last_message = None;
        panel(&view);
        assert_eq!(view.viewport.borrow().details_scroll.reveal, vec![0, 0]);
    }

    #[test]
    fn opening_prompt_pins_header_and_tail_and_closes_exactly() {
        let mut view = fixture();
        let before = panel(&view);
        let pointer = (0..24)
            .find(|&y| text(&before, y) == "Prompt row 3")
            .unwrap();
        let message = (0..24)
            .find(|&y| text(&before, y) == "Last message")
            .unwrap();
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, pointer));
        let after = panel(&view);
        for x in 0..120 {
            assert_eq!(after[(x, 0)], before[(x, 0)], "pinned header {x}");
            for y in 1..pointer {
                assert_eq!(after[(x, y)], before[(x, y + 1)], "shift {x},{y}");
            }
            for y in message..24 {
                assert_eq!(after[(x, y)], before[(x, y)], "tail {x},{y}");
            }
        }
        assert_eq!(text(&after, pointer), "Prompt row 4");
        let cut_before = (0..message)
            .map(|y| text(&before, y))
            .find(|s| s.ends_with('…'))
            .unwrap();
        let cut_after = (0..message)
            .map(|y| text(&after, y))
            .find(|s| s.ends_with('…'))
            .unwrap();
        assert_ne!(cut_before, cut_after);
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, pointer));
        assert_eq!(panel(&view), before);
    }

    #[test]
    fn wheel_below_last_cut_does_nothing() {
        let mut view = fixture();
        let before = panel(&view);
        let project = (0..24).find(|&y| text(&before, y) == "Project").unwrap();
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, project));
        assert_eq!(panel(&view), before);
    }
}

#[test]
fn path_abbreviation_in_details_preserves_last_component() {
    let mut task = session("path", "Path task", SessionStatus::Ready, 0);
    task.cwd = "/Users/visitor/Code/.worktrees/agents-panel".into();
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![task], false);
    let area = Rect::new(0, 0, 26, 20);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    let rows: Vec<String> = buf
        .content
        .chunks(26)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    let project = rows.iter().position(|row| row == "Project").unwrap();
    assert_eq!(rows[project + 1], "/U…/v…/C…/.w…/agents-panel");
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
        .find(|row| row.starts_with('/'))
        .unwrap();
    assert_eq!(heading, "/U…/v…/C…/.w…/agents-panel  1");
}

mod log_path_abbrev {
    use super::*;

    #[test]
    fn read_path_is_abbreviated_to_the_available_body_width() {
        let (mut view, now) = log_fixture();
        let mut event = view.log[2].clone();
        event.body = "Read".into();
        let home = std::env::var_os("HOME").expect("HOME");
        event.path = Some(
            std::path::Path::new(&home)
                .join("Users/workspace/project/main.rs")
                .to_string_lossy()
                .into_owned(),
        );
        view.log = vec![event];
        let area = Rect::new(0, 0, 43, 3);
        let mut buf = Buffer::empty(area);
        view.render_log(area, &mut buf, now);
        let row = buf
            .content
            .chunks(43)
            .nth(1)
            .unwrap()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(row.contains("Tool      Read ~/U…/w…/p…/main.rs"), "{row}");
    }
}

mod log_filters {
    use super::*;
    use crate::log::Kind;

    fn fixture() -> (View, chrono::DateTime<Utc>) {
        let (mut view, now) = log_fixture();
        let template = view.log[0].clone();
        view.log = [
            Kind::User,
            Kind::Assistant,
            Kind::Agent,
            Kind::Session,
            Kind::Tool,
            Kind::Error,
            Kind::Denied,
            Kind::QueuedUser,
            Kind::QueuedSystem,
        ]
        .into_iter()
        .map(|kind| {
            let mut event = template.clone();
            event.body = format!("row-{kind:?}-body");
            event.kind = kind;
            event
        })
        .collect();
        (view, now)
    }

    fn rows(view: &View, now: chrono::DateTime<Utc>) -> String {
        let area = Rect::new(0, 0, 120, 15);
        let mut buffer = Buffer::empty(area);
        view.render_log(area, &mut buffer, now);
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn model_selection_stays_active_and_all_restores_rows() {
        let (mut view, now) = fixture();
        let all = rows(&view, now);
        view.viewport.borrow_mut().log_scroll = 3;
        key(&mut view, KeyCode::Char('m'));
        assert_eq!(view.log_scroll(), 0);
        let filtered = rows(&view, now);
        for event in &view.log {
            assert_eq!(
                filtered.contains(&event.body),
                matches!(event.kind, Kind::Assistant | Kind::Agent)
            );
        }
        view.viewport.borrow_mut().log_scroll = 3;
        key(&mut view, KeyCode::Char('m'));
        assert_eq!(view.log_scroll(), 3);
        view.viewport.borrow_mut().log_scroll = 0;
        assert_eq!(rows(&view, now), filtered);
        view.viewport.borrow_mut().log_scroll = 3;
        key(&mut view, KeyCode::Char('*'));
        assert_eq!(view.log_scroll(), 0);
        assert_eq!(rows(&view, now), all);
        view.viewport.borrow_mut().log_scroll = 3;
        key(&mut view, KeyCode::Char('*'));
        assert_eq!(view.log_scroll(), 3);
    }

    #[test]
    fn kind_selection_replaces_previous_set_and_persists_across_sessions() {
        for (hotkey, kinds) in [
            ('t', vec![Kind::Tool]),
            ('u', vec![Kind::User, Kind::QueuedUser]),
            ('e', vec![Kind::Error, Kind::Denied]),
        ] {
            let (mut view, now) = fixture();
            key(&mut view, KeyCode::Char('m'));
            view.viewport.borrow_mut().log_scroll = 3;
            key(&mut view, KeyCode::Char(hotkey));
            assert_eq!(view.log_scroll(), 0);
            let mut second = view.rows[0].clone();
            second.id = session_key("second");
            let first = view.rows[0].clone();
            view.set_sessions(vec![first, second], false);
            let index = view
                .rows
                .iter()
                .position(|row| row.id == session_key("second"))
                .unwrap();
            view.set_selected(index);
            view.log_session = Some(session_key("second"));
            let filtered = rows(&view, now);
            for event in &view.log {
                assert_eq!(filtered.contains(&event.body), kinds.contains(&event.kind));
            }
        }
    }

    #[test]
    fn overlays_do_not_change_filters() {
        for opener in [
            KeyCode::Char('?'),
            KeyCode::Char('/'),
            KeyCode::Char('r'),
            KeyCode::Backspace,
        ] {
            let (mut view, now) = fixture();
            key(&mut view, opener);
            for hotkey in ['m', 't', 'u', 'e'] {
                key(&mut view, KeyCode::Char(hotkey));
            }
            key(&mut view, KeyCode::Esc);
            let rendered = rows(&view, now);
            assert!(view.log.iter().all(|event| rendered.contains(&event.body)));
        }
    }

    #[test]
    fn help_has_log_column_right_of_view() {
        let mut view = simple_fixture();
        key(&mut view, KeyCode::Char('?'));
        let rendered = screen(&view, 100, 24);
        let headings = rendered
            .lines()
            .find(|line| line.contains("Navigate"))
            .unwrap();
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
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
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
        assert!(text(&panel(&view, now)).contains("Search › "));
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
        for x in 25..32 {
            assert!(buf[(x, 1)].modifier.contains(Modifier::BOLD));
            assert_eq!(buf[(x, 1)].fg, crate::theme::inline_code_color());
        }
        key(&mut view, KeyCode::Enter);
        let buf = panel(&view, now);
        assert!(text(&buf).contains("timeout"));
        assert!(buf[(99, 11)].modifier.contains(Modifier::DIM));
        assert!(!text(&buf).contains("Search › "));
        open(&mut view);
        assert!(text(&panel(&view, now)).contains("Search › time out"));
        key(&mut view, KeyCode::Backspace);
        assert!(text(&panel(&view, now)).contains("Search › time ou"));
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
        key(&mut view, KeyCode::Char('e'));
        open(&mut view);
        for c in "timeout".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        assert_eq!(view.matching_log_count(), 1);
        assert!(!text(&panel(&view, now)).contains("timeout alone"));
        key(&mut view, KeyCode::Esc);
        view.log_filter = crate::log::Filter::default();
        open(&mut view);
        for c in "cafe".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        let buf = panel(&view, now);
        assert!(text(&buf).contains("plain café link"));
        for x in 25..29 {
            assert_eq!(buf[(x, 1)].fg, crate::theme::inline_code_color());
            assert!(buf[(x, 1)].modifier.contains(Modifier::BOLD));
        }
        assert!(buf[(30, 1)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buf[(30, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn selection_change_clears_and_wheel_still_scrolls() {
        let (mut view, now) = log_fixture();
        open(&mut view);
        key(&mut view, KeyCode::Char('a'));
        screen(&view, 120, 40);
        let rect = view.viewport.borrow().log;
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y + 1));
        assert_eq!(view.log_scroll(), 1);
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
        assert!(!text(&buf).contains("Search › "));
        assert_eq!(
            view.center_footer_hints().to_string(),
            super::super::hints::hint_line(&[
                ("?", "help"),
                ("esc", "back"),
                ("↑/↓", "move"),
                ("enter", "open"),
                ("n", "new")
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
        key(&mut view, KeyCode::F(3));
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
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        for c in "你好".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let y = view.center_layout(area).log.bottom() - 1;
        assert_eq!(buf[(11, y)].symbol(), "你");
        assert_eq!(buf[(13, y)].symbol(), "好");
        assert_eq!(view.cursor_pos(area), Some((15, y)));
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
        log.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
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
        let log_row = super::super::log::search_input_rect(
            log.center_layout(area).log.inner(Margin::new(2, 0)),
        );
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
                    let (task, task_row, log, log_row, _, _) =
                        rendered_inputs(width, "你好 query", false);
                    assert_eq!(task_row.x, log_row.x);
                    assert_eq!(task_row.width, log_row.width);
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
                    assert_eq!(
                        log.cursor_pos(area),
                        Some((log_row.x + expected, log_row.y))
                    );
                }
            }
        }
    }
}

#[test]
fn task_log_search_active_snapshot() {
    let (mut view, now) = log_fixture();
    view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
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
            .position(|l| l.contains("› ○ Parent task"))
            .unwrap();
        let lines: Vec<_> = rendered.lines().collect();
        assert!(lines[parent + 2].contains("└ ● Check files"), "{rendered}");
        assert!(lines[parent + 2].contains("Working"));
        assert!(lines[parent + 1].contains("└ ○ Review results"));
        assert!(lines[parent + 1].contains("Ready"));
    }

    #[test]
    fn tabs_filter_each_status_and_refresh_keeps_child_selected() {
        let mut view = fixture();
        for (tab, expected) in [
            (0, vec!["parent", "agent-newer", "agent-older"]),
            (1, vec!["parent", "agent-newer"]),
            (2, vec!["agent-older"]),
            (3, vec![]),
            (4, vec![]),
        ] {
            set_filter(&mut view, tab);
            assert_eq!(ids(&view), expected);
        }
        set_filter(&mut view, 2);
        let mut rows = view.rows.clone();
        rows.iter_mut()
            .find(|s| s.id == session_key("agent-older"))
            .unwrap()
            .status = SessionStatus::Ready;
        rows.iter_mut()
            .find(|s| s.id == session_key("parent"))
            .unwrap()
            .status = SessionStatus::Working;
        view.set_sessions(rows, false);
        assert_eq!(view.status_filter, 1);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        assert_eq!(ids(&view), ["agent-newer", "agent-older"]);
    }

    #[test]
    fn tab_counts_use_each_rows_status() {
        let view = fixture();
        let rendered = screen(&view, 160, 30);
        for label in [
            "Active 3",
            "Ready 2",
            "Working 1",
            "Needs you 0",
            "Inactive 0",
        ] {
            assert!(rendered.contains(label), "{rendered}");
        }
    }

    #[test]
    fn selection_skips_dimmed_parent_headers() {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        let mut other = session("other", "Other task", SessionStatus::Working, 0);
        other.updated_at = rows
            .iter()
            .find(|s| s.id == session_key("parent"))
            .unwrap()
            .updated_at
            - Duration::seconds(1);
        rows.push(other);
        view.set_sessions(rows, false);
        set_filter(&mut view, 2);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        let rendered = screen(&view, 120, 30);
        let (x, y) = rendered_cell(&rendered, "Parent task");
        assert!(
            view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y))
                .is_none()
        );
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        key(&mut view, KeyCode::Down);
        assert_eq!(view.selected_id(), Some(&session_key("other")));
        key(&mut view, KeyCode::Up);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        key(&mut view, KeyCode::Home);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        key(&mut view, KeyCode::PageDown);
        assert_eq!(view.selected_id(), Some(&session_key("other")));
        key(&mut view, KeyCode::PageUp);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        view.render_at(buf.area, &mut buf, "2026-10-13T12:00:00Z".parse().unwrap());
        assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::DIM));
        view.selected = view
            .rows
            .iter()
            .position(|s| s.id == session_key("parent"))
            .unwrap();
        assert_eq!(view.selected_id(), None);
        assert_eq!(key(&mut view, KeyCode::Enter), None);
    }

    #[test]
    fn filtered_snapshots() {
        let mut view = fixture();
        for (tab, expected) in [
            (
                1,
                include_str!("../../tests/ui_snapshots/subagents_ready.txt"),
            ),
            (
                2,
                include_str!("../../tests/ui_snapshots/subagents_working.txt"),
            ),
        ] {
            set_filter(&mut view, tab);
            assert_eq!(screen(&view, 120, 30), expected);
        }
    }

    #[test]
    fn status_grouping_uses_child_status() {
        let mut view = fixture();
        view.grouping = Grouping::Status;
        assert_eq!(ids(&view), ["agent-older", "parent", "agent-newer"]);
        let rendered = screen(&view, 120, 30);
        assert_eq!(
            rendered
                .lines()
                .filter(|line| line.split('│').next().unwrap().contains("Parent task"))
                .count(),
            2
        );
        assert!(rendered.contains("Working  1"));
        assert!(rendered.contains("Ready  2"));
    }

    #[test]
    fn grouping_and_pins_follow_parent() {
        let mut view = fixture();
        for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
            view.grouping = grouping;
            for pins in [vec![], vec![session_key("parent")]] {
                view.set_pinned(&pins);
                let expected = if grouping == Grouping::Status {
                    vec!["agent-older", "parent", "agent-newer"]
                } else {
                    vec!["parent", "agent-newer", "agent-older"]
                };
                assert_eq!(ids(&view), expected);
                let entries = view.center_rows(&view.visible_indices(), grouping);
                assert_eq!(
                    entries
                        .iter()
                        .filter(|r| matches!(r, navigation::CenterRow::Group(_)))
                        .count(),
                    if grouping == Grouping::Status { 2 } else { 1 }
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
            let expected = if grouping == Grouping::Status {
                vec!["agent-older", "other", "parent", "agent-newer"]
            } else {
                vec!["other", "parent", "agent-newer", "agent-older"]
            };
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
    fn filtered_headers_keep_parent_order_group_and_pin() {
        let mut view = fixture();
        let mut rows = view.rows.clone();
        let mut other = session("other", "Other task", SessionStatus::Working, 0);
        other.model = Some("parent-model".into());
        other.updated_at = rows
            .iter()
            .find(|s| s.id == session_key("parent"))
            .unwrap()
            .updated_at
            + Duration::seconds(30);
        rows.push(other);
        view.set_sessions(rows, false);
        for grouping in [Grouping::Project, Grouping::Model] {
            view.grouping = grouping;
            set_filter(&mut view, 2);
            view.set_pinned(&[]);
            assert_eq!(ids(&view), ["other", "agent-older"]);
            let rendered = screen(&view, 120, 30);
            let lines: Vec<_> = rendered.lines().collect();
            let parent = lines
                .iter()
                .position(|line| line.contains("○ Parent task"))
                .unwrap();
            assert!(lines[parent - 1].contains("Other task"));
            assert!(lines[parent + 1].contains("└ ● Check files"));
            let heading = if grouping == Grouping::Project {
                "/tmp/project  2 of 4"
            } else {
                "parent-model  2 of 4"
            };
            assert!(rendered.contains(heading), "{rendered}");
            view.set_pinned(&[session_key("parent")]);
            assert_eq!(ids(&view), ["agent-older", "other"]);
            assert!(screen(&view, 120, 30).contains("Pinned  1 of 3"));
        }
        view.hide(&session_key("agent-older"));
        assert_eq!(ids(&view), ["other"]);
        assert!(!screen(&view, 120, 30).contains("Parent task"));
        let rendered = screen(&view, 160, 30);
        assert!(rendered.contains("Active 3"));
        assert!(rendered.contains("Working 1"));
    }

    #[test]
    fn keyboard_and_mouse_select_child_and_show_details() {
        let mut view = fixture();
        key(&mut view, KeyCode::Down);
        key(&mut view, KeyCode::Down);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        let panel = details_panel(&view);
        let header: String = (0..panel.area.width)
            .map(|x| panel[(x, 0)].symbol())
            .collect();
        assert!(header.starts_with("Working · Check files"), "{header}");
        let rendered = screen(&view, 120, 30);
        assert!(rendered.contains("Model: child-model"));
        assert!(!rendered.contains("Needs attention"));
        let (x, y) = rendered_cell(&rendered, "Review results");
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
        assert_eq!(view.selected_id(), Some(&session_key("agent-newer")));
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, x, y));
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, x, y));
        assert_eq!(view.selected_id(), Some(&session_key("agent-newer")));
        key(&mut view, KeyCode::Down);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
    }

    #[test]
    fn search_child_title_includes_parent_and_only_matching_child() {
        let mut view = fixture();
        key(&mut view, KeyCode::Char('/'));
        for c in "Check files".chars() {
            key(&mut view, KeyCode::Char(c));
        }
        assert_eq!(ids(&view), ["parent", "agent-older"]);
        assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        let rendered = screen(&view, 120, 30);
        assert!(rendered.contains("Parent task"));
        assert!(rendered.contains("└ ● Check files"));
        assert!(
            !rendered.lines().any(|line| line
                .split('│')
                .next()
                .unwrap()
                .contains("Review results"))
        );
        set_filter(&mut view, 2);
        assert_eq!(ids(&view), ["agent-older"]);
        set_filter(&mut view, 1);
        view.search.set_text("Parent task");
        assert_eq!(ids(&view), ["parent", "agent-newer"]);
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
                .find(|&y| buf[(2, y)].symbol() == "└" && buf[(4, y)].symbol() == "●")
                .unwrap();
            assert_eq!(buf[(4, row)].symbol(), "●");
            assert_eq!(buf[(6, row)].symbol(), "界");
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
        assert!(view.delete_dialog.is_none());
        assert!(view.hidden.is_empty());
        assert!(view.pinned.is_empty());
        assert!(!view.searching);
        assert!(!view.help);
        assert_eq!(subagent_rows::screen(view, 120, 30), before);
    }

    macro_rules! ignored_key {
        ($name:ident, $code:expr, $modifiers:expr) => {
            #[test]
            fn $name() {
                for notice in [None, Some("Keep this notice".to_owned())] {
                    let mut view = fixture();
                    view.notice = notice.clone();
                    let before = subagent_rows::screen(&view, 120, 30);
                    assert_eq!(view.handle_key(KeyEvent::new($code, $modifiers)), None);
                    assert_eq!(view.notice, notice);
                    assert_unchanged(&view, &before);
                }
            }
        };
    }

    ignored_key!(control_l, KeyCode::Char('l'), KeyModifiers::CONTROL);
    ignored_key!(rename, KeyCode::Char('r'), KeyModifiers::NONE);
    ignored_key!(fork, KeyCode::Char('f'), KeyModifiers::NONE);
    ignored_key!(stop, KeyCode::Char('x'), KeyModifiers::NONE);
    ignored_key!(delete, KeyCode::Backspace, KeyModifiers::NONE);
    ignored_key!(hide, KeyCode::Char('h'), KeyModifiers::NONE);
    ignored_key!(pin, KeyCode::Char('p'), KeyModifiers::NONE);

    #[test]
    fn resume_emits_unsupported_notice() {
        for code in [KeyCode::Enter, KeyCode::Right, KeyCode::Char('o')] {
            let mut view = fixture();
            assert_eq!(key(&mut view, code), Some(Action::Unsupported("Resume")));
            assert_eq!(view.selected_id(), Some(&session_key("agent-older")));
        }
        let mut view = fixture();
        subagent_rows::screen(&view, 120, 30);
        let (x, y) = view.marker_pos().unwrap();
        let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(view.handle_mouse(click), None);
        assert_eq!(
            view.handle_mouse(click),
            Some(Action::Unsupported("Resume"))
        );
    }

    #[test]
    fn click() {
        let mut view = fixture();
        let before = subagent_rows::screen(&view, 120, 30);
        let (x, y) = view.marker_pos().unwrap();
        assert_eq!(
            view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y)),
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
            view.search.set_text("Check files");
            let before = subagent_rows::screen(&view, 120, 30);
            assert_eq!(view.handle_key(event), Some(Action::Unsupported("Resume")));
            assert!(view.searching);
            assert_eq!(view.search.text(), "Check files");
            assert_eq!(subagent_rows::screen(&view, 120, 30), before);
        }
    }

    #[test]
    fn footer_snapshot() {
        let view = fixture();
        assert_eq!(
            view.center_footer_hints().to_string(),
            "? help  esc back  ↑/↓ move"
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
        (KeyCode::Char('o'), "Resume"),
        (KeyCode::Char('f'), "Fork"),
        (KeyCode::Char('n'), "New session"),
        (KeyCode::Char('r'), "Rename"),
        (KeyCode::Backspace, "Delete"),
    ] {
        assert_eq!(key(&mut view, code), Some(Action::Unsupported(name)));
        assert!(view.rename_target.is_none());
        assert!(view.delete_dialog.is_none());
    }
}

#[test]
fn unsupported_provider_mouse_activation_emits_notice() {
    let mut view = fixture();
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities::default(),
    )]));
    let rendered = screen(&view, 120, 40);
    let second = view.selectable_indices()[1];
    let (x, y) = rendered_cell(&rendered, view.rows[second].display_title());
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    view.handle_mouse(click);
    assert_eq!(
        view.handle_mouse(click),
        Some(Action::Unsupported("Resume"))
    );
}

#[test]
fn missing_provider_capabilities_do_not_allow_actions() {
    let mut view = fixture();
    view.set_capabilities(HashMap::new());
    assert_eq!(
        key(&mut view, KeyCode::Char('o')),
        Some(Action::Unsupported("Resume"))
    );
}

fn test_view(grouping: Grouping, use_colors: bool) -> View {
    let mut view = View::new(grouping, use_colors);
    view.set_capabilities(HashMap::from([(
        ProviderId::Claude,
        Capabilities {
            resume: true,
            fork: true,
            new: true,
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
            codex
                .log(row, 8, &crate::log::Filter::default(), false)
                .unwrap()
        } else {
            crate::providers::claude::log::tail(&path, 8).unwrap()
        };
        let question = events.last().unwrap();
        assert_eq!(style::log_styles(&question.kind).1.fg, Some(Color::Yellow));
        view.log = events;
        let area = Rect::new(0, 0, 160, 40);
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        let question_row = buf
            .content
            .chunks(160)
            .find(|cells| {
                cells[11..20]
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .contains("Assistant")
            })
            .expect("question log row");
        assert_eq!(question_row[21].fg, Color::Yellow);
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
    view.handle_mouse(mouse(MouseEventKind::Down(button), x, y));
}

#[test]
fn task_log_click_opens_cut_event_and_right_click_returns() {
    let (mut view, now) = log_fixture();
    view.log[6].body = "A long response with **bold** words whose body must be cut before the right edge of the log\n\nA second paragraph".into();
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "A long response", MouseButton::Left);
    let open = log_screen(&view, 100, 25, now);
    assert_eq!(
        open,
        include_str!("../../tests/ui_snapshots/task_log_open.txt")
    );
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 25));
    view.render_at(buf.area, &mut buf, now);
    let (x, y) = rendered_cell(&open, "bold");
    assert!(
        buf[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    assert_eq!(view.log_redraw_in(now), None);
    click_log_text(&mut view, &open, "A second paragraph", MouseButton::Left);
    assert_eq!(log_screen(&view, 100, 25, now), open);
    click_log_text(&mut view, &open, "A second paragraph", MouseButton::Right);
    assert_eq!(log_screen(&view, 100, 25, now), list);
}

#[test]
fn task_log_click_on_whole_row_keeps_list() {
    let (mut view, now) = log_fixture();
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "run tests", MouseButton::Left);
    assert_eq!(log_screen(&view, 100, 25, now), list);
    click_log_text(&mut view, &list, "run tests", MouseButton::Right);
    assert_eq!(log_screen(&view, 100, 25, now), list);
}

#[test]
fn task_log_click_opens_row_with_shortened_path() {
    let (mut view, now) = log_fixture();
    let path = "/private/workspace/a-very-long-directory-name/another-long-directory/and-one-more-level/report.md";
    view.log[7].path = Some(path.into());
    let list = log_screen(&view, 100, 25, now);
    assert!(!list.contains(path), "{list}");
    click_log_text(&mut view, &list, "run tests", MouseButton::Left);
    let open = log_screen(&view, 100, 25, now);
    // The open event wraps the full path at the panel edge instead of shortening it.
    assert!(open.contains(&path[..96]), "{open}");
}

#[test]
fn task_log_open_event_closes_when_selection_changes() {
    let (mut view, now) = log_fixture();
    let list = log_screen(&view, 100, 25, now);
    click_log_text(&mut view, &list, "A long response", MouseButton::Left);
    assert!(view.log_open.is_some());
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
    assert!(view.log_open.is_none());
}
