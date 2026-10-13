use super::*;
use crate::{model::SessionStatus, providers::SessionKey};
use rusqlite::Connection;

fn fixture() -> (tempfile::TempDir, CodexProvider) {
    let dir = tempfile::tempdir().unwrap();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let provider = CodexProvider::new(dir.path().to_owned(), dir.path().join("manager"));
    (dir, provider)
}

#[test]
fn reads_named_columns_and_pages_inactive_threads() {
    let (_dir, mut provider) = fixture();
    let first = provider.refresh(1);
    assert_eq!(first.sessions.len(), 10);
    assert!(first.has_more);
    let all = provider.refresh(2);
    assert!(!all.has_more);
    assert_eq!(all.sessions.len(), 14);
    let session = all.sessions.iter().find(|s| s.id.id == "thread-0").unwrap();
    assert_eq!(session.id.provider, ProviderId::Codex);
    assert_eq!(session.cwd, PathBuf::from("/workspace/project"));
    assert_eq!(session.title, "Example task 0");
    assert_eq!(session.prompt, "Inspect the project");
    assert_eq!(session.git_branch.as_deref(), Some("example-branch"));
    assert!(session.model.is_some());
    assert!(session.started_at.is_some());
    assert_eq!(session.status, SessionStatus::Inactive);
    let child = all
        .sessions
        .iter()
        .find(|s| s.id.id == "thread-13")
        .unwrap();
    assert_eq!(
        child.parent,
        Some(SessionKey {
            provider: ProviderId::Codex,
            id: "thread-12".into()
        })
    );
    assert!(provider.rename(session, "name").is_err());
    assert!(provider.delete(session).is_err());
}

#[test]
fn launches_top_level_threads_in_their_cwd() {
    let (dir, mut provider) = fixture();
    let mut session = provider.refresh(1).sessions.remove(0);
    session.cwd = dir.path().into();
    for (action, args) in [
        (
            Action::Resume(session.id.clone()),
            vec!["resume", session.id.id.as_str()],
        ),
        (
            Action::Open(session.id.clone()),
            vec!["resume", session.id.id.as_str()],
        ),
        (Action::Back, vec!["resume", session.id.id.as_str()]),
        (
            Action::Fork(session.id.clone()),
            vec!["fork", session.id.id.as_str()],
        ),
        (Action::New(Some(session.cwd.clone())), vec![]),
    ] {
        let command = provider.launch(&action, Some(&session)).unwrap();
        assert_eq!(command.get_program(), "codex");
        assert_eq!(command.get_args().collect::<Vec<_>>(), args);
        assert_eq!(command.get_current_dir(), Some(dir.path()));
    }
}

#[test]
fn launch_rejects_children_missing_sessions_and_missing_cwd() {
    let (dir, mut provider) = fixture();
    let mut session = provider.refresh(1).sessions.remove(0);
    session.cwd = dir.path().into();
    for action in [
        Action::Resume(session.id.clone()),
        Action::Fork(session.id.clone()),
    ] {
        assert!(provider.launch(&action, None).is_none());
        session.cwd = dir.path().join("missing");
        assert!(provider.launch(&action, Some(&session)).is_none());
        session.cwd = dir.path().into();
    }
    for parent in [
        key("parent".into()),
        SessionKey {
            provider: ProviderId::Claude,
            id: "parent".into(),
        },
    ] {
        session.parent = Some(parent);
        for action in [
            Action::Resume(session.id.clone()),
            Action::Fork(session.id.clone()),
            Action::Open(session.id.clone()),
            Action::Back,
            Action::New(Some(session.cwd.clone())),
        ] {
            assert!(provider.launch(&action, Some(&session)).is_none());
        }
    }
    session.parent = None;
    assert!(provider.launch(&Action::Quit, Some(&session)).is_none());
}

