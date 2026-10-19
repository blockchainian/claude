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
    app.refresh_log(1);
    assert!(app.log_parsed_bytes < std::fs::metadata(path).unwrap().len());
    super::tests::click_log(&mut app);
    (dir, app)
}

#[test]
fn first_list_search_does_not_read_history_on_the_ui_thread() {
    let (_dir, mut app) = fixture();
    let parsed = app.log_parsed_bytes;
    let loaded = app.view.log.clone();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::NONE,
    )));
    assert!(app.view.log_search.active());
    app.refresh_log(8);
    assert_eq!(
        app.log_parsed_bytes, parsed,
        "opening search blocks on history"
    );
    assert_eq!(app.view.log, loaded, "the loaded tail stays available");
}

fn start_search(app: &mut App) {
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::NONE,
    )));
    app.refresh_log(8);
    assert!(app.pending_log.is_some());
}

pub(super) fn finish_search(app: &mut App) {
    let input = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut schedule = RefreshSchedule::new(Instant::now(), FALLBACK_INTERVAL);
    app.handle_inputs(input, &mut None, &mut schedule).unwrap();
}

#[test]
fn loaded_history_matches_the_current_query_and_catches_growth() {
    use std::io::Write;
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let expected = crate::providers::claude::log::tail(&path, usize::MAX).unwrap();
    start_search(&mut app);
    for c in "zzzz_no_such_transcript_zzzz".chars() {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
        app.refresh_log(8);
    }
    finish_search(&mut app);
    assert!(app.pending_log.is_none());
    assert_eq!(app.view.log, expected, "full event bodies survive loading");
    assert_eq!(app.view.matching_log_count(), 0, "use the latest query");
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.view.matching_log_count(), expected.len());
    for c in "agents-stage5-ux-1792167241".chars() {
        app.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
    assert_eq!(
        app.view.matching_log_count(),
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
    app.refresh_log(8);
    assert_eq!(
        app.view.log,
        crate::providers::claude::log::tail(&path, usize::MAX).unwrap()
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
    app.refresh_log(8);
    let second_log = app.view.log.clone();
    assert_eq!(app.view.selected_id(), Some(&second.id));
    let mut schedule = RefreshSchedule::new(Instant::now(), FALLBACK_INTERVAL);
    app.handle_inputs(stale, &mut None, &mut schedule).unwrap();
    assert_eq!(app.view.log, second_log);
    assert_eq!(app.view.log_session.as_ref(), Some(&second.id));
    app.sessions = vec![first.clone()];
    app.view.set_sessions(app.sessions.clone(), false);
    app.refresh_log(8);
    // The saved search on the first task also loads asynchronously.
    assert!(app.pending_log.is_some());
    finish_search(&mut app);
    assert_eq!(app.view.log_session.as_ref(), Some(&first.id));
}

#[test]
fn history_load_errors_clear_the_pending_request() {
    let (_dir, mut app) = fixture();
    std::fs::remove_file(app.sessions[0].transcript.as_ref().unwrap()).unwrap();
    start_search(&mut app);
    finish_search(&mut app);
    assert!(app.pending_log.is_none());
    assert!(app.task_log.is_none());
    assert!(app.view.log.is_empty());
    assert!(app.view.log_session.is_none());
}

#[test]
fn duration_sort_loads_history_off_the_ui_thread_and_time_keeps_it() {
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let expected = crate::providers::claude::log::tail(&path, usize::MAX).unwrap();
    let parsed = app.log_parsed_bytes;
    let loaded = app.view.log.clone();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
    )));
    app.refresh_log(8);
    assert!(
        app.pending_log.is_some(),
        "duration sorting loads the whole history"
    );
    assert_eq!(
        app.log_parsed_bytes, parsed,
        "history loads off the UI thread"
    );
    assert_eq!(app.view.log, loaded, "the loaded tail stays available");
    finish_search(&mut app);
    assert_eq!(app.view.log, expected);
    assert!(app.full_log_stamp.is_some());
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
    )));
    app.refresh_log(1);
    assert_eq!(
        app.view.log, expected,
        "returning to time keeps loaded history"
    );
    assert!(app.full_log_stamp.is_none());
}

#[test]
fn duration_history_load_errors_keep_loaded_rows_and_retry() {
    let (_dir, mut app) = fixture();
    let path = app.sessions[0].transcript.clone().unwrap();
    let transcript = std::fs::read(&path).unwrap();
    let loaded = app.view.log.clone();
    let session = app.view.log_session.clone();
    std::fs::remove_file(&path).unwrap();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('t'),
        KeyModifiers::NONE,
    )));
    app.refresh_log(8);
    assert!(
        app.pending_log.is_some(),
        "duration sorting loads the whole history"
    );
    finish_search(&mut app);
    assert!(app.pending_log.is_none());
    assert_eq!(
        app.view.log, loaded,
        "failed history load keeps existing rows"
    );
    assert_eq!(app.view.log_session, session);
    std::fs::write(&path, transcript).unwrap();
    app.refresh_log(8);
    assert!(
        app.pending_log.is_some(),
        "the next refresh retries history loading"
    );
    assert_eq!(
        app.view.log, loaded,
        "retry keeps existing rows while loading"
    );
    finish_search(&mut app);
    assert_eq!(
        app.view.log,
        crate::providers::claude::log::tail(&path, usize::MAX).unwrap()
    );
}
