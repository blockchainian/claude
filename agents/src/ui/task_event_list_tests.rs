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
    view.select_event(view.shown_row_position(EventRow::Pinned(0)));
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
    view.select_event(view.shown_row_position(EventRow::Pinned(0)));
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
    assert_eq!(view.event_list.pinned[0].pin, first);
    let second = view.event_list.pinned[1].pin.clone();
    view.event_panel[1].source_records.extend(second.records);
    view.select_event(view.shown_position(1));
    view.toggle_event_pin();
    assert!(view.event_list.pinned.is_empty());
}

#[test]
fn event_pin_repin_appends_after_remaining_pins() {
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
    let second = view.event_list.pinned[1].pin.clone();
    assert_eq!(view.event_list.pinned[0].pin, first);

    view.select_event(view.shown_position(1));
    view.toggle_event_pin();
    assert_eq!(view.event_list.pinned.len(), 1);
    assert_eq!(view.event_list.pinned[0].pin, second);
    view.select_event(view.shown_position(1));
    view.toggle_event_pin();
    assert_eq!(view.event_list.pinned.len(), 2);
    assert_eq!(view.event_list.pinned[0].pin, second);
    assert_eq!(view.event_list.pinned[1].pin, first);
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
fn event_pin_rejects_missing_identity_and_queued_but_allows_inactive() {
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
    assert!(view.event_list.pinned.is_empty());
    view.event_panel = events;
    view.prepare_event_search();
    view.rows[view.selected].status = SessionStatus::Inactive;
    view.status_filter = 1;
    view.select_event(view.shown_position(1));
    assert!(matches!(
        view.toggle_event_pin(),
        Some(Action::ToggleEventPin { .. })
    ));
    assert_eq!(view.event_list.pinned.len(), 1);
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
    view.select_event(view.shown_row_position(EventRow::Pinned(0)));
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
    view.set_pinned_events(&first, Vec::new());
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
    view.select_event(view.shown_row_position(EventRow::Pinned(0)));
    view.move_event_selection(true);
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    view.navigate_event(KeyCode::Home);
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(0)));
    view.move_event_selection(false);
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(0)));
    view.navigate_event(KeyCode::End);
    assert_eq!(view.event_list.cursor, view.newest_event_position());
    let area = Rect::new(0, 1, 100, 4);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 6));
    view.render_event(area, &mut buffer, Utc::now());
    assert_eq!(
        view.viewport.borrow().event_rows.last().copied().unwrap(),
        EventRow::Pinned(0)
    );
    view.select_event(Some(1));
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    view.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 3,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    view.event_list.has_older = true;
    assert_eq!(view.event_list_read_count(4), view.event_panel.len().max(4));
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
    view.select_event(view.shown_row_position(EventRow::Pinned(1)));
    view.toggle_event_pin();
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    view.toggle_event_pin();
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(1)));
}

#[test]
fn event_pin_fixed_below_scrolled_timeline() {
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
    assert_eq!(
        view.viewport.borrow().event_rows.last().copied().unwrap(),
        EventRow::Pinned(0)
    );
    assert_eq!(buffer[(50, 3)].symbol(), " ");
    assert_eq!(buffer[(0, 3)].symbol(), " ");
    assert_eq!(view.event_scroll(), view.shown_events().count() - 3);
    let row_text = |y| {
        (0..100)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    assert!(!row_text(4).contains('│'));
    assert_eq!(buffer[(100, 4)].symbol(), " ");
    assert_eq!(
        view.viewport.borrow().event_rows.get(2),
        Some(&EventRow::Timeline(view.event_panel.len() - 1))
    );
}

#[test]
fn event_pin_clock_ends_at_same_display_column_as_timeline() {
    let (_dir, mut view, events, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.select_event(view.shown_position(0));
    view.toggle_event_pin();
    view.select_event(view.shown_position(0));
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
    let pinned = line(29);
    let timeline = line(0);
    let clock = super::formatting::clock_text(events[0].started_at);
    assert!(pinned.ends_with(&clock), "{pinned}");
    assert!(timeline.ends_with(&clock), "{timeline}");
    assert!(
        pinned.chars().count() < pinned.width(),
        "fixture must contain wide characters"
    );
    assert_eq!(pinned.width(), timeline.width());
    assert!(!pinned.contains('│'));
    assert_eq!(buffer[(100, 29)].symbol(), " ");
}

#[test]
fn pinned_sections_navigation_clamps_at_list_ends() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    view.toggle_event_pin();
    view.select_event(Some(1));
    view.navigate_event(KeyCode::Up);
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(0)));
    view.select_event(view.shown_row_position(EventRow::Pinned(0)));
    view.navigate_event(KeyCode::Down);
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    for key in [KeyCode::PageDown, KeyCode::End] {
        view.select_event(view.shown_row_position(EventRow::Pinned(0)));
        view.navigate_event(key);
        assert_eq!(view.event_list.cursor, view.newest_event_position());
    }
}

