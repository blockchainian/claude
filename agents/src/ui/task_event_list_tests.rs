use super::*;
use crate::providers::Provider;

fn fixture() -> (
    tempfile::TempDir,
    View,
    Vec<crate::event::Event>,
    SessionKey,
    SessionKey,
) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(
        &path,
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl"),
    )
    .unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut sessions = provider.refresh(1).sessions;
    let first = sessions[0].id.clone();
    let mut other = sessions[0].clone();
    other.id.id = "other".into();
    let second = other.id.clone();
    sessions.push(other);
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(sessions, false);
    let mut events =
        crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true).unwrap();
    // The captured payload omits UUIDs; give its events distinct record identities.
    for (index, event) in events.iter_mut().enumerate() {
        event.source = Some(format!("task-event-{index}"));
    }
    assert!(events.len() >= 4);
    assert_ne!(events[1].source, events[2].source);
    show(&mut view, &first, events.clone());
    view.viewport.borrow_mut().event_panel = Rect::new(0, 0, 100, 3);
    (dir, view, events, first, second)
}

fn show(view: &mut View, id: &SessionKey, events: Vec<crate::event::Event>) {
    let row = view.rows.iter().position(|row| &row.id == id).unwrap();
    view.select(row);
    assert_eq!(view.selected_id(), Some(id));
    view.event_session = Some(id.clone());
    view.apply_event_update(crate::event::Update {
        events,
        reset: true,
        ..Default::default()
    });
}

#[test]
fn task_event_list_restores_selection_position_and_unseen() {
    let (_dir, mut view, events, first, second) = fixture();
    view.select_event(Some(2));
    view.event_list.scroll.set(1);
    view.event_list.unseen.set(2);
    let selected = view.visible_event().unwrap().source.clone();
    show(&mut view, &second, events.clone());
    show(&mut view, &first, events);
    assert_eq!(view.visible_event().unwrap().source, selected);
    assert_eq!(view.event_scroll(), 1);
    assert_eq!(view.event_list.unseen.get(), 2);
}

#[test]
fn task_event_list_restores_identity_after_history_changes() {
    let (_dir, mut view, events, first, second) = fixture();
    show(&mut view, &first, events[..events.len() - 2].to_vec());
    view.select_event(Some(2));
    view.event_list.scroll.set(1);
    let selected = view.visible_event().unwrap().source.clone();
    let anchor = events[1].source.clone();
    show(&mut view, &second, events.clone());
    show(&mut view, &first, events[1..].to_vec());
    assert_eq!(view.visible_event().unwrap().source, selected);
    assert_eq!(
        view.shown_events()
            .nth(view.event_scroll())
            .unwrap()
            .1
            .source,
        anchor
    );
    assert_eq!(view.event_list.unseen.get(), 2);
}

#[test]
fn task_event_list_missing_anchor_resumes_live() {
    let (_dir, mut view, events, first, second) = fixture();
    view.select_event(Some(2));
    view.event_list.scroll.set(1);
    show(&mut view, &second, events.clone());
    show(&mut view, &first, events[2..].to_vec());
    assert!(view.following_event());
    assert_eq!(view.event_scroll(), view.event_max_scroll());
}

#[test]
fn task_event_list_sort_is_per_task() {
    let (_dir, mut view, events, first, second) = fixture();
    view.set_event_sort(EventSort::Duration);
    show(&mut view, &second, events.clone());
    assert_eq!(view.event_list.sort, EventSort::Time);
    show(&mut view, &first, events);
    assert_eq!(view.event_list.sort, EventSort::Duration);
}

#[test]
fn task_event_list_source_less_positions_resume_live() {
    for missing in [1, 2] {
        let (_dir, mut view, events, first, second) = fixture();
        view.select_event(Some(2));
        view.event_list.scroll.set(1);
        view.event_panel[missing].source = None;
        show(&mut view, &second, events.clone());
        show(&mut view, &first, events);
        assert!(view.following_event());
        assert_eq!(view.event_scroll(), view.event_max_scroll());
        assert_eq!(view.event_list.unseen.get(), 0);
    }
}

#[test]
fn task_event_list_live_follows_events_added_while_away() {
    let (_dir, mut view, events, first, second) = fixture();
    view.show_newest_event();
    show(&mut view, &second, events.clone());
    let mut appended = events.clone();
    let mut event = appended.last().unwrap().clone();
    event.source = Some("appended-event".into());
    appended.push(event);
    show(&mut view, &first, appended);
    assert_eq!(
        view.visible_event().unwrap().source.as_deref(),
        Some("appended-event")
    );
    assert!(view.following_event());
    assert_eq!(view.event_scroll(), view.event_max_scroll());
}

