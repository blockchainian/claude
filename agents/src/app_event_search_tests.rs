use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn fixture() -> (tempfile::TempDir, App) {
    let (dir, mut app) = super::tests::fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    std::fs::write(
        &path,
        include_str!("../tests/fixtures/claude-bg-metadata.jsonl").repeat(4),
    )
    .unwrap();
    app.refresh_event_for_list_rows(1);
    assert!(app.transcript_parsed_bytes < std::fs::metadata(path).unwrap().len());
    super::tests::click_event(&mut app);
    (dir, app)
}

#[test]
fn first_list_search_does_not_read_history_on_the_ui_thread() {
    let (_dir, mut app) = fixture();
    let parsed = app.transcript_parsed_bytes;
    let loaded = app.view.event_panel.clone();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::NONE,
    )));
    assert!(app.view.event_list.search.input.applied());
    app.refresh_event_for_list_rows(8);
    assert_eq!(
        app.transcript_parsed_bytes, parsed,
        "opening search blocks on history"
    );
    assert_eq!(
        app.view.event_panel, loaded,
        "the loaded tail stays available"
    );
}

fn start_search(app: &mut App) {
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::NONE,
    )));
    app.refresh_event_for_list_rows(8);
    assert!(app.pending_transcript.is_some());
}

pub(super) fn finish_search(app: &mut App) {
    let mut schedule = RefreshSchedule::new(Instant::now(), FALLBACK_INTERVAL);
    while app.pending_transcript.is_some() {
        let input = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        app.handle_inputs(input, &mut None, &mut schedule).unwrap();
    }
}

#[test]
fn loaded_history_matches_the_current_query_and_catches_growth() {
    use std::io::Write;
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let expected = crate::providers::claude::event_reader::tail(&path, usize::MAX).unwrap();
    start_search(&mut app);
    for c in "zzzz_no_such_transcript_zzzz".chars() {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
        app.refresh_event_for_list_rows(8);
    }
    finish_search(&mut app);
    assert!(app.pending_transcript.is_none());
    assert_eq!(
        app.view.event_panel, expected,
        "full event bodies survive loading"
    );
    assert_eq!(app.view.matching_event_count(), 0, "use the latest query");
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.view.matching_event_count(), expected.len());
    for c in "agents-stage5-ux-1792167241".chars() {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    assert_eq!(
        app.view.matching_event_count(),
        expected
            .iter()
            .filter(|event| event.body.contains("agents-stage5-ux-1792167241"))
            .count(),
        "all older bodies are indexed"
    );
    let notification = include_str!("../tests/fixtures/claude-task-notifications.jsonl")
        .split_inclusive('\n')
        .find(|line| line.contains("<task-notification>"))
        .unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(notification.as_bytes()).unwrap();
    app.refresh_event_for_list_rows(8);
    assert_eq!(
        app.view.event_panel,
        crate::providers::claude::event_reader::tail(&path, usize::MAX).unwrap()
    );
}

