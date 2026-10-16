use super::*;
use crate::providers::{Job, SessionInput};

#[test]
fn input_jobs_keep_results_when_the_terminal_reader_is_absent() {
    let (directory, mut app) = tests::fixture();
    let id = app.sessions[0].id.clone();
    let output = directory.path().join("input-result");
    let written = output.clone();
    let text = include_str!("../tests/fixtures/claude-peer-message.txt").to_owned();
    let expected = text.clone();
    let job: Job = Box::new(move || std::fs::write(written, text));
    assert!(app.start_input_job(id.clone(), SessionInput::Prompt(expected.clone()), job));
    let first = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(std::fs::read_to_string(output).unwrap(), expected);
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    assert!(!app.handle_inputs(first, &mut None, &mut schedule).unwrap());
    assert!(!app.pending_inputs.contains(&id));
}

#[test]
fn one_input_job_per_session_blocks_open_until_completion() {
    let (directory, mut app) = tests::fixture();
    let id = app.sessions[0].id.clone();
    let (release, wait) = mpsc::channel();
    let path = directory.path().join("completed");
    let duplicate = directory.path().join("duplicate");
    let duplicate_write = duplicate.clone();
    assert!(app.start_input_job(
        id.clone(),
        SessionInput::Interrupt,
        Box::new(move || {
            wait.recv_timeout(Duration::from_secs(5))
                .map_err(io::Error::other)?;
            std::fs::write(path, b"completed")
        })
    ));
    assert!(!app.start_input_job(
        id.clone(),
        SessionInput::Interrupt,
        Box::new(move || std::fs::write(duplicate_write, b"wrong"))
    ));
    assert!(!app.handle_action(Some(Action::Open(id.clone()))));
    assert!(app.launch.is_none());
    release.send(()).unwrap();
    let first = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(first, &mut None, &mut schedule).unwrap();
    assert!(!app.pending_inputs.contains(&id));
    assert!(!duplicate.exists());
    assert!(app.handle_action(Some(Action::Open(id))));
    assert!(app.launch.is_some());
}

#[test]
fn failed_input_job_releases_the_session_for_retry() {
    let (directory, mut app) = tests::fixture();
    let id = app.sessions[0].id.clone();
    let missing = directory.path().join("missing/parent/result");
    assert!(app.start_input_job(
        id.clone(),
        SessionInput::Interrupt,
        Box::new(move || std::fs::write(missing, b"cannot write"))
    ));
    let first = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(first, &mut None, &mut schedule).unwrap();
    assert!(!app.pending_inputs.contains(&id));
    assert!(app.start_input_job(id, SessionInput::Interrupt, Box::new(|| Ok(()))));
}

#[test]
fn stop_uses_input_job_and_reports_completion_through_event_channel() {
    let (_directory, mut app) = tests::fixture();
    let id = app.sessions[0].id.clone();
    assert!(app.sessions[0].process.is_none());
    assert!(!app.handle_action(Some(Action::Stop(id.clone()))));
    assert!(app.pending_inputs.contains(&id));
    let event = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(!app.pending_inputs.contains(&id));
}

#[test]
fn codex_stop_refreshes_after_failure_and_reports_in_footer() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("state_5.sqlite");
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    let provider = crate::providers::codex::CodexProvider::new(
        directory.path().to_owned(),
        directory.path().join("manager"),
    );
    let mut app = App::new(
        Hub::new(vec![Box::new(provider)]),
        directory.path().to_owned(),
    );
    app.refresh();
    let id = app.sessions[0].id.clone();
    assert!(!app.handle_action(Some(Action::Stop(id.clone()))));
    assert!(app.pending_inputs.contains(&id));
    let event = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(&event, Input::Sent { result: Err(error), .. }
        if error.to_string().contains("daemon isn't running")));
    db.execute(
        "UPDATE threads SET title='Refreshed after stop' WHERE id=?1",
        [&id.id],
    )
    .unwrap();
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(!app.pending_inputs.contains(&id));
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render(area, &mut buffer);
    let screen = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("Failed to stop"), "{screen}");
    assert!(!screen.contains(" · failed:"), "{screen}");
    assert!(!screen.contains(" · sending"), "{screen}");
    assert_eq!(app.session(&id).unwrap().title, "Refreshed after stop");
}