#[test]
fn task_event_list_keeps_parked_state_while_history_is_pending() {
    let (_dir, mut view, events, first, second) = fixture();
    view.select_event(Some(2));
    view.event_list.scroll.set(1);
    view.event_list.unseen.set(2);
    let selected = view.visible_event().unwrap().source.clone();
    show(&mut view, &second, events.clone());
    let index = view.rows.iter().position(|row| row.id == first).unwrap();
    view.select(index);
    assert!(view.restoring_event_position());
    let area = Rect::new(0, 0, 100, 1);
    view.render_event_jump(area, &mut Buffer::empty(area));
    assert_eq!(view.event_list.unseen.get(), 2);
    // Switching away before App finishes its history read must not park another task's rows.
    show(&mut view, &second, events.clone());
    show(&mut view, &first, events);
    assert_eq!(view.visible_event().unwrap().source, selected);
    assert_eq!(view.event_scroll(), 1);
    assert_eq!(view.event_list.unseen.get(), 2);
}

#[test]
fn event_pin_toggle_keeps_snapshot_and_timeline() {
    let (_dir, mut view, events, first, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    let action = view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    assert!(
        matches!(action, Some(Action::ToggleEventPin { task, pinned }) if task == first && pinned.len() == 1)
    );
    assert_eq!(view.event_list.pinned.len(), 1);
    assert_eq!(view.event_panel, events);
    assert!(view.event_list.pinned[0].event.turn.is_none());
    assert!(view.event_list.pinned[0].event.previous_end.is_none());
    view.select_event(Some(0));
    view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    assert!(view.event_list.pinned.is_empty());
}

#[test]
fn event_pin_cursor_survives_task_switch() {
    let (_dir, mut view, events, first, second) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    view.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    view.select_event(Some(0));
    show(&mut view, &second, events.clone());
    show(&mut view, &first, events);
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
}

#[test]
fn event_pin_order_and_overlapping_records() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    view.toggle_event_pin();
    let first = view.event_list.pinned[0].pin.clone();
    view.select_event(view.shown_position(2));
    view.toggle_event_pin();
    assert_eq!(view.event_list.pinned[1].pin, first);
    let second = view.event_list.pinned[0].pin.clone();
    view.event_panel[1].source_records.extend(second.records);
    view.select_event(view.shown_position(1));
    view.toggle_event_pin();
    assert!(view.event_list.pinned.is_empty());
}

#[test]
fn event_pin_repin_moves_row_above_more_recent_pin() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(view.shown_position(1));
    view.toggle_event_pin();
    let first = view.event_list.pinned[0].pin.clone();
    view.select_event(view.shown_position(2));
    view.toggle_event_pin();
    let second = view.event_list.pinned[0].pin.clone();
    assert_eq!(view.event_list.pinned[1].pin, first);

    view.select_event(view.shown_position(1));
    view.toggle_event_pin();
    assert_eq!(view.event_list.pinned.len(), 1);
    assert_eq!(view.event_list.pinned[0].pin, second);
    view.select_event(view.shown_position(1));
    view.toggle_event_pin();
    assert_eq!(view.event_list.pinned.len(), 2);
    assert_eq!(view.event_list.pinned[0].pin, first);
    assert_eq!(view.event_list.pinned[1].pin, second);
}

#[test]
fn event_pin_running_tool_keeps_snapshot_until_repinned() {
    use std::io::Write;

    let (dir, mut view, _, first, _) = fixture();
    let path = dir.path().join("running-tool.jsonl");
    let mut records = include_str!("../../tests/fixtures/claude-tool-calls.jsonl").lines();
    let request = records.next().unwrap();
    let result = records.next().unwrap();
    std::fs::write(&path, format!("{request}\n")).unwrap();
    let running =
        crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true).unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].calls.len(), 1);
    assert!(running[0].calls[0].output.is_none());
    show(&mut view, &first, running);
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(view.shown_position(0));
    view.toggle_event_pin();
    assert_eq!(view.event_list.pinned.len(), 1);
    let snapshot = view.event_list.pinned[0].event.clone();

    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap(),
        "{result}"
    )
    .unwrap();
    let completed =
        crate::providers::claude::event_reader::tail_session(&path, usize::MAX, true).unwrap();
    assert_eq!(completed.len(), 1);
    assert!(
        completed[0].calls[0]
            .output
            .as_ref()
            .is_some_and(|output| !output.is_empty())
    );
    assert_eq!(completed[0].source, snapshot.source);
    view.apply_event_update(crate::event::Update {
        events: completed.clone(),
        reset: true,
        ..Default::default()
    });
    assert_eq!(view.event_panel, completed);
    assert_eq!(view.event_list.pinned[0].event, snapshot);

    view.select_event(view.shown_position(0));
    view.toggle_event_pin();
    assert!(view.event_list.pinned.is_empty());
    view.select_event(view.shown_position(0));
    view.toggle_event_pin();
    let mut expected = completed[0].clone();
    expected.turn = None;
    expected.previous_end = None;
    assert_eq!(view.event_list.pinned[0].event, expected);
    assert_eq!(
        view.event_list.pinned[0].pin.records,
        completed[0].source_records
    );
}