#[test]
fn completed_history_does_not_replace_another_task_or_request() {
    let (_dir, mut app) = fixture();
    start_search(&mut app);
    let stale = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let first = app.sessions[0].clone();
    let mut second = first.clone();
    second.id.id = "second".into();
    second.updated_at -= chrono::Duration::seconds(1);
    second.transcript = Some(
        first
            .transcript
            .as_ref()
            .unwrap()
            .with_file_name("second.jsonl"),
    );
    std::fs::write(
        second.transcript.as_ref().unwrap(),
        include_bytes!("../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    app.sessions = vec![second.clone()];
    app.view.set_sessions(app.sessions.clone(), false);
    app.refresh_event_for_list_rows(8);
    let second_event = app.view.event_panel.clone();
    assert_eq!(app.view.selected_id(), Some(&second.id));
    let Input::EventsLoaded {
        token,
        reader,
        result,
    } = stale
    else {
        panic!("expected a transcript completion");
    };
    app.finish_event_load(token, reader, result);
    assert_eq!(app.view.event_panel, second_event);
    assert_eq!(app.view.event_session.as_ref(), Some(&second.id));
    app.sessions = vec![first.clone()];
    app.view.set_sessions(app.sessions.clone(), false);
    app.refresh_event_for_list_rows(8);
    // The saved search on the first task also loads asynchronously.
    assert!(app.pending_transcript.is_some());
    finish_search(&mut app);
    assert_eq!(app.view.event_session.as_ref(), Some(&first.id));
}

#[test]
fn history_load_errors_clear_the_pending_request() {
    let (_dir, mut app) = fixture();
    std::fs::remove_file(app.sessions[0].transcript.as_ref().unwrap()).unwrap();
    start_search(&mut app);
    finish_search(&mut app);
    assert!(app.pending_transcript.is_none());
    assert!(app.transcript.is_none());
    assert!(app.view.event_panel.is_empty());
    assert!(app.view.event_session.is_none());
}

#[test]
fn duration_sort_loads_history_off_the_ui_thread_and_time_keeps_it() {
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    std::fs::write(
        &path,
        include_str!("../tests/fixtures/claude-bg-metadata.jsonl").repeat(40),
    )
    .unwrap();
    app.refresh_event_for_list_rows(8);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 23)).unwrap();
    app.draw(&mut terminal).unwrap();
    super::tests::click_event(&mut app);
    let expected = crate::providers::claude::event_reader::tail(&path, usize::MAX).unwrap();
    let parsed = app.transcript_parsed_bytes;
    let loaded = app.view.event_panel.clone();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
    )));
    assert!(app.view.event_max_scroll() > 0);
    app.view.assert_event_tail();
    app.draw(&mut terminal).unwrap();
    app.view.assert_event_tail();
    assert!(
        app.pending_transcript.is_some(),
        "duration sorting loads the whole history"
    );
    assert_eq!(
        app.transcript_parsed_bytes, parsed,
        "history loads off the UI thread"
    );
    assert_eq!(
        app.view.event_panel, loaded,
        "the loaded tail stays available"
    );
    finish_search(&mut app);
    app.view.assert_event_tail();
    app.draw(&mut terminal).unwrap();
    app.view.assert_event_tail();
    assert_eq!(app.view.event_panel, expected);
    assert!(app.full_transcript_stamp.is_some());
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
    )));
    app.refresh_event_for_list_rows(1);
    assert_eq!(
        app.view.event_panel, expected,
        "returning to time keeps loaded history"
    );
    assert!(app.full_transcript_stamp.is_none());
}

#[test]
fn duration_history_load_errors_keep_loaded_rows_and_retry() {
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let transcript = std::fs::read(&path).unwrap();
    let loaded = app.view.event_panel.clone();
    let session = app.view.event_session.clone();
    std::fs::remove_file(&path).unwrap();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
    )));
    app.refresh_event_for_list_rows(8);
    assert!(
        app.pending_transcript.is_some(),
        "duration sorting loads the whole history"
    );
    finish_search(&mut app);
    assert!(app.pending_transcript.is_none());
    assert_eq!(
        app.view.event_panel, loaded,
        "failed history load keeps existing rows"
    );
    assert_eq!(app.view.event_session, session);
    std::fs::write(&path, transcript).unwrap();
    app.refresh_event_for_list_rows(8);
    assert!(
        app.pending_transcript.is_some(),
        "the next refresh retries history loading"
    );
    assert_eq!(
        app.view.event_panel, loaded,
        "retry keeps existing rows while loading"
    );
    finish_search(&mut app);
    assert_eq!(
        app.view.event_panel,
        crate::providers::claude::event_reader::tail(&path, usize::MAX).unwrap()
    );
}

#[test]
fn sorting_key_persists_both_modes() {
    let (_dir, mut app) = fixture();
    let id = app.view.selected_id().unwrap().to_string();
    for sorting in ["duration", "time"] {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('t'),
            KeyModifiers::NONE,
        )));
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(app.config_dir.join("agents.json")).unwrap())
                .unwrap();
        assert!(stored["config"].get("sorting").is_none());
        if sorting == "duration" {
            assert_eq!(stored["state"]["tasks"][&id]["sort"], sorting);
        } else {
            assert!(stored["state"]["tasks"].get(&id).is_none());
        }
    }
}