#[test]
fn pinned_sections_render_blank_separator_without_header() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.select_event(Some(1));
    view.toggle_event_pin();
    view.select_event(None);
    let area = Rect::new(0, 0, 100, 5);
    let mut buffer = Buffer::empty(area);
    view.render_event(area, &mut buffer, Utc::now());
    let text = |y| {
        (0..100)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    assert!((0..5).all(|y| !text(y).contains("Pinned")));
    assert_eq!(text(3).trim(), "");
    assert_eq!(view.viewport.borrow().event_row_at(3), None);
    assert_eq!(
        view.viewport.borrow().event_row_at(4),
        Some(EventRow::Pinned(0))
    );
    assert!(matches!(
        view.viewport.borrow().event_row_at(2),
        Some(EventRow::Timeline(_))
    ));
    assert_eq!(view.event_timeline_height(), 3);
}

#[test]
fn pinned_sections_separator_ignores_mouse_clicks() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    view.toggle_event_pin();
    for width in [59, 60] {
        let area = Rect::new(11, 3, width, 5);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 12));
        view.render_event(area, &mut buffer, Utc::now());
        let center = area.x + area.width / 2;
        let y = area.y + 3;
        assert_eq!(buffer[(center, y)].symbol(), " ");
        assert_eq!(
            buffer[(center, y)].style(),
            ratatui::buffer::Cell::default().style()
        );
        for selected in [Some(0), Some(2), None] {
            view.select_event(selected);
            let before = view.selected_event_row();
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                view.handle_mouse(MouseEvent {
                    kind,
                    column: center,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                });
            }
            assert_eq!(view.selected_event_row(), before);
        }
        assert_eq!(view.event_timeline_height(), 3);
    }
}

#[test]
fn event_list_backtick_preserves_selected_row() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.active_panel = Panel::EventList;
    view.select_event(Some(1));
    view.toggle_event_pin();
    for selected in [EventRow::Timeline(3), EventRow::Pinned(0)] {
        let position = view.shown_row_position(selected).unwrap();
        view.select_event(Some(position));
        let source = view.visible_event().unwrap().source.clone();
        assert!(
            view.handle_key(KeyEvent::new(KeyCode::Char('`'), KeyModifiers::NONE))
                .is_none()
        );
        assert_eq!(view.selected_event_row(), Some(selected));
        assert_eq!(view.visible_event().unwrap().source, source);
    }
}

#[test]
fn pinned_sections_scroll_many_pins_without_moving_timeline() {
    let (_dir, mut view, _, _, _) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    for index in 0..4 {
        view.select_event(view.shown_position(index));
        view.toggle_event_pin();
    }
    let area = Rect::new(0, 0, 100, 5);
    let mut buffer = Buffer::empty(area);
    view.select_event(None);
    view.render_event(area, &mut buffer, Utc::now());
    let timeline_scroll = view.event_scroll();
    for _ in 0..4 {
        view.navigate_event(KeyCode::Down);
    }
    view.render_event(area, &mut buffer, Utc::now());
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(3)));
    assert_eq!(view.event_scroll(), timeline_scroll);
    assert!(
        view.viewport
            .borrow()
            .event_rows
            .contains(&EventRow::Pinned(3))
    );
    let pinned_scroll = view.event_list.pinned_scroll.get();
    view.navigate_event(KeyCode::Home);
    view.render_event(area, &mut buffer, Utc::now());
    assert_eq!(view.event_list.pinned_scroll.get(), pinned_scroll);
    assert_eq!(view.event_scroll(), 0);
    assert_eq!(view.viewport.borrow().event_row_at(1), None);
}

#[test]
fn pinned_sections_absent_without_matching_pins() {
    let (_dir, view, _, _, _) = fixture();
    let area = Rect::new(0, 0, 100, 5);
    let mut buffer = Buffer::empty(area);
    view.render_event(area, &mut buffer, Utc::now());
    assert!(view.viewport.borrow().event_separator.is_none());
    assert!(matches!(
        view.viewport.borrow().event_row_at(0),
        Some(EventRow::Timeline(_))
    ));
    assert_eq!(view.event_timeline_height(), 5);
}

