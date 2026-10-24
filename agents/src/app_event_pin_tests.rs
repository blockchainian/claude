use super::*;

#[test]
fn inactive_snapshot_forgets_all_task_state_once() {
    let (dir, mut app) = super::tests::fixture();
    let id = app.sessions[0].id.clone();
    app.preferences
        .save_task_sort(dir.path(), &id, crate::model::EventSort::Duration)
        .unwrap();
    app.preferences
        .save_task_ask(
            dir.path(),
            &id,
            Some(crate::state::StoredAsk {
                id: app.view.event_panel[0].source.clone().unwrap(),
                page: 0,
                answers: Default::default(),
            }),
        )
        .unwrap();
    app.handle_action(Some(Action::TogglePin {
        tab: 0,
        id: id.clone(),
        pinned: true,
    }));
    let mut sessions = app.sessions.clone();
    sessions[0].status = SessionStatus::Inactive;
    app.apply_sessions(Snapshot {
        sessions: sessions.clone(),
        has_more: false,
        supervisions: vec![],
    });
    let saved = Preferences::load(dir.path());
    assert!(!saved.tasks.contains_key(&id));
    assert!(saved.pinned.active.is_empty());
    let store = dir.path().join("agents.json");
    let modified = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&store)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    app.apply_sessions(Snapshot {
        sessions,
        has_more: false,
        supervisions: vec![],
    });
    assert_eq!(
        std::fs::metadata(store).unwrap().modified().unwrap(),
        modified
    );
}

fn pin(event: &crate::event::Event) -> crate::state::PinnedEvent {
    crate::state::PinnedEvent {
        source: event.source.clone().unwrap(),
        records: event.source_records.clone(),
    }
}

fn reload(app: &mut App) {
    app.transcript = None;
    app.transcript_stamp = None;
    app.full_transcript_stamp = None;
    app.refresh_event(8);
    assert!(app.pending_transcript.is_some());
    super::event_search_tests::finish_search(app);
}

#[test]
fn event_pin_action_persists_order_and_unpin() {
    let (dir, mut app) = super::tests::fixture();
    let task = app.sessions[0].id.clone();
    let mut pinned: Vec<_> = app
        .view
        .event_panel
        .iter()
        .filter(|event| event.source.is_some())
        .take(2)
        .map(pin)
        .collect();
    pinned.reverse();
    assert_eq!(pinned.len(), 2);
    app.handle_action(Some(Action::ToggleEventPin {
        task: task.clone(),
        pinned: pinned.clone(),
    }));
    assert_eq!(
        Preferences::load(dir.path()).task_pinned_events(&task),
        pinned
    );
    app.handle_action(Some(Action::ToggleEventPin {
        task: task.clone(),
        pinned: vec![],
    }));
    assert!(
        Preferences::load(dir.path())
            .task_pinned_events(&task)
            .is_empty()
    );
}

#[test]
fn first_load_recovers_pins_and_removes_invalid_records_and_sources() {
    let (dir, mut app) = super::tests::fixture();
    let task = app.sessions[0].id.clone();
    let path = app.sessions[0].transcript.as_ref().unwrap();
    std::fs::write(
        path,
        include_bytes!("../tests/fixtures/claude-tool-calls.jsonl"),
    )
    .unwrap();
    let event = crate::providers::claude::event_reader::tail(path, usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.source.is_some() && event.source_records.len() > 1)
        .unwrap();
    let valid = pin(&event);
    let mut wrong_source = valid.clone();
    wrong_source.source.push_str("-mismatch");
    let mut missing = valid.clone();
    missing.records[0].offset = u64::MAX;
    app.preferences
        .save_task_pinned_events(
            dir.path(),
            &task,
            vec![wrong_source, valid.clone(), missing],
        )
        .unwrap();
    reload(&mut app);
    assert_eq!(
        Preferences::load(dir.path()).task_pinned_events(&task),
        std::slice::from_ref(&valid)
    );
    assert_eq!(app.view.event_list.pinned.len(), 1);
    let row = &app.view.event_list.pinned[0];
    assert_eq!(row.pin, valid);
    let mut expected = event;
    expected.turn = None;
    expected.previous_end = None;
    assert_eq!(row.event, expected);
}