#[test]
fn new_session_uses_requested_cwd_or_current_directory() {
    let (dir, provider) = fixture();
    let current = std::env::current_dir().unwrap();
    for (cwd, expected) in [
        (Some(dir.path().to_owned()), dir.path().to_owned()),
        (None, current.clone()),
        (Some(dir.path().join("missing")), current),
    ] {
        let command = provider.launch(&Action::New(cwd), None).unwrap();
        assert_eq!(command.get_program(), "codex");
        assert_eq!(command.get_args().count(), 0);
        assert_eq!(command.get_current_dir(), Some(expected.as_path()));
    }
}

#[test]
fn capabilities_gate_codex_actions_in_the_view() {
    use crate::{model::Grouping, ui::View};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (_dir, mut provider) = fixture();
    assert_eq!(
        provider.capabilities(),
        Capabilities {
            resume: true,
            fork: true,
            new: true,
            ..Capabilities::default()
        }
    );
    let mut session = provider.refresh(1).sessions.remove(0);
    session.status = SessionStatus::Ready;
    let mut view = View::new(Grouping::Project, false);
    view.set_capabilities(HashMap::from([(
        ProviderId::Codex,
        provider.capabilities(),
    )]));
    view.set_sessions(vec![session.clone()], false);
    for (code, expected) in [
        (KeyCode::Enter, Action::Open(session.id.clone())),
        (KeyCode::Char('o'), Action::Resume(session.id.clone())),
        (KeyCode::Char('f'), Action::Fork(session.id.clone())),
        (KeyCode::Char('n'), Action::New(Some(session.cwd.clone()))),
        (KeyCode::Char('r'), Action::Unsupported("Rename")),
        (KeyCode::Backspace, Action::Unsupported("Delete")),
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Some(expected)
        );
    }
    let mut parent = session.clone();
    parent.id = key("parent".into());
    parent.status = SessionStatus::Inactive;
    session.parent = Some(parent.id.clone());
    view.set_sessions(vec![parent, session.clone()], false);
    assert_eq!(view.selected_id(), Some(&session.id));
    for code in [
        KeyCode::Enter,
        KeyCode::Char('o'),
        KeyCode::Char('f'),
        KeyCode::Char('r'),
        KeyCode::Backspace,
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            None
        );
    }
}

#[test]
fn missing_home_is_empty_without_creating_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent");
    assert!(
        CodexProvider::new(path.clone(), dir.path().join("manager"))
            .refresh(1)
            .sessions
            .is_empty()
    );
    assert!(!path.exists());
}

#[test]
fn missing_optional_columns_degrade_and_database_stays_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state_5.sqlite");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE threads (id TEXT, cwd TEXT, title TEXT, updated_at INTEGER); INSERT INTO threads VALUES ('minimal','/workspace','Task',1791911000);").unwrap();
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let mut provider = CodexProvider::new(dir.path().to_owned(), dir.path().join("manager"));
    let snap = provider.refresh(1);
    assert_eq!(snap.sessions.len(), 1);
    assert_eq!(snap.sessions[0].title, "Task");
    assert_eq!(snap.sessions[0].updated_at.timestamp(), 1791911000);
    assert_eq!(snap.sessions[0].model, None);
    assert_eq!(before, std::fs::read(path).unwrap());
}

#[test]
fn locks_require_a_holder_not_just_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("thread.lock");
    assert!(!lock_held(&path).unwrap());
    let writer = std::fs::File::create(&path).unwrap();
    assert!(!lock_held(&path).unwrap());
    writer.lock().unwrap();
    assert!(lock_held(&path).unwrap());
    drop(writer);
    assert!(!lock_held(&path).unwrap());
    assert_eq!(status(false, false), SessionStatus::Inactive);
    assert_eq!(status(false, true), SessionStatus::Inactive);
    assert_eq!(status(true, false), SessionStatus::Working);
    assert_eq!(status(true, true), SessionStatus::Ready);
}

