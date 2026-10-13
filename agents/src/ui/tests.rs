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
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 24));
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
fn mouse_details_wheel_shifts_rendered_rows() {
    let mut view = overflowing_details();
    let before = details_panel(&view);
    let rect = view.viewport.borrow().details;
    assert!(!rect.is_empty());
    let selected = view.selected;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    assert_eq!(view.viewport.borrow().details_scroll, 1);
    assert_eq!(view.selected, selected);
    let after = details_panel(&view);
    // The last visible row is reserved for the overflow marker.
    for y in 0..rect.height - 2 {
        for x in 0..rect.width {
            assert_eq!(after[(x, y)], before[(x, y + 1)], "cell {x},{y}");
        }
    }
    assert_ne!(before, after);
}

#[test]
fn mouse_log_wheel_shifts_rendered_rows_and_hides_live_time() {
    let (mut view, now) = log_fixture();
    let mut older = view.log[0].clone();
    older.body = "Older event".into();
    view.log.insert(0, older);
    let before = log_screen(&view, 100, 25, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 25)).log;
    assert!(before.contains("35s Tool    run tests"), "{before}");
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y + 1));
    let after = log_screen(&view, 100, 25, now);
    assert!(after.contains("Older event"), "{after}");
    assert!(!after.contains("run tests"));
    for y in rect.y + 1..rect.bottom() - 3 {
        assert_eq!(
            before.lines().nth(y as usize),
            after.lines().nth(y as usize + 1)
        );
    }
    // A tool at the visible edge must use its recorded duration, even if unfinished.
    view.log[7] = view.log[8].clone();
    view.log[7].body = "Earlier unfinished tool".into();
    let after = log_screen(&view, 100, 25, now);
    assert!(
        after.contains("5s Tool    Earlier unfinished tool"),
        "{after}"
    );
    assert!(!after.contains("35s Tool"));
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x + 2, rect.y + 1));
    assert!(log_screen(&view, 100, 25, now).contains("35s Tool    run tests"));
}

#[test]
fn mouse_log_wheel_keeps_body_column_when_clock_leaves_window() {
    use crate::sessions::log::Kind;
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
    assert!(before.contains("11:59:30 Tool    Event 11"), "{before}");
    assert!(before.contains("12s Tool    Event 10"), "{before}");
    let (before_x, before_y) = rendered_cell(&before, "Event 10");
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y + 1));
    let after = log_screen(&view, 100, 25, now);
    assert!(!after.contains("Event 11"), "{after}");
    let (after_x, after_y) = rendered_cell(&after, "Event 10");
    assert_eq!(after_y, before_y + 1);
    assert_eq!(after_x, before_x);
}

#[test]
fn mouse_log_wheel_down_at_end_stays_at_end() {
    let (mut view, now) = log_fixture();
    let before = log_screen(&view, 100, 25, now);
    let rect = view.center_layout(Rect::new(0, 0, 100, 25)).log;
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x + 2, rect.y + 1));
    assert_eq!(before, log_screen(&view, 100, 25, now));
}

#[test]
fn mouse_log_scroll_tracks_selected_session_identity() {
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
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
    assert_eq!(view.selected_id(), Some(id.as_str()));
    assert_eq!(view.log_scroll(), 1);
    rows.retain(|row| row.id != id);
    for row in &mut rows {
        row.transcript = Some(row.cwd.join("session.jsonl"));
    }
    view.set_sessions(rows, false);
    assert_eq!(view.log_scroll(), 0);
    details_panel(&view);
    let rect = view.viewport.borrow().log;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    assert_eq!(view.log_scroll(), 1);
    key(&mut view, KeyCode::Down);
    assert_eq!(view.log_scroll(), 0);
}

#[test]
fn mouse_details_wheel_clamps_and_clears_end_marker() {
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, rect.x, rect.y));
    assert_eq!(view.viewport.borrow().details_scroll, 0);
    let total = view.viewport.borrow().details_total_rows;
    for _ in 0..total + 2 {
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    }
    assert_eq!(
        view.viewport.borrow().details_scroll,
        total.saturating_sub(usize::from(rect.height))
    );
    let panel = details_panel(&view);
    let last = (0..rect.width)
        .map(|x| panel[(x, rect.height - 1)].symbol())
        .collect::<String>();
    assert!(last.starts_with("Model: "), "{last}");
    assert!(!last.contains('…'));
}