#[test]
fn pinned_sections_wheel_moves_current_selection_at_every_hover_position() {
    for height in [4, 8] {
        for sort in [EventSort::Time, EventSort::Duration] {
            let (_dir, mut view, _, _, _) = fixture();
            for row in &mut view.rows {
                row.status = SessionStatus::Ready;
            }
            view.status_filter = 0;
            for index in [0, 1] {
                view.select_event(view.shown_position(index));
                view.toggle_event_pin();
            }
            view.set_event_sort(sort);
            let area = Rect::new(11, 3, 80, height);
            let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 14));
            for row in area.y..area.bottom() {
                view.select_event(None);
                view.render_event(area, &mut buffer, Utc::now());
                for (kind, expected) in [
                    (MouseEventKind::ScrollDown, EventRow::Pinned(0)),
                    (MouseEventKind::ScrollDown, EventRow::Pinned(1)),
                    (MouseEventKind::ScrollUp, EventRow::Pinned(0)),
                ] {
                    view.handle_mouse(MouseEvent {
                        kind,
                        column: area.x + 5,
                        row,
                        modifiers: KeyModifiers::NONE,
                    });
                    assert_eq!(view.selected_event_row(), Some(expected));
                    view.render_event(area, &mut buffer, Utc::now());
                }
                view.handle_mouse(MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column: area.x + 5,
                    row,
                    modifiers: KeyModifiers::NONE,
                });
                assert_eq!(view.event_list.cursor, view.newest_event_position());
            }
        }
    }
}

#[test]
fn pinned_sections_timeline_restore_and_live_follow_ignore_pin_indices() {
    let (_dir, mut view, events, first, second) = fixture();
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.select_event(Some(1));
    view.toggle_event_pin();
    view.select_event(Some(2));
    view.event_list.scroll.set(1);
    show(&mut view, &second, events.clone());
    show(&mut view, &first, events.clone());
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(2)));
    assert_eq!(view.event_scroll(), 1);
    view.show_newest_event();
    assert_eq!(
        view.selected_event_row(),
        Some(EventRow::Timeline(events.len() - 1))
    );
    assert!(view.following_event());
    show(&mut view, &second, events.clone());
    let mut appended = events.clone();
    let mut event = events.last().unwrap().clone();
    event.source = Some("pinned-follow-arrival".into());
    appended.push(event);
    show(&mut view, &first, appended);
    assert_eq!(
        view.selected_event_row(),
        Some(EventRow::Timeline(events.len()))
    );
    assert!(view.following_event());
    view.navigate_event(KeyCode::Down);
    assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
    assert!(!view.following_event());
    view.navigate_event(KeyCode::Up);
    assert_eq!(view.event_list.cursor, view.newest_event_position());
    assert!(view.following_event());
    view.navigate_event(KeyCode::Down);
    view.navigate_event(KeyCode::End);
    assert_eq!(view.event_list.cursor, view.newest_event_position());
    assert!(view.following_event());
    view.navigate_event(KeyCode::Home);
    assert_eq!(view.selected_event_row(), Some(EventRow::Timeline(0)));
}

#[test]
fn pinned_sections_short_timeline_keeps_blank_rows_out_of_hit_testing() {
    let (_dir, mut view, events, first, _) = fixture();
    show(&mut view, &first, events[..1].to_vec());
    for row in &mut view.rows {
        row.status = SessionStatus::Ready;
    }
    view.status_filter = 0;
    view.select_event(Some(0));
    view.toggle_event_pin();
    let area = Rect::new(0, 0, 100, 8);
    view.render_event(area, &mut Buffer::empty(area), Utc::now());
    let viewport = view.viewport.borrow();
    assert_eq!(
        viewport.event_rows,
        [EventRow::Timeline(0), EventRow::Pinned(0)]
    );
    assert_eq!(viewport.event_row_at(0), Some(EventRow::Timeline(0)));
    for offset in 1..7 {
        assert_eq!(viewport.event_row_at(offset), None, "row {offset}");
    }
    assert_eq!(viewport.event_row_at(7), Some(EventRow::Pinned(0)));
}