#[test]
fn rollout_details_are_read_through_the_provider() {
    let (dir, mut provider) = fixture();
    let path = dir.path().join("rollout.jsonl");
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/codex-rollout.jsonl"),
    )
    .unwrap();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path=?",
            [path.to_str().unwrap()],
        )
        .unwrap();
    let snapshot = provider.refresh(1);
    let session = &snapshot.sessions[0];
    assert_eq!(session.last_message.as_deref(), Some("Checks passed."));
    assert!(session.last_record_at.is_some());
    assert!(session.activity.is_some());
    let events = provider
        .log(session, 100, &log::Filter::default(), false)
        .unwrap();
    assert_eq!(events.len(), 3);
}

fn manager_fixture(dir: &Path, provider: &mut CodexProvider, kind: &str) -> PathBuf {
    let root = dir.join("manager");
    std::fs::create_dir_all(root.join("threads")).unwrap();
    let session = root.join("claude-parent");
    std::fs::create_dir_all(&session).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-manager");
    let id = if kind == "attached" {
        "thread-12"
    } else {
        "thread-0"
    };
    std::fs::copy(
        fixtures.join(format!("{kind}-supervisor.json")),
        root.join("threads").join(format!("{id}.json")),
    )
    .unwrap();
    std::fs::copy(
        fixtures.join(format!("{kind}-state.json")),
        session.join("state.json"),
    )
    .unwrap();
    provider.manager_home = root;
    session
}

#[test]
fn manager_children_bypass_codex_paging_and_keep_descendants() {
    let (dir, mut provider) = fixture();
    manager_fixture(dir.path(), &mut provider, "attached");
    let snapshot = provider.snapshot(0).unwrap();
    assert_eq!(snapshot.sessions.len(), 2);
    let child = snapshot
        .sessions
        .iter()
        .find(|s| s.id.id == "thread-12")
        .unwrap();
    assert_eq!(
        child.parent,
        Some(SessionKey {
            provider: ProviderId::Claude,
            id: "claude-parent".into()
        })
    );
    assert_eq!(child.name.as_deref(), Some("Managed worker"));
    assert_eq!(child.status, SessionStatus::Inactive);
    assert!(
        provider
            .watch_roots()
            .unwrap()
            .contains(&provider.manager_home)
    );
}

#[test]
fn manager_waiting_and_asks_override_rollout_status_and_clear() {
    let (dir, mut provider) = fixture();
    let session = manager_fixture(dir.path(), &mut provider, "attached");
    let path = session.join("state.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    // No pending requests existed at capture; exercise the manager's documented disk transitions.
    state["threads"][0]["waiting"] = serde_json::json!([{"callId":"approval-1","kind":"approval","since":1791911000000_i64,"text":"Run checks"}]);
    std::fs::write(&path, state.to_string()).unwrap();
    assert_eq!(
        provider
            .snapshot(0)
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap()
            .status,
        SessionStatus::NeedsInput
    );
    state["threads"][0]["waiting"] = serde_json::json!([]);
    std::fs::write(&path, state.to_string()).unwrap();
    let ask = session.join("thread-12.ask.question-1.json");
    std::fs::write(
        &ask,
        r#"{"callId":"question-1","text":"Continue?","since":1791911000000}"#,
    )
    .unwrap();
    assert_eq!(
        provider
            .snapshot(0)
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap()
            .status,
        SessionStatus::NeedsInput
    );
    std::fs::remove_file(ask).unwrap();
    assert_eq!(
        provider
            .snapshot(0)
            .unwrap()
            .sessions
            .iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap()
            .status,
        SessionStatus::Inactive
    );
}

#[test]
fn manager_detached_missing_and_reassigned_records_do_not_nest() {
    let (dir, mut provider) = fixture();
    let session = manager_fixture(dir.path(), &mut provider, "detached");
    let snapshot = provider.snapshot(1).unwrap();
    assert!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "thread-0")
            .unwrap()
            .parent
            .is_none()
    );
    let session = {
        std::fs::remove_file(session.join("state.json")).unwrap();
        manager_fixture(dir.path(), &mut provider, "attached")
    };
    std::fs::write(
        provider.manager_home.join("threads/thread-12.json"),
        r#"{"sessionId":"another-session","pid":12345}"#,
    )
    .unwrap();
    assert!(provider.snapshot(0).unwrap().sessions.is_empty());
    std::fs::remove_file(provider.manager_home.join("threads/thread-12.json")).unwrap();
    assert!(provider.snapshot(0).unwrap().sessions.is_empty());
    assert!(session.join("state.json").exists());
}