#[test]
fn mouse_details_scroll_resets_with_selection_and_filter() {
    for code in [KeyCode::Down, KeyCode::End, KeyCode::PageDown, KeyCode::Tab] {
        let mut view = overflowing_details();
        details_panel(&view);
        let rect = view.viewport.borrow().details;
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
        assert_eq!(view.viewport.borrow().details_scroll, 1);
        key(&mut view, code);
        assert_eq!(view.viewport.borrow().details_scroll, 0, "{code:?}");
    }
    let mut view = overflowing_details();
    details_panel(&view);
    let rect = view.viewport.borrow().details;
    let id = view.selected_id().unwrap().to_owned();
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, rect.x, rect.y));
    let (_, tab) = view.viewport.borrow().tabs[3];
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), tab.x, tab.y));
    assert_eq!(view.selected_id(), Some(id.as_str()));
    assert_eq!(view.viewport.borrow().details_scroll, 0);
}

#[test]
fn details_scroll_preserves_background_cells_and_hyperlinks() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "Background output").unwrap();
    let mut view = overflowing_details();
    let selected = &mut view.rows[view.selected];
    selected.background.push(crate::model::BackgroundTask {
        waits_for_codex: false,
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
        .find(|y| full[(full_area.x, *y)].symbol() == "╭")
        .unwrap();
    let area = Rect::new(7, 4, 43, 10);
    view.viewport.borrow_mut().details_scroll = usize::from(top + 1 - full_area.y);
    let mut clipped = Buffer::empty(area);
    view.render_details(area, &mut clipped, now);
    assert_eq!(clipped[(area.x, area.y)].symbol(), "│");
    let scroll = view.viewport.borrow().details_scroll as u16;
    for y in area.y..area.bottom() - 1 {
        for x in area.x..area.right() {
            assert_eq!(clipped[(x, y)], full[(x, y + scroll)]);
        }
    }
    view.viewport.borrow_mut().details_scroll = usize::MAX;
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
    assert_eq!(view.selected_id(), Some(id.as_str()));
    assert_eq!(view.viewport.borrow().details_scroll, 1);
    rows.retain(|row| row.id != id);
    view.set_sessions(rows, false);
    assert_eq!(view.viewport.borrow().details_scroll, 0);
}

#[test]
fn mouse_list_click_selects_and_opens_second_task() {
    let mut view = fixture();
    let rendered = screen(&view, 120, 40);
    let second = view.selectable_indices()[1];
    let id = view.rows[second].id.clone();
    let (x, y) = rendered_cell(&rendered, view.rows[second].display_title());
    assert!(matches!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y)),
        Some(Action::Open(opened)) if opened == id
    ));
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
    let mut view = View::default();
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
    assert!(
        matches!(view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y)), Some(Action::Open(id)) if id == view.rows[target].id)
    );
    assert_eq!(view.selected, target);
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
        id: id.into(),
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
        last_message: None,
        last_message_at: None,
        activity: None,
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
    let mut view = View::new(Grouping::Project, false);
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
    view.set_pinned(&["42".into(), "44".into()]);
    view.selected = view.rows.iter().position(|s| s.id == "42").unwrap();
    view
}
fn simple_fixture() -> View {
    let mut view = View::new(Grouping::Project, false);
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
    let mut view = View::default();
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
    assert_eq!(view.selected_id(), Some("42"));
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|s| s.id == "42").unwrap().status = SessionStatus::Working;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Working");
    assert_eq!(view.selected_id(), Some("42"));
}

#[test]
fn refresh_follows_selected_needs_input_session_to_ready_tab() {
    let mut view = fixture();
    for _ in 0..3 {
        key(&mut view, KeyCode::Tab);
    }
    assert_eq!(view.selected_id(), Some("44"));
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|s| s.id == "44").unwrap().status = SessionStatus::Ready;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Ready");
    assert_eq!(view.selected_id(), Some("44"));
}

