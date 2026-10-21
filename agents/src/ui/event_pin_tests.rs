use super::*;
use crate::event::{Event, Kind};
use crate::providers::Provider;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(view: &mut View, code: KeyCode) -> Option<Action> {
    view.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// A view showing the events a real Claude transcript decodes to.
fn fixture() -> (tempfile::TempDir, View, Vec<Event>) {
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
    let mut session = provider.refresh(1).sessions.remove(0);
    session.status = crate::model::SessionStatus::Ready;
    session.transcript = Some(path);
    let events = crate::providers::event_reader(&session)
        .unwrap()
        .refresh(100)
        .unwrap()
        .events;
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(vec![session], false);
    view.event_session = view.selected_id().cloned();
    view.set_event(events.clone());
    view.active_panel = Panel::EventList;
    draw(&view);
    (dir, view, events)
}

fn draw(view: &View) {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 12));
    view.render_event(Rect::new(0, 1, 100, 10), &mut buffer, Utc::now());
}

fn pin_at(view: &mut View, index: usize) -> Option<Action> {
    view.select_event(Some(index));
    key(view, KeyCode::Char('p'))
}

fn pinned_sources(view: &View) -> Vec<Option<String>> {
    view.event_list
        .pinned
        .iter()
        .map(|row| row.event.source.clone())
        .collect()
}

fn pinnable_indices(events: &[Event]) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event.source.as_deref().is_some_and(|source| !source.is_empty())
                && !event.source_records.is_empty()
        })
        .map(|(index, _)| index)
        .collect()
}

#[test]
fn pinning_puts_the_newest_pin_on_top_and_unpinning_removes_it() {
    let (_dir, mut view, events) = fixture();
    let rows = pinnable_indices(&events);
    let (a, b) = (rows[0], rows[1]);
    let Some(Action::ToggleEventPin { task, pinned }) = pin_at(&mut view, a) else {
        panic!("pinning emits the new list");
    };
    assert_eq!(Some(&task), view.selected_id());
    assert_eq!(pinned.len(), 1);
    assert_eq!(pinned[0].source, events[a].source.clone().unwrap());
    assert_eq!(pinned[0].records, events[a].source_records);
    pin_at(&mut view, b);
    assert_eq!(
        pinned_sources(&view),
        vec![events[b].source.clone(), events[a].source.clone()]
    );
    // The snapshot has no context from its neighbours.
    assert!(view.event_list.pinned.iter().all(|row| row.event.turn.is_none()
        && row.event.previous_end.is_none()));
    let Some(Action::ToggleEventPin { pinned, .. }) = pin_at(&mut view, b) else {
        panic!("unpinning emits the new list");
    };
    assert_eq!(pinned.len(), 1);
    assert_eq!(pinned_sources(&view), vec![events[a].source.clone()]);
    // Pinning again puts it back on top.
    pin_at(&mut view, b);
    assert_eq!(
        pinned_sources(&view),
        vec![events[b].source.clone(), events[a].source.clone()]
    );
}

#[test]
fn pinned_log_stays_in_the_timeline_and_p_there_unpins_it() {
    let (_dir, mut view, events) = fixture();
    let index = pinnable_indices(&events)[0];
    pin_at(&mut view, index);
    assert_eq!(view.event_panel.len(), events.len());
    assert_eq!(view.event_list.cursor, Some(index));
    assert!(view.has_pinned_events(&view.selected_id().cloned().unwrap()));
    key(&mut view, KeyCode::Char('p'));
    assert!(view.event_list.pinned.is_empty());
}

#[test]
fn p_unpins_every_pin_sharing_a_record_with_the_row() {
    let (_dir, mut view, events) = fixture();
    let index = pinnable_indices(&events)[0];
    pin_at(&mut view, index);
    let mut other = view.event_list.pinned[0].clone();
    other.pin.source = "another".to_owned();
    view.event_list.pinned.push(other);
    key(&mut view, KeyCode::Char('p'));
    assert!(view.event_list.pinned.is_empty());
}

#[test]
fn rows_without_identity_or_records_or_still_queued_are_not_pinnable() {
    let (_dir, mut view, events) = fixture();
    let index = pinnable_indices(&events)[0];
    for change in [
        (|event: &mut Event| event.source = None) as fn(&mut Event),
        |event| event.source = Some(String::new()),
        |event| event.source_records.clear(),
        |event| event.kind = Kind::QueuedUser,
        |event| event.kind = Kind::QueuedSystem,
    ] {
        let mut changed = events.clone();
        change(&mut changed[index]);
        view.set_event(changed);
        assert!(matches!(
            pin_at(&mut view, index),
            Some(Action::Unsupported("Pin"))
        ));
        assert!(view.event_list.pinned.is_empty());
    }
}

#[test]
fn inactive_tasks_cannot_pin_events() {
    let (_dir, mut view, events) = fixture();
    let mut session = view.selected_row().unwrap().clone();
    session.status = crate::model::SessionStatus::Inactive;
    view.set_sessions(vec![session], false);
    view.status_filter = 1;
    view.reconcile_selection();
    view.event_session = view.selected_id().cloned();
    view.set_event(events.clone());
    view.active_panel = Panel::EventList;
    let action = pin_at(&mut view, pinnable_indices(&events)[0]);
    assert!(matches!(action, Some(Action::Unsupported("Pin"))), "{action:?}");
}