#[test]
fn manager_children_use_shared_hub_filtering_and_view_only_rows() {
    use crate::{model::Grouping, providers::claude::ClaudeProvider, sessions::Hub, ui::View};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};

    let (dir, mut codex) = fixture();
    let manager_session = manager_fixture(dir.path(), &mut codex, "attached");
    let claude_home = dir.path().join("claude");
    let project = claude_home.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let parent_id = "00000000-0000-0000-0000-000000000001";
    std::fs::write(
        project.join(format!("{parent_id}.jsonl")),
        include_str!("../../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    std::fs::rename(&manager_session, codex.manager_home.join(parent_id)).unwrap();
    std::fs::write(
        codex.manager_home.join("threads/thread-12.json"),
        serde_json::json!({"sessionId":parent_id,"pid":12345}).to_string(),
    )
    .unwrap();
    let session = codex.manager_home.join(parent_id);
    std::fs::write(
        session.join("thread-12.ask.question.json"),
        r#"{"callId":"question","text":"Continue?","since":1791911000000}"#,
    )
    .unwrap();

    let mut orphan_hub = Hub::new(vec![Box::new(CodexProvider::new(
        dir.path().into(),
        codex.manager_home.clone(),
    ))]);
    assert!(
        orphan_hub
            .refresh()
            .sessions
            .iter()
            .all(|s| s.id.id != "thread-12" && s.id.id != "thread-13")
    );
    let mut hub = Hub::new(vec![
        Box::new(ClaudeProvider::new(claude_home)),
        Box::new(codex),
    ]);
    let snapshot = hub.refresh();
    let parent = snapshot
        .sessions
        .iter()
        .find(|s| s.id.provider == ProviderId::Claude)
        .unwrap();
    let parent_key = parent.id.clone();
    let parent_title = parent.display_title().chars().take(12).collect::<String>();
    assert_eq!(parent.status, SessionStatus::Inactive);
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|s| s.id.id == "thread-12" && s.parent.as_ref() == Some(&parent_key))
    );
    let mut view = View::new(Grouping::Project, false);
    view.set_capabilities(hub.capabilities());
    view.set_sessions(snapshot.sessions, snapshot.has_more);
    assert_eq!(view.selected_id(), Some(&key("thread-12".into())));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 40));
    view.render(buffer.area, &mut buffer);
    let lines: Vec<String> = buffer
        .content
        .chunks(160)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    let parent_row = lines
        .iter()
        .position(|line| line.contains(&parent_title))
        .unwrap_or_else(|| panic!("parent={parent_title:?}\n{}", lines.join("\n")));
    let x = 6;
    assert!(
        buffer[(x as u16, parent_row as u16)]
            .modifier
            .contains(Modifier::DIM)
    );
    assert!(lines[parent_row + 1].contains("Managed worker"));
    for code in [
        KeyCode::Enter,
        KeyCode::Right,
        KeyCode::Char('o'),
        KeyCode::Char('r'),
        KeyCode::Char('x'),
        KeyCode::Char('p'),
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            None
        );
        assert_eq!(view.selected_id(), Some(&key("thread-12".into())));
    }
    // Pending and inactive children never populate the parent's Background section.
    for _ in 0..4 {
        view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    }
    while view.selected_id() != Some(&parent_key) {
        let previous = view.selected_id().cloned();
        view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_ne!(view.selected_id(), previous.as_ref());
    }
    assert!(!view.has_background());
}