#[test]
fn inactive_snapshot_clears_displayed_and_parked_event_pins() {
    let (dir, mut app) = super::tests::fixture();
    let task = app.sessions[0].id.clone();
    let event = app
        .view
        .event_panel
        .iter()
        .find(|event| event.source.is_some())
        .unwrap()
        .clone();
    let pinned = pin(&event);
    let mut other = app.sessions[0].clone();
    other.id.id.push_str("-parked");
    app.view.set_pinned_events(
        &other.id,
        vec![crate::ui::PinnedRow {
            pin: pinned.clone(),
            event: event.clone(),
        }],
    );
    app.view.set_pinned_events(
        &task,
        vec![crate::ui::PinnedRow {
            pin: pinned.clone(),
            event,
        }],
    );
    app.preferences
        .save_task_pinned_events(dir.path(), &task, vec![pinned.clone()])
        .unwrap();
    app.preferences
        .save_task_pinned_events(dir.path(), &other.id, vec![pinned])
        .unwrap();
    let mut sessions = app.sessions.clone();
    sessions.push(other.clone());
    for session in &mut sessions {
        session.status = SessionStatus::Inactive;
    }
    app.apply_sessions(Snapshot {
        sessions,
        has_more: false,
        supervisions: vec![],
    });
    assert!(!app.view.has_pinned_events(&task));
    assert!(!app.view.has_pinned_events(&other.id));
    let saved = Preferences::load(dir.path());
    assert!(!saved.tasks.contains_key(&task));
    assert!(!saved.tasks.contains_key(&other.id));
}

#[test]
fn pending_pin_restore_does_not_resurrect_forgotten_state() {
    let (dir, mut app) = super::tests::fixture();
    let task = app.sessions[0].id.clone();
    let event = app
        .view
        .event_panel
        .iter()
        .find(|event| event.source.is_some())
        .unwrap()
        .clone();
    let pinned = pin(&event);
    app.preferences
        .save_task_pinned_events(dir.path(), &task, vec![pinned.clone()])
        .unwrap();
    app.transcript = None;
    app.refresh_event(8);
    assert!(app.pending_transcript.is_some());
    let mut sessions = app.sessions.clone();
    sessions[0].status = SessionStatus::Inactive;
    app.apply_sessions(Snapshot {
        sessions,
        has_more: false,
        supervisions: vec![],
    });
    super::event_search_tests::finish_search(&mut app);
    assert!(!app.view.has_pinned_events(&task));
    assert!(
        Preferences::load(dir.path())
            .task_pinned_events(&task)
            .is_empty()
    );
}

#[test]
fn valid_pin_restore_keeps_order_without_rewriting_store() {
    let (dir, mut app) = super::tests::fixture();
    let task = app.sessions[0].id.clone();
    let pinned: Vec<_> = app
        .view
        .event_panel
        .iter()
        .filter(|event| {
            event
                .source
                .as_ref()
                .is_some_and(|source| !source.is_empty())
                && !event.source_records.is_empty()
        })
        .rev()
        .take(2)
        .map(pin)
        .collect();
    assert_eq!(pinned.len(), 2);
    app.preferences
        .save_task_pinned_events(dir.path(), &task, pinned.clone())
        .unwrap();
    let store = dir.path().join("agents.json");
    let modified = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&store)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    reload(&mut app);
    assert_eq!(
        app.view
            .event_list
            .pinned
            .iter()
            .map(|row| row.pin.clone())
            .collect::<Vec<_>>(),
        pinned
    );
    assert_eq!(
        std::fs::metadata(store).unwrap().modified().unwrap(),
        modified
    );
}

#[test]
fn event_refresh_preserves_pinned_timeline_scroll_after_leaving_newest() {
    use ratatui::backend::TestBackend;

    for pinned_count in [1, 3] {
        let (_dir, mut app) = super::tests::fixture();
        let path = app.sessions[0].transcript.as_ref().unwrap();
        std::fs::write(
            path,
            include_bytes!("../tests/fixtures/claude-tool-calls.jsonl"),
        )
        .unwrap();
        reload(&mut app);
        let task = app.sessions[0].id.clone();
        let pins = app
            .view
            .event_panel
            .iter()
            .rev()
            .take(pinned_count)
            .map(|event| crate::ui::PinnedRow {
                pin: pin(event),
                event: event.clone(),
            })
            .collect();
        app.view.set_pinned_events(&task, pins);
        app.view.event_list.cursor = app.view.event_panel.len().checked_sub(1);
        let mut terminal = Terminal::new(TestBackend::new(200, 24)).unwrap();
        app.draw(&mut terminal).unwrap();
        let scroll = app.view.event_scroll();
        assert!(scroll > 0, "fixture must overflow the timeline");
        // Moving off the newest row stops render from revealing that row again.
        app.view.event_list.cursor = app.view.event_list.cursor.map(|index| index - 1);
        app.draw(&mut terminal).unwrap();
        assert_eq!(app.view.event_scroll(), scroll, "pins: {pinned_count}");
    }
}
