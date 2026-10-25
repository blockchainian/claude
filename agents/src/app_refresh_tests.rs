use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;

fn claude_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    for index in 1..=2 {
        std::fs::write(
            project.join(format!("00000000-0000-0000-0000-{index:012}.jsonl")),
            include_bytes!("../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
    }
    home
}

fn claude_hub(home: &std::path::Path) -> Hub {
    Hub::new(vec![Box::new(
        crate::providers::claude::ClaudeProvider::new(home.into()),
    )])
}

#[test]
fn filesystem_refresh_updates_only_the_affected_session() {
    use std::io::Write;
    let home = claude_home();
    let mut hub = claude_hub(home.path());
    let before = hub.refresh();
    let target = before.sessions[0].transcript.clone().unwrap();
    let unaffected = before.sessions[1].clone();
    for session in &before.sessions {
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(session.transcript.as_ref().unwrap())
                .unwrap(),
            "{}",
            serde_json::json!({"type": "custom-title", "customTitle": "Changed title", "sessionId": session.id.id})
        )
        .unwrap();
    }
    let after = hub.refresh_paths(&[target], before);
    assert_eq!(after.sessions.len(), 2);
    let updated = after
        .sessions
        .iter()
        .find(|s| s.id != unaffected.id)
        .unwrap();
    assert_eq!(updated.title, "Changed title");
    assert_eq!(
        after
            .sessions
            .iter()
            .find(|s| s.id == unaffected.id)
            .unwrap(),
        &unaffected,
        "an unrelated transcript must not be refreshed"
    );
}

#[test]
fn refresh_allows_first_frame_input_and_exit_while_sqlite_is_locked() {
    let home = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(home.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    // A real database lock bounds the blocked reader without substituting a provider.
    let (release, locked) = mpsc::channel();
    let owner = thread::spawn(move || {
        let _ = locked.recv_timeout(Duration::from_millis(800));
        db.execute_batch("COMMIT").unwrap();
    });
    let mut app = App::new(
        Hub::new(vec![Box::new(crate::providers::codex::CodexProvider::new(
            home.path().into(),
        ))]),
        home.path().into(),
    );
    let start = Instant::now();
    app.request_refresh();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    let mut schedule = RefreshSchedule::new(Instant::now(), FALLBACK_INTERVAL);
    assert!(
        app.handle_inputs(
            Input::Terminal(Ok(Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )))),
            &mut None,
            &mut schedule,
        )
        .unwrap()
    );
    let receiver = std::mem::replace(&mut app.receiver, mpsc::channel().1);
    drop(app);
    let elapsed = start.elapsed();
    let _ = release.send(());
    owner.join().unwrap();
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(5)).unwrap(),
        Input::Sessions { .. }
    ));
    assert!(elapsed < Duration::from_millis(300), "{elapsed:?}");
}

#[test]
fn unknown_filesystem_path_discovers_new_sessions() {
    let home = claude_home();
    let mut hub = claude_hub(home.path());
    let before = hub.refresh();
    let path = home
        .path()
        .join("projects/project/00000000-0000-0000-0000-000000000003.jsonl");
    std::fs::write(&path, include_bytes!("../tests/fixtures/transcript.jsonl")).unwrap();
    let after = hub.refresh_paths(&[path], before);
    assert_eq!(after.sessions.len(), 3);
}

#[test]
fn targeted_refresh_keeps_the_history_page_budget_with_subagents() {
    let home = claude_home();
    let project = home.path().join("projects/project");
    let parent = project.join("00000000-0000-0000-0000-000000000001.jsonl");
    let children = parent.with_extension("").join("subagents");
    std::fs::create_dir_all(&children).unwrap();
    for index in 0..13 {
        let path = children.join(format!("agent-{index:016x}.jsonl"));
        std::fs::write(&path, include_bytes!("../tests/fixtures/subagent.jsonl")).unwrap();
        std::fs::File::open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(index))
            .unwrap();
    }
    let mut hub = claude_hub(home.path());
    let before = hub.refresh();
    assert_eq!(before.sessions.len(), 10);
    assert!(
        before
            .sessions
            .iter()
            .any(|s| s.transcript.as_ref() == Some(&parent))
    );
    let after = hub.refresh_paths(&[parent], before);
    let expected = hub.refresh();
    assert_eq!(after.sessions, expected.sessions);
    assert_eq!(after.has_more, expected.has_more);
}