#[test]
fn refresh_returns_to_active_when_selected_working_session_becomes_inactive() {
    let mut view = fixture();
    for _ in 0..2 {
        key(&mut view, KeyCode::Tab);
    }
    assert_eq!(view.selected_id(), Some("46"));
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|s| s.id == "46").unwrap().status = SessionStatus::Inactive;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some("42"));
    assert_eq!(view.selected, view.visible_indices()[0]);
}

#[test]
fn refresh_returns_to_active_when_selected_working_session_disappears() {
    let mut view = fixture();
    for _ in 0..2 {
        key(&mut view, KeyCode::Tab);
    }
    assert_eq!(view.selected_id(), Some("46"));
    let rows = view.rows.iter().filter(|s| s.id != "46").cloned().collect();

    view.set_sessions(rows, true);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some("42"));
    assert_eq!(view.selected, view.visible_indices()[0]);
}

#[test]
fn refresh_keeps_selected_session_on_active_when_ready_becomes_working() {
    let mut view = fixture();
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|s| s.id == "42").unwrap().status = SessionStatus::Working;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some("42"));
}

#[test]
fn refresh_selects_first_active_session_when_selection_becomes_inactive() {
    let mut view = fixture();
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|s| s.id == "42").unwrap().status = SessionStatus::Inactive;

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Active");
    assert_eq!(view.selected_id(), Some("44"));
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
    assert_eq!(view.selected_id(), Some("42"));
    let mut rows = view.rows.clone();
    let selected = rows.iter_mut().find(|s| s.id == "42").unwrap();
    selected.title = "Other task".into();
    selected.prompt = "Other task".into();

    view.set_sessions(rows, false);

    assert_eq!(TASK_FILTERS[view.status_filter].0, "Ready");
    assert_eq!(view.search, "Current");
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
    let mut view = View::default();
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
    assert_eq!(view.selected_id(), Some("42"));
}

#[test]
fn empty_active_tab_does_not_page_inactive_sessions() {
    let mut view = View::default();
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
        ..View::default()
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
    let mut view = View::new(Grouping::Model, false);
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
        ..View::default()
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
    assert_eq!(view.search, "taskj");
    key(&mut view, KeyCode::Backspace);
    assert_eq!(view.search, "task");
    let id = view.selected_id().unwrap().to_owned();
    view.set_sessions(view.rows.clone(), false);
    assert_eq!(view.selected_id(), Some(id.as_str()));
    assert!(view.searching);
    assert_eq!(view.search, "task");
    key(&mut view, KeyCode::Esc);
    assert!(!view.searching);
    assert_eq!(key(&mut view, KeyCode::Esc), Some(Action::Back));
}
#[test]
fn actions_and_hide_move_to_next_visible_row() {
    let mut view = fixture();
    assert_eq!(
        key(&mut view, KeyCode::Char('o')),
        Some(Action::Resume("42".into()))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('f')),
        Some(Action::Fork("42".into()))
    );
    assert_eq!(
        key(&mut view, KeyCode::Right),
        Some(Action::Open("42".into()))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('x')),
        Some(Action::Unsupported("Stop"))
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin {
            id: "42".into(),
            pinned: false
        })
    );
    assert_eq!(
        key(&mut view, KeyCode::Char('h')),
        Some(Action::Hide("42".into()))
    );
    assert_eq!(view.selected_id(), Some("44"));
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
        ..View::default()
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
    assert_eq!(view.selected_id(), Some("10"));
    key(&mut view, KeyCode::PageUp);
    assert_eq!(view.selected_id(), Some("0"));
    for _ in 0..40 {
        key(&mut view, KeyCode::PageDown);
        screen(&view, 110, 18);
    }
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::PageDown);
    assert_eq!(view.selected_id(), None);
    key(&mut view, KeyCode::Down);
    assert_eq!(view.selected_id(), Some("0"));
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
        status: "waiting".into(),
        kind: None,
        version: Some("2.0".into()),
        started_at: None,
    });
    row.prompt = "prompt ".repeat(40);
    row.last_message = Some("message ".repeat(60));
    let mut view = View::new(Grouping::Project, false);
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
    let title_cell = &buf[(0, 0)];
    assert!(title_cell.modifier.contains(ratatui::style::Modifier::BOLD));
    assert_eq!(title_cell.fg, Color::Reset);
}