#[test]
fn cursor_moves_between_the_pinned_area_and_the_timeline() {
    let (_dir, mut view, events) = fixture();
    let rows = pinnable_indices(&events);
    pin_at(&mut view, rows[0]);
    pin_at(&mut view, rows[1]);
    view.select_event(Some(0));
    key(&mut view, KeyCode::Up);
    assert_eq!(view.event_list.pinned_cursor, Some(1));
    assert_eq!(view.event_list.cursor, None);
    assert_eq!(
        view.visible_event().unwrap().source,
        events[rows[0]].source,
        "the detail follows the pinned row"
    );
    key(&mut view, KeyCode::Up);
    assert_eq!(view.event_list.pinned_cursor, Some(0));
    key(&mut view, KeyCode::Up);
    assert_eq!(view.event_list.pinned_cursor, Some(0));
    key(&mut view, KeyCode::Down);
    key(&mut view, KeyCode::Down);
    assert_eq!(view.event_list.pinned_cursor, None);
    assert_eq!(view.event_list.cursor, Some(0));
    key(&mut view, KeyCode::Up);
    key(&mut view, KeyCode::End);
    assert_eq!(view.event_list.pinned_cursor, None);
    key(&mut view, KeyCode::Home);
    assert_eq!(view.event_list.pinned_cursor, Some(0));
}

#[test]
fn unpinning_from_the_pinned_area_keeps_the_cursor_on_the_log() {
    let (_dir, mut view, events) = fixture();
    let index = pinnable_indices(&events)[0];
    pin_at(&mut view, index);
    key(&mut view, KeyCode::Up);
    assert_eq!(view.event_list.pinned_cursor, Some(0));
    key(&mut view, KeyCode::Char('p'));
    assert!(view.event_list.pinned.is_empty());
    assert_eq!(view.event_list.pinned_cursor, None);
    assert_eq!(view.event_list.cursor, Some(index));
}

#[test]
fn cursor_in_the_pinned_area_survives_switching_tasks() {
    let (_dir, mut view, events) = fixture();
    let index = pinnable_indices(&events)[0];
    pin_at(&mut view, index);
    key(&mut view, KeyCode::Up);
    view.park_event_list();
    assert_eq!(view.event_list.pinned_cursor, None);
    view.event_list.restoring = true;
    view.set_event(events.clone());
    assert_eq!(view.event_list.pinned_cursor, Some(0));
    assert_eq!(view.event_list.cursor, None);
}

#[test]
fn pinned_events_can_be_set_and_cleared_for_a_task() {
    let (_dir, mut view, events) = fixture();
    let key = view.selected_id().cloned().unwrap();
    let index = pinnable_indices(&events)[0];
    pin_at(&mut view, index);
    let rows = view.event_list.pinned.clone();
    view.clear_pinned_events(&key);
    assert!(!view.has_pinned_events(&key));
    view.set_pinned_events(&key, rows);
    assert!(view.has_pinned_events(&key));
    self::key(&mut view, KeyCode::Up);
    view.clear_pinned_events(&key);
    assert_eq!(view.event_list.pinned_cursor, None);
    assert!(view.event_list.cursor.is_some());
}

#[test]
fn pins_follow_a_parked_task() {
    let (_dir, mut view, events) = fixture();
    let key = view.selected_id().cloned().unwrap();
    let index = pinnable_indices(&events)[0];
    pin_at(&mut view, index);
    let rows = view.event_list.pinned.clone();
    view.panels_task = None;
    view.task_uis
        .entry(draft_controls::TaskKey::Session(key.clone()))
        .or_default()
        .event_list = std::mem::take(&mut view.event_list);
    assert!(view.has_pinned_events(&key));
    view.clear_pinned_events(&key);
    assert!(!view.has_pinned_events(&key));
    view.set_pinned_events(&key, rows.clone());
    assert_eq!(view.task_uis.values().next().unwrap().event_list.pinned, rows);
}

#[test]
fn clicks_select_pinned_rows_and_timeline_rows_below_them() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_dir, mut view, events) = fixture();
    let index = pinnable_indices(&events)[0];
    pin_at(&mut view, index);
    draw(&view);
    let (panel_y, timeline_top) = {
        let viewport = view.viewport.borrow();
        assert_eq!(viewport.event_pinned_rows, vec![0]);
        (viewport.event_panel.y, viewport.event_timeline_top)
    };
    assert!(timeline_top > panel_y);
    let click = |view: &mut View, row| {
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            view.handle_mouse(MouseEvent {
                kind,
                column: 5,
                row,
                modifiers: KeyModifiers::NONE,
            });
        }
    };
    click(&mut view, panel_y);
    assert_eq!(view.event_list.pinned_cursor, Some(0));
    assert_eq!(view.event_list.cursor, None);
    click(&mut view, timeline_top);
    assert_eq!(view.event_list.pinned_cursor, None);
    assert!(view.event_list.cursor.is_some());
}