#[test]
fn pinned_navigation_crosses_sections_by_line_and_pages_within_timeline() {
    for sort in [EventSort::Time, EventSort::Duration] {
        let (_dir, mut view, _, _, _) = fixture();
        for row in &mut view.rows {
            row.status = SessionStatus::Ready;
        }
        view.status_filter = 0;
        for index in [0, 1, 2] {
            view.select_event(view.shown_position(index));
            view.toggle_event_pin();
        }
        view.set_event_sort(sort);
        let area = Rect::new(0, 0, 100, 7);
        view.render_event(area, &mut Buffer::empty(area), Utc::now());
        view.select_event(None);
        let last = view.newest_event_position().unwrap();
        view.navigate_event(KeyCode::Down);
        assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
        view.navigate_event(KeyCode::Up);
        assert_eq!(view.event_list.cursor, Some(last));
        view.navigate_event(KeyCode::PageDown);
        assert_eq!(view.event_list.cursor, Some(last));
        view.navigate_event(KeyCode::Down);
        view.navigate_event(KeyCode::Down);
        assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(1)));
        view.navigate_event(KeyCode::PageUp);
        assert_eq!(
            view.event_list.cursor,
            Some(last.saturating_sub(view.event_timeline_height().max(1)))
        );
        for key in [KeyCode::PageDown, KeyCode::End] {
            view.select_event(view.shown_row_position(EventRow::Pinned(2)));
            view.navigate_event(key);
            assert_eq!(view.event_list.cursor, Some(last));
        }
        view.navigate_event(KeyCode::Home);
        assert_eq!(view.event_list.cursor, Some(0));
    }
}

#[test]
fn pinned_tail_keeps_details_and_follows_updates_and_task_restore() {
    for reset in [false, true] {
        let (_dir, mut view, events, first, second) = fixture();
        for row in &mut view.rows {
            row.status = SessionStatus::Ready;
        }
        view.status_filter = 0;
        for index in [0, 1] {
            view.select_event(view.shown_position(index));
            view.toggle_event_pin();
        }
        let area = Rect::new(0, 0, 100, 5);
        view.select_event(None);
        view.render_event(area, &mut Buffer::empty(area), Utc::now());
        view.navigate_event(KeyCode::Down);
        view.navigate_event(KeyCode::Down);
        let pinned = view.visible_event().unwrap().clone();
        let mut appended = events.clone();
        let mut event = events.last().unwrap().clone();
        event.source = Some("tail-arrival".into());
        appended.push(event);
        view.apply_event_update(crate::event::Update {
            events: appended.clone(),
            appended: 1,
            reset,
            ..Default::default()
        });
        view.render_event(area, &mut Buffer::empty(area), Utc::now());
        assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(1)));
        assert_eq!(view.visible_event(), Some(&pinned));
        assert!(!view.following_event());
        assert_eq!(view.event_scroll(), view.event_max_scroll());
        assert_eq!(view.event_list.unseen.get(), 0);
        // Preserve the selected pin even if the parked timeline anchor disappears.
        show(&mut view, &second, events.clone());
        let mut away = appended.last().unwrap().clone();
        away.source = Some("away-arrival".into());
        appended = vec![away];
        show(&mut view, &first, appended);
        view.render_event(area, &mut Buffer::empty(area), Utc::now());
        assert_eq!(view.visible_event(), Some(&pinned));
        assert_eq!(view.event_list.unseen.get(), 0);
        assert_eq!(view.event_scroll(), view.event_max_scroll());
        view.navigate_event(KeyCode::Up);
        view.navigate_event(KeyCode::Up);
        assert!(view.following_event());
        assert_eq!(
            view.visible_event().unwrap().source.as_deref(),
            Some("away-arrival")
        );
    }
}

#[test]
fn clicking_pin_stops_tail_even_if_that_pin_is_already_selected() {
    for enter_from_tail in [false, true] {
        let (_dir, mut view, events, _, _) = fixture();
        for row in &mut view.rows {
            row.status = SessionStatus::Ready;
        }
        view.status_filter = 0;
        view.select_event(Some(0));
        view.toggle_event_pin();
        view.select_event(None);
        if enter_from_tail {
            view.navigate_event(KeyCode::Down);
        }
        let area = Rect::new(0, 1, 100, 5);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 101, 8));
        view.render_event(area, &mut buffer, Utc::now());
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            view.handle_mouse(MouseEvent {
                kind,
                column: 3,
                row: 5,
                modifiers: KeyModifiers::NONE,
            });
        }
        assert_eq!(view.selected_event_row(), Some(EventRow::Pinned(0)));
        let scroll = view.event_scroll();
        let pinned = view.visible_event().unwrap().clone();
        let mut appended = events.clone();
        let mut event = events.last().unwrap().clone();
        event.source = Some("click-arrival".into());
        appended.push(event);
        view.set_event(appended);
        view.render_event(area, &mut buffer, Utc::now());
        assert_eq!(view.visible_event(), Some(&pinned));
        assert_eq!(view.event_scroll(), scroll);
        assert_eq!(view.event_list.unseen.get(), 1);
    }
}