#[test]
fn empty_states_notices_and_help_only_handle_close_keys() {
    let mut view = View::default();
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
    let mut view = View::new(Grouping::Project, false);
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
    let mut view = View::new(Grouping::Project, false);
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
            id: "42".into(),
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
        assert_eq!(view.selected_id(), Some("42"));
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
    rows.iter_mut().find(|s| s.id == "42").unwrap().status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some("42"));
    assert!(screen(&view, 88, 16).contains("Rename › Current task"));
    view.hide("42");
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
    let mut view = View::default();
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
        Some(Action::Delete("42".into()))
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
        Some(Action::Delete("42".into()))
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
        assert_eq!(view.selected_id(), Some("42"));
    }
    for c in ['n', 'p', 'j', 'k'] {
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(view.selected_id(), Some("42"));
    }
}

#[test]
fn delete_live_rows_open_the_dialog_and_ignore_show_more() {
    let mut view = fixture();
    view.rows[view.selected].process = Some(crate::model::Process {
        pid: 123,
        status: "idle".into(),
        kind: None,
        version: None,
        started_at: None,
    });
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.delete_dialog.is_some());
    assert!(screen(&view, 120, 24).contains("This stops any running work in this task"));
    assert_eq!(key(&mut view, KeyCode::Down), None);
    assert_eq!(
        key(&mut view, KeyCode::Enter),
        Some(Action::Delete("42".into()))
    );
    view.has_more = true;
    view.selected = usize::MAX;
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(view.delete_dialog.is_none());
}

#[test]
fn live_center_delete_dialog() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    let mut view = View::new(Grouping::Project, false);
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
        Some(Action::Delete("42".into()))
    );
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
    let color = style::thread_color(&row.id);
    let mut view = View::new(Grouping::Project, true);
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
    let mut view = View::new(Grouping::Project, false);
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
        label: "Run checks".into(),
        at: now - Duration::seconds(7),
    });
    row.last_message = Some("Checking the project".into());
    row.last_message_at = Some(now - Duration::hours(1));
    let mut view = View::new(Grouping::Project, false);
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
        label: "Run checks".into(),
        at: now - Duration::minutes(2),
    });
    row.last_message = Some("Checking the project".into());
    row.last_message_at = Some(now - Duration::minutes(5));
    let mut view = View::default();
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
    use ratatui::style::Modifier;
    let now = Utc::now();
    let mut row = session("message", "Message task", SessionStatus::Ready, 0);
    row.activity = Some(crate::model::Activity {
        label: "Run checks".into(),
        at: now - Duration::minutes(5),
    });
    row.last_message = Some("Plain `code` **bold** *italic*\n\nSecond paragraph".into());
    row.last_message_at = Some(now - Duration::minutes(2));
    let mut view = View::default();
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
                Color::Cyan
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
        let mut view = View::default();
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

    #[test]
    fn title_precedes_status() {
        let row = session("header", "Header task", SessionStatus::Working, 0);
        let rendered = queued_details(row, 15);
        assert_eq!(rendered.lines().next().unwrap(), "Header task ● Working");
    }

    #[test]
    fn truncates_title_and_preserves_trailing_status() {
        let mut row = session(
            "header",
            "A very long session title that exceeds the panel width",
            SessionStatus::Working,
            0,
        );
        row.prompt = "Current prompt".into();
        let rendered = queued_details(row, 15);
        let header = rendered.lines().next().unwrap();
        let title = header.strip_suffix(" ● Working").expect("visible status");
        assert!(title.ends_with('…'), "{title}");
        assert_eq!(header, "A very long session title tha… ● Working");
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
    let mut view = View::default();
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
    let total = view.viewport.borrow().details_total_rows;
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
    let mut view = View::default();
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, height);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
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
            waits_for_codex: false,
            description: "Run checks".into(),
            output: output.path().into(),
            started_at: Utc::now(),
        },
        crate::model::BackgroundTask {
            waits_for_codex: false,
            description: "Waiting for output".into(),
            output: output.path().with_extension("missing"),
            started_at: Utc::now(),
        },
    ];
    let rendered = queued_details(row.clone(), 22);
    assert!(rendered.find("Last message").unwrap() < rendered.find("Background\n").unwrap());
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/details_background.txt")
    );
    row.background[0].description = "A very long description ".repeat(5);
    assert_eq!(
        queued_details(row.clone(), 22),
        include_str!("../../tests/ui_snapshots/details_background_long_title.txt")
    );
    let clipped = queued_details(row, 14);
    assert!(clipped.contains("Background\n╭"));
    assert!(clipped.contains("Last message"));
    assert!(!clipped.contains("Project"));
    assert!(clipped.ends_with("\n…"));
}