#[test]
fn event_pin_rejects_missing_identity_queued_and_inactive() {
    let (_dir, mut view, events, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    for invalid in 0..5 {
        view.event_panel = events.clone();
        match invalid {
            0 => view.event_panel[1].source = None,
            1 => view.event_panel[1].source = Some(String::new()),
            2 => view.event_panel[1].source_records.clear(),
            3 => view.event_panel[1].kind = crate::event::Kind::QueuedUser,
            _ => view.event_panel[1].kind = crate::event::Kind::QueuedSystem,
        }
        view.prepare_event_search();
        view.select_event(view.shown_position(1));
        assert!(matches!(
            view.toggle_event_pin(),
            Some(Action::Unsupported("Pin"))
        ));
    }
    view.rows[view.selected].status = SessionStatus::Inactive;
    view.status_filter = 1;
    assert!(matches!(
        view.toggle_event_pin(),
        Some(Action::Unsupported("Pin"))
    ));
    assert!(view.event_list.pinned.is_empty());
}

#[test]
fn event_pin_snapshot_survives_update_and_clear_parked() {
    let (_dir, mut view, mut events, first, second) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    view.toggle_event_pin();
    let snapshot = view.event_list.pinned[0].event.clone();
    view.select_event(Some(0));
    events[1].body.push_str(" updated");
    view.apply_event_update(crate::event::Update {
        events: events.clone(),
        reset: true,
        ..Default::default()
    });
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    assert_eq!(view.event_list.pinned[0].event, snapshot);
    view.set_event_sort(EventSort::Duration);
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    show(&mut view, &second, events.clone());
    assert!(view.has_pinned_events(&first));
    view.clear_pinned_events(&first);
    show(&mut view, &first, events);
    assert!(!view.has_pinned_events(&first));
}

#[test]
fn event_pin_navigation_and_mouse_use_distinct_rows() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    view.toggle_event_pin();
    view.select_event(Some(0));
    view.move_event_selection(true);
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(0)));
    view.move_event_selection(false);
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    let area = Rect::new(0, 1, 100, 4);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 6));
    view.render_event(area, &mut buffer, Utc::now());
    assert_eq!(view.viewport.borrow().event_rows[0], EventRow::Pinned(0));
    view.select_event(Some(1));
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 3,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    assert_eq!(
        view.event_read_count(4, true),
        view.event_panel.len().max(4)
    );
}

#[test]
fn c2_unpin_selects_nearest_pin_then_same_timeline_event() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    for index in [1, 2] {
        view.select_event(view.shown_position(index));
        view.toggle_event_pin();
    }
    view.select_event(Some(1));
    view.toggle_event_pin();
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    view.toggle_event_pin();
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(2)));
}

#[test]
fn event_pin_fixed_above_scrolled_timeline() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.select_event(Some(1));
    view.toggle_event_pin();
    view.select_event(None);
    let area = Rect::new(0, 0, 100, 5);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 101, 5));
    view.render_event(area, &mut buffer, Utc::now());
    assert_eq!(view.viewport.borrow().event_rows[0], EventRow::Pinned(0));
    assert_eq!(buffer[(0, 1)].symbol(), "─");
    assert_eq!(view.event_scroll(), view.shown_events().count() - 3);
    let row_text = |y| {
        (0..100)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    assert!(!row_text(0).contains('│'));
    assert_eq!(buffer[(100, 0)].symbol(), " ");
    assert_eq!(
        view.viewport.borrow().event_rows.last(),
        Some(&EventRow::Timeline(view.event_panel.len() - 1))
    );
}

#[test]
fn event_pin_clock_ends_at_same_display_column_as_timeline() {
    let (_dir, mut view, events, _, _) = fixture();
    for row in &mut view.rows { row.status = SessionStatus::Ready; }
    view.status_filter = 0;
    view.select_event(Some(0));
    view.toggle_event_pin();
    view.select_event(Some(0));
    let area = Rect::new(0, 0, 100, 30);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 101, 30));
    view.render_event(area, &mut buffer, events[0].started_at);
    let line = |y| {
        let mut text = String::new();
        let mut x = 0;
        while x < area.width {
            let symbol = buffer[(x, y)].symbol();
            text.push_str(symbol);
            x += (symbol.width() as u16).max(1);
        }
        text.trim_end().to_owned()
    };
    let pinned = line(0);
    let timeline = line(2);
    let clock = super::formatting::clock_text(events[0].started_at);
    assert!(pinned.ends_with(&clock), "{pinned}");
    assert!(timeline.ends_with(&clock), "{timeline}");
    assert!(pinned.chars().count() < pinned.width(), "fixture must contain wide characters");
    assert_eq!(pinned.width(), timeline.width());
    assert!(!pinned.contains('│'));
    assert_eq!(buffer[(100, 0)].symbol(), " ");
}
