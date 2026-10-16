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