#[test]
fn background_box_dims_border_but_not_title_or_output() {
    use ratatui::style::Modifier;
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        waits_for_codex: false,
        description: "Run checks".into(),
        output: output.path().into(),
        started_at: Utc::now(),
    });
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 40, 20);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, Utc::now());
    let top = (0..20)
        .find(|y| buf[(0, *y)].symbol() == "╭")
        .expect("box top border");
    assert!(buf[(0, top)].modifier.contains(Modifier::DIM));
    assert!(buf[(30, top)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(2, top)].symbol(), "R");
    assert!(!buf[(2, top)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(2, top + 1)].symbol(), "t");
    assert!(!buf[(2, top + 1)].modifier.contains(Modifier::DIM));
}

#[test]
fn details_background_codex_wait_uses_model_color_only_for_description() {
    use ratatui::style::Modifier;
    use std::io::Write;
    for has_output in [false, true] {
        for waits_for_codex in [false, true] {
            let mut output = tempfile::NamedTempFile::new().unwrap();
            if has_output {
                writeln!(output, "tick").unwrap();
            }
            let mut row = session("background", "Background task", SessionStatus::Ready, 120);
            row.background.push(crate::model::BackgroundTask {
                waits_for_codex,
                description: "Wait for Codex".into(),
                output: output.path().into(),
                started_at: Utc::now(),
            });
            let mut view = View::default();
            view.set_sessions(vec![row], false);
            let area = Rect::new(0, 0, 40, 20);
            let mut buf = Buffer::empty(area);
            view.render_details(area, &mut buf, Utc::now());
            let top = (0..area.height)
                .find(|y| buf[(2, *y)].symbol() == "W")
                .expect("background description");
            let expected = if waits_for_codex {
                Color::Yellow
            } else {
                Color::Reset
            };
            for (offset, character) in "Wait for Codex".chars().enumerate() {
                let cell = &buf[(2 + offset as u16, top)];
                assert_eq!(cell.symbol(), character.to_string());
                assert_eq!(
                    cell.fg, expected,
                    "wait={waits_for_codex}, output={has_output}"
                );
                assert!(!cell.modifier.contains(Modifier::DIM));
            }
            for x in 16..21 {
                let cell = &buf[(x, top)];
                assert_eq!(cell.fg, Color::Reset);
                assert!(cell.modifier.contains(Modifier::DIM));
            }
        }
    }
}

#[test]
fn background_task_whose_output_ended_is_hidden() {
    use std::io::Write;
    let mut output = tempfile::NamedTempFile::new().unwrap();
    writeln!(output, "tick\n\n[exited with code 0]").unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 120);
    row.background.push(crate::model::BackgroundTask {
        waits_for_codex: false,
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
        waits_for_codex: false,
        description: "Running".into(),
        output: "/tmp/unused.output".into(),
        started_at: Utc::now(),
    });
    let mut view = View::default();
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
    let mut row = session("markdown", "Markdown task", SessionStatus::Working, 0);
    row.prompt = "Use `cargo` and **tests**".into();
    row.queued = vec!["- *queued* prompt".into()];
    row.last_message =
        Some("## Result\n\nBuilt [agents](https://example.com).\n\n```\ncargo test\n```".into());
    let mut view = View::default();
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
    assert_eq!(buffer[(4, 3)].fg, Color::Cyan);
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
    assert_eq!(buffer[(6, 9)].fg, Color::Cyan);
    assert_eq!(buffer[(0, 11)].fg, Color::Cyan);
}