#[test]
fn targeted_refresh_backfills_a_removed_history_row() {
    let home = claude_home();
    let project = home.path().join("projects/project");
    for index in 3..=12 {
        std::fs::write(
            project.join(format!("00000000-0000-0000-0000-{index:012}.jsonl")),
            include_bytes!("../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
    }
    let mut hub = claude_hub(home.path());
    let before = hub.refresh();
    let path = before.sessions[0].transcript.clone().unwrap();
    std::fs::remove_file(&path).unwrap();
    let after = hub.refresh_paths(&[path], before);
    let expected = hub.refresh();
    assert_eq!(after.sessions, expected.sessions);
    assert_eq!(after.has_more, expected.has_more);
}

#[test]
fn subagent_file_change_does_not_refresh_its_parent_row() {
    use std::io::Write;
    let home = claude_home();
    let parent = home
        .path()
        .join("projects/project/00000000-0000-0000-0000-000000000001.jsonl");
    let children = parent.with_extension("").join("subagents");
    std::fs::create_dir_all(&children).unwrap();
    let child = children.join("agent-abf695d5995bd43ab.jsonl");
    std::fs::write(&child, include_bytes!("../tests/fixtures/subagent.jsonl")).unwrap();
    let mut hub = claude_hub(home.path());
    let before = hub.refresh();
    let parent_before = before
        .sessions
        .iter()
        .find(|s| s.transcript.as_ref() == Some(&parent))
        .unwrap()
        .clone();
    writeln!(std::fs::OpenOptions::new().append(true).open(&parent).unwrap(), "{}", serde_json::json!({"type": "custom-title", "customTitle": "Unrelated parent edit", "sessionId": parent_before.id.id})).unwrap();
    let after = hub.refresh_paths(&[child], before);
    assert_eq!(
        after
            .sessions
            .iter()
            .find(|s| s.id == parent_before.id)
            .unwrap(),
        &parent_before
    );
}

#[test]
fn codex_filesystem_refresh_leaves_unrelated_rows_unchanged() {
    let home = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(home.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    let ids: Vec<String> = db
        .prepare("SELECT id FROM threads")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for id in ids {
        let path = home.path().join(format!("{id}.jsonl"));
        std::fs::write(
            &path,
            include_bytes!("../tests/fixtures/codex-thread-clock.jsonl"),
        )
        .unwrap();
        db.execute(
            "UPDATE threads SET rollout_path=?1 WHERE id=?2",
            rusqlite::params![path.to_str().unwrap(), id],
        )
        .unwrap();
    }
    let mut hub = Hub::new(vec![Box::new(crate::providers::codex::CodexProvider::new(
        home.path().into(),
    ))]);
    let before = hub.refresh();
    let target = before.sessions[0].clone();
    let unaffected = before.sessions[1].clone();
    db.execute(
        "UPDATE threads SET title='Changed title' WHERE id=?1 OR id=?2",
        [&target.id.id, &unaffected.id.id],
    )
    .unwrap();
    let after = hub.refresh_paths(&[target.transcript.unwrap()], before);
    assert_eq!(
        after
            .sessions
            .iter()
            .find(|s| s.id == target.id)
            .unwrap()
            .title,
        "Changed title"
    );
    assert_eq!(
        after
            .sessions
            .iter()
            .find(|s| s.id == unaffected.id)
            .unwrap(),
        &unaffected
    );
}

#[test]
fn filesystem_input_keeps_paths_through_the_debounce() {
    let home = claude_home();
    let mut app = App::new(claude_hub(home.path()), home.path().into());
    let mut watcher = Some(crate::sessions::watch(&[], |_| {}).unwrap());
    let now = Instant::now();
    let mut schedule = RefreshSchedule::new(now, FALLBACK_INTERVAL);
    let path = home
        .path()
        .join("projects/project/00000000-0000-0000-0000-000000000001.jsonl");
    let event = notify::Event::new(notify::EventKind::Any).add_path(path.clone());
    app.handle_inputs(Input::Filesystem(Ok(event)), &mut watcher, &mut schedule)
        .unwrap();
    assert_eq!(
        schedule.take_due(now + Duration::from_millis(100)),
        Some(crate::sessions::RefreshRequest::paths(vec![path]))
    );
}

#[test]
fn output_file_notifications_do_not_schedule_session_refresh() {
    use std::io::Write;
    let home = claude_home();
    let mut app = App::new(claude_hub(home.path()), home.path().into());
    let output = home.path().join("task.output");
    std::fs::write(&output, "").unwrap();
    let sender = app.sender.clone();
    let mut output_watcher = crate::sessions::OutputWatcher::new(move |event| {
        let _ = sender.send(Input::EventOutput(event));
    })
    .unwrap();
    output_watcher.set_path(Some(output.clone())).unwrap();
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(output)
            .unwrap(),
        "background output"
    )
    .unwrap();
    let input = app.receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(input, Input::EventOutput(Ok(_))));
    let now = Instant::now();
    let mut schedule = RefreshSchedule::new(now, FALLBACK_INTERVAL);
    app.handle_inputs(input, &mut None, &mut schedule).unwrap();
    assert_eq!(schedule.take_due(now + Duration::from_millis(100)), None);
}

#[test]
fn hidden_event_list_still_loads_the_detail() {
    let home = claude_home();
    for index in 1..=2 {
        std::fs::write(
            home.path().join(format!(
                "projects/project/00000000-0000-0000-0000-{index:012}.jsonl"
            )),
            include_bytes!("../tests/fixtures/claude-raw-detail.jsonl"),
        )
        .unwrap();
    }
    let mut app = App::new(claude_hub(home.path()), home.path().into());
    app.refresh();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::NONE,
    )));
    assert!(app.view.selected_id().is_some());
    assert!(app.transcript.is_none());
    let area = ratatui::layout::Rect::new(0, 0, 200, 40);
    app.view
        .render(area, &mut ratatui::buffer::Buffer::empty(area));
    for (code, modifiers) in [
        (KeyCode::Char('w'), KeyModifiers::CONTROL),
        (KeyCode::Char('_'), KeyModifiers::NONE),
    ] {
        app.handle_event(Event::Key(KeyEvent::new(code, modifiers)));
    }
    let mut terminal = Terminal::new(TestBackend::new(200, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    super::event_search_tests::finish_search(&mut app);
    app.draw(&mut terminal).unwrap();
    assert_eq!(app.view.event_panel.len(), 1);
    let buffer = terminal.backend().buffer();
    let text = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Bash"), "details header missing: {text}");
    assert!(
        text.contains(".local/bin/cctail"),
        "details body missing: {text}"
    );
}