#[test]
fn saved_duration_loads_full_history_on_first_refresh() {
    let (_dir, fixture) = fixture();
    std::fs::write(
        fixture.config_dir.join("agents.json"),
        serde_json::json!({"config": {}, "state": {"tasks": {fixture.view.selected_id().unwrap().to_string(): {"sort": "duration"}}}}).to_string(),
    )
    .unwrap();
    let mut app = App::new(fixture.hub, fixture.config_dir);
    app.sessions = fixture.sessions;
    app.view.set_sessions(app.sessions.clone(), false);
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 23)).unwrap();
    app.draw(&mut terminal).unwrap();
    assert!(
        app.pending_transcript.is_some(),
        "first load requests full history"
    );
    assert_eq!(app.transcript_parsed_bytes, 0);
    finish_search(&mut app);
    app.draw(&mut terminal).unwrap();
    app.view.assert_event_tail();
    assert!(app.view.event_max_scroll() > 0);
    let path = app.sessions[0].transcript.as_ref().unwrap();
    assert_eq!(
        app.view.event_panel,
        crate::providers::claude::event_reader::tail(path, usize::MAX).unwrap()
    );
    app.view.apply_event_update(crate::event::Update {
        events: app.view.event_panel.clone(),
        reset: true,
        ..Default::default()
    });
    assert!(app.view.transcript_fully_loaded());
    app.view.assert_event_tail();
    let mut second = app.sessions[0].clone();
    second.id.id = "second-duration".into();
    app.sessions = vec![second];
    app.view.set_sessions(app.sessions.clone(), false);
    app.refresh_event_for_list_rows(1);
    assert!(app.pending_transcript.is_some());
    finish_search(&mut app);
    assert_eq!(app.view.event_list.sort, crate::ui::EventSort::Time);
    assert!(!app.view.transcript_fully_loaded());
}

#[test]
fn parked_event_position_loads_history_before_restoring_selection() {
    let (dir, mut app) = super::tests::fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let records: Vec<serde_json::Value> = include_str!("../tests/fixtures/transcript.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut transcript = String::new();
    for copy in 0..12 {
        for record in &records {
            let mut record = record.clone();
            if let Some(id) = record["uuid"].as_str() {
                record["uuid"] = format!("{id}-{copy}").into();
            }
            transcript.push_str(&serde_json::to_string(&record).unwrap());
            transcript.push('\n');
        }
    }
    std::fs::write(&path, transcript).unwrap();
    app.refresh_event_for_list_rows(usize::MAX);
    let first = app.sessions[0].id.clone();
    let mut other = app.sessions[0].clone();
    other.id.id = "other-history".into();
    other.updated_at -= chrono::Duration::seconds(1);
    other.transcript = Some(dir.path().join("other-history.jsonl"));
    std::fs::write(
        other.transcript.as_ref().unwrap(),
        include_bytes!("../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let second = other.id.clone();
    app.sessions.push(other);
    app.view.set_sessions(app.sessions.clone(), false);
    super::tests::click_event(&mut app);
    for code in [KeyCode::Home, KeyCode::Down] {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    let selected = app.view.event_panel[app.view.event_list.cursor.unwrap()]
        .source
        .clone();
    let anchor = app.view.event_panel[app.view.event_scroll()].source.clone();
    app.view.event_list.unseen.set(3);
    for _ in 0..2 {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::CONTROL,
        )));
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
        )));
    }
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    assert_eq!(app.view.selected_id(), Some(&second));
    app.refresh_event_for_list_rows(1);
    assert!(app.pending_transcript.is_some());
    finish_search(&mut app);
    assert!(app.view.event_panel.len() < 24);
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
    assert_eq!(app.view.selected_id(), Some(&first));
    app.refresh_event_for_list_rows(1);
    assert!(app.pending_transcript.is_some());
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 23)).unwrap();
    app.draw(&mut terminal).unwrap();
    assert_eq!(app.view.event_list.unseen.get(), 3);
    finish_search(&mut app);
    assert_eq!(
        app.view.event_panel[app.view.event_list.cursor.unwrap()].source,
        selected
    );
    assert_eq!(app.view.event_panel[app.view.event_scroll()].source, anchor);
    assert_eq!(app.view.event_list.unseen.get(), 3);
}