fn log_fixture() -> (View, chrono::DateTime<Utc>) {
    use crate::sessions::log::{Event, Kind};
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
        status: "busy".into(),
        kind: None,
        version: None,
        started_at: None,
    });
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![row], false);
    view.log_session = Some("log".into());
    view.log = [
        (Kind::User, "Please\n  inspect the task", 120, None),
        (Kind::Model, "I will check the files", 110, None),
        (Kind::Tool, "Read src/main.rs，Read src/app.rs", 100, Some(90)),
        (Kind::Error, "failed check", 80, Some(70)),
        (Kind::QueuedUser, "Also inspect the tests", 60, None),
        (Kind::Tool, "run a background job *", 50, None),
        (Kind::Model, "A long response whose whitespace collapses and whose body must be truncated before the right aligned timestamp", 40, None),
        (Kind::Tool, "run tests", 30, None),
    ].into_iter().map(|(kind, body, age, end)| Event {
        kind, body: body.into(), started_at: now - Duration::seconds(age),
        ended_at: end.map(|age| now - Duration::seconds(age)),
        previous_end: Some(now - Duration::seconds(age + 5)),
    }).collect();
    let mut previous_end = None;
    for event in &mut view.log {
        if event.kind == Kind::Model {
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
fn task_log_labels_are_left_aligned_in_seven_columns() {
    let (view, now) = log_fixture();
    let rendered = log_screen(&view, 100, 25, now);
    assert!(
        rendered
            .lines()
            .nth(15)
            .unwrap()
            .starts_with("  11:58:00 User    Please")
    );
}

#[test]
fn task_log_denied_snapshot() {
    use crate::sessions::log::{Kind, tail};
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
    view.log_session = view.selected_id().map(str::to_owned);
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
            .nth(22)
            .unwrap()
            .starts_with("       35s ")
    );
    assert!(
        log_screen(&view, 100, 25, now + Duration::seconds(1))
            .lines()
            .nth(22)
            .unwrap()
            .starts_with("       36s ")
    );
    view.rows[view.selected].process = None;
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .nth(22)
            .unwrap()
            .starts_with("       25s ")
    );
    view.log.last_mut().unwrap().body.push_str(" *");
    assert!(
        log_screen(&view, 100, 25, now)
            .lines()
            .nth(22)
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
            .nth(21)
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
    use crate::sessions::log::Kind;
    use ratatui::style::Modifier;
    let (mut view, now) = log_fixture();
    view.log.truncate(5);
    let area = Rect::new(0, 0, 100, 25);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    for (y, colour, bold) in [
        (15, Color::Reset, true),
        (16, Color::Yellow, true),
        (17, Color::Blue, true),
        (18, Color::Red, true),
        (19, Color::Reset, true),
    ] {
        let label = &buf[(11, y)];
        let body = &buf[(19, y)];
        assert_eq!(label.fg, colour, "label row {y}");
        assert_eq!(body.fg, colour, "body row {y}");
        assert_eq!(label.modifier.contains(Modifier::BOLD), bold, "row {y}");
        assert!(!label.modifier.contains(Modifier::DIM), "label row {y}");
        assert!(
            !body.modifier.intersects(Modifier::DIM | Modifier::BOLD),
            "body row {y}"
        );
    }
    for y in 15..19 {
        assert_eq!(buf[(9, y)].fg, Color::Reset, "time row {y}");
        assert!(buf[(9, y)].modifier.is_empty(), "time row {y}");
    }
    for (kind, colour) in [
        (Kind::User, Color::Reset),
        (Kind::Model, Color::Yellow),
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
        let time = &buf[(9, 15)];
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
                    .nth(19)
                    .unwrap()
                    .starts_with(&format!("    1m ago {expected_label:<7} Also"))
            );
        }
        let mut buf = Buffer::empty(area);
        view.render_at(area, &mut buf, now);
        for x in [11, 19] {
            let cell = &buf[(x, 19)];
            assert_eq!(cell.fg, colour);
            assert_eq!(cell.modifier.contains(Modifier::BOLD), x == 11);
            assert!(!cell.modifier.contains(Modifier::DIM));
        }
        let time = &buf[(9, 19)];
        assert_ne!(time.symbol(), " ");
        assert_eq!(time.fg, colour);
        assert!(time.modifier.contains(Modifier::BOLD));
        assert!(!time.modifier.contains(Modifier::DIM));
    }
}

