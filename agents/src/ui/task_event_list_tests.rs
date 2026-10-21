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