#[test]
fn task_log_preserves_time_when_truncated() {
    use crate::sessions::log::Kind;
    use ratatui::style::Modifier;
    let (view, now) = log_fixture();
    let area = Rect::new(0, 0, 100, 25);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    assert_eq!(buf[(97, 21)].symbol(), "…");
    assert_eq!(buf[(2, 21)].fg, Color::Reset);
    assert!(
        !buf[(2, 21)]
            .modifier
            .intersects(Modifier::DIM | Modifier::BOLD)
    );
    for kind in [
        Kind::User,
        Kind::Model,
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
                .nth(22)
                .unwrap()
                .starts_with("   30s ago ")
        );
    }
}

#[test]
fn task_log_right_aligns_mixed_width_times_and_aligns_labels() {
    use crate::sessions::log::Kind;
    let (mut view, now) = log_fixture();
    view.log.truncate(3);
    view.log[0].kind = Kind::Tool;
    view.log[0].previous_end = Some(now - Duration::seconds(123));
    view.log[0].ended_at = Some(now - Duration::seconds(120));
    view.log[1].started_at = now + Duration::seconds(225);
    view.log[1].previous_end = None;
    view.log[2].kind = Kind::Model;
    view.log[2].started_at = now - Duration::minutes(44);
    let rendered = log_screen(&view, 100, 25, now);
    assert_eq!(
        rendered
            .lines()
            .skip(15)
            .take(3)
            .map(|line| line.chars().skip(2).take(16).collect::<String>())
            .collect::<Vec<_>>(),
        ["      3s Tool   ", "12:03:45 Model  ", " 44m ago Model  "]
    );
    for (line, (time, label)) in rendered.lines().skip(15).take(3).zip([
        ("3s", "Tool"),
        ("12:03:45", "Model"),
        ("44m ago", "Model"),
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
    let first = crate::sessions::log::Event {
        kind: crate::sessions::log::Kind::User,
        body: "Discarded event".into(),
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
            assert!(events[0].contains(&format!("event-{:02}", 30 - count)));
            assert!(events.last().unwrap().contains("event-29"));
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
                waits_for_codex: false,
            });
        }
        let mut view = View::default();
        view.set_sessions(vec![row], false);
        let rows = panel(&view);
        assert!(rows.iter().any(|line| line == "Queued work"));
        assert!(
            rows.iter()
                .any(|line| line.starts_with("╭") && line.contains("Check 2"))
        );
        assert!(!rows.iter().any(|line| line.contains("Check 3")));
        assert_eq!(rows.iter().filter(|line| *line == "…").count(), 1);
        assert!(rows[23].starts_with("Model:"));
    }

    #[test]
    fn tiny_panel_keeps_plain_bottom_cut() {
        let mut row = session("tiny", "Tiny task", SessionStatus::NeedsInput, 0);
        row.prompt = "First row\n\nSecond row\n\nThird row".into();
        let mut view = View::default();
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
    fn rest_budget_and_wheel_natural_stack() {
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
        let mut view = View::default();
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
        assert_eq!(view.viewport.borrow().details_scroll, 1);
        let scrolled = panel(&view);
        assert_eq!(scrolled[23], "…");
        assert_eq!(scrolled[2], "Prompt row 1");
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, 0));
        assert_eq!(panel(&view), rest);
    }
}
