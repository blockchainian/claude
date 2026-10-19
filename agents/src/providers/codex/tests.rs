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
fn thread_clock_uses_latest_recorded_turn_without_changing_thread_birth() {
    let (dir, mut provider) = fixture();
    let path = dir.path().join("clock.jsonl");
    let records = include_str!("../../../tests/fixtures/codex-thread-clock.jsonl");
    let manager = dir.path().join("manager");
    std::fs::create_dir_all(manager.join("threads")).unwrap();
    std::fs::create_dir_all(manager.join("claude-parent")).unwrap();
    std::fs::write(
        manager.join("threads/thread-12.json"),
        include_str!("../../../tests/fixtures/codex-manager/attached-supervisor.json"),
    )
    .unwrap();
    std::fs::write(
        manager.join("claude-parent/state.json"),
        include_str!("../../../tests/fixtures/codex-manager/attached-state.json"),
    )
    .unwrap();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?, created_at_ms=1792218925141 WHERE id='thread-12'",
        [path.to_str().unwrap()],
    )
    .unwrap();
    for count in [1, 2, 3, 4] {
        std::fs::write(
            &path,
            records.lines().take(count).collect::<Vec<_>>().join("\n") + "\n",
        )
        .unwrap();
        let snapshot = provider.refresh(1);
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap();
        assert_eq!(
            session.parent,
            Some(SessionKey {
                provider: ProviderId::Claude,
                id: "claude-parent".into(),
            })
        );
        let expected = if count <= 2 {
            "2026-10-17T06:35:25.554Z"
        } else {
            "2026-10-17T15:46:56.983Z"
        };
        assert_eq!(
            session.prompt_at,
            Some(expected.parse().unwrap()),
            "{count}"
        );
        assert_eq!(
            session.started_at,
            DateTime::from_timestamp_millis(1792218925141)
        );
    }
}

#[test]
fn request_user_input_pending_answered_and_offline() {
    let (dir, mut provider) = fixture();
    let path = dir.path().join("rollout.jsonl");
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path=? WHERE id='thread-0'",
            [path.to_str().unwrap()],
        )
        .unwrap();
    for mode in ["default", "plan"] {
        for (state, expected) in [
            ("pending", SessionStatus::NeedsInput),
            ("answered", SessionStatus::Ready),
        ] {
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/codex-input/{mode}-{state}.jsonl"));
            std::fs::copy(fixture, &path).unwrap();
            let snapshot = provider.refresh(1);
            let mut session = snapshot
                .sessions
                .into_iter()
                .find(|session| session.id.id == "thread-0")
                .unwrap();
            assert_eq!(session.status, SessionStatus::Inactive);
            session.process = Some(Process {
                pid: std::process::id(),
                label: Some("codex".into()),
                started_at: None,
            });
            apply_rollout(&mut session, rollout::summary(&path).unwrap());
            assert_eq!(session.status, expected, "{mode}-{state}");
            assert_eq!(
                session.pending_questions,
                if state == "pending" {
                    vec!["Pick a color?"]
                } else {
                    vec![]
                }
            );
            if state == "pending" {
                assert_eq!(
                    session.latest_activity().unwrap().label,
                    "Request user input Color"
                );
            }
            session.process = None;
            apply_rollout(&mut session, rollout::summary(&path).unwrap());
            assert_eq!(session.status, SessionStatus::Inactive);
        }
    }
}

#[test]
fn archived_threads_are_listed_as_inactive() {
    let (dir, mut provider) = fixture();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute("UPDATE threads SET archived=1 WHERE id='thread-0'", [])
        .unwrap();
    let sessions = provider.refresh(2).sessions;
    assert_eq!(sessions.len(), 14);
    let archived = sessions.iter().find(|s| s.id.id == "thread-0").unwrap();
    assert!(archived.archived);
    assert_eq!(archived.status, SessionStatus::Inactive);
    assert!(sessions.iter().filter(|s| s.archived).count() == 1);
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
    let db = Connection::open(_dir.path().join("state_5.sqlite")).unwrap();
    let created: i64 = db
        .query_row(
            "SELECT created_at_ms FROM threads WHERE id = 'thread-0'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(session.started_at, DateTime::from_timestamp_millis(created));
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
    assert_eq!(
        provider.rename(session, "name").unwrap_err().to_string(),
        "Codex's app-server daemon isn't running"
    );
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
            Action::Fork(session.id.clone()),
            vec!["fork", session.id.id.as_str()],
        ),
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
    for parent in [key("parent".into())] {
        session.parent = Some(parent);
        for action in [
            Action::Resume(session.id.clone()),
            Action::Fork(session.id.clone()),
        ] {
            assert!(provider.launch(&action, Some(&session)).is_none());
        }
    }
    session.parent = None;
    assert!(provider.launch(&Action::Quit, Some(&session)).is_none());
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
            delete: true,
            rename: true,
            stop: true,
            archive: true,
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
        (KeyCode::Enter, Action::OpenTerminal(session.id.clone())),
        (KeyCode::Char('o'), Action::OpenTerminal(session.id.clone())),
        (KeyCode::Char('f'), Action::Fork(session.id.clone())),
        (
            KeyCode::Char('n'),
            Action::NewDraft {
                cwd: session.cwd.clone(),
            },
        ),
        (KeyCode::Char('x'), Action::Stop(session.id.clone())),
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Some(expected)
        );
    }
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        None
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Action::Rename {
            id: session.id.clone(),
            name: session.display_title().into()
        })
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        None
    );
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Action::Delete(session.id.clone()))
    );
    let mut parent = session.clone();
    parent.id = key("parent".into());
    parent.status = SessionStatus::Working;
    session.parent = Some(parent.id.clone());
    view.set_sessions(vec![parent, session.clone()], false);
    assert_eq!(view.selected_id(), Some(&session.id));
    for (code, name) in [(KeyCode::Enter, "Terminal"), (KeyCode::Char('o'), "Terminal")] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Some(Action::Unsupported(name))
        );
    }
    for (code, label) in [
        (KeyCode::Char('p'), "Pin"),
        (KeyCode::Char('f'), "Fork"),
        (KeyCode::Char('r'), "Rename"),
        (KeyCode::Backspace, "Delete"),
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Some(Action::Unsupported(label))
        );
    }
}

#[test]
fn delete_rejects_children_and_requires_the_daemon() {
    let (_dir, provider) = fixture();
    let mut session = provider.snapshot(1).unwrap().sessions.remove(0);
    session.parent = Some(key("parent".into()));
    assert_eq!(
        provider
            .delete(&session)
            .and_then(|delete| delete())
            .unwrap_err()
            .kind(),
        io::ErrorKind::Unsupported
    );
    session.parent = None;
    session.process = Some(Process {
        pid: std::process::id(),
        label: Some("codex".into()),
        started_at: None,
    });
    assert_eq!(
        provider
            .delete(&session)
            .and_then(|delete| delete())
            .unwrap_err()
            .to_string(),
        "Codex's app-server daemon isn't running"
    );
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
    assert!(session.activity.is_none());
    let events = provider.log(session, 100, false).unwrap();
    let bodies: Vec<_> = events.iter().map(|event| event.body.as_str()).collect();
    assert_eq!(bodies, ["Check the dashboard.", "Checks passed."]);
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
fn archived_manager_threads_still_need_detach() {
    let (dir, mut provider) = fixture();
    manager_fixture(dir.path(), &mut provider, "attached");
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute("UPDATE threads SET archived=1 WHERE id='thread-12'", [])
        .unwrap();
    let snapshot = provider.snapshot(2).unwrap();
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id.id == "thread-12")
        .unwrap();
    assert!(session.archived && session.parent.is_some());
    assert_eq!(
        provider.archive(session).unwrap()().unwrap_err().to_string(),
        "Codex's app-server daemon isn't running"
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
fn manager_children_use_shared_hub_filtering_and_resumable_rows() {
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
    // Descendants follow their inactive root into the Inactive tab.
    assert_eq!(view.selected_id(), None);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
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
        !buffer[(x as u16, parent_row as u16)]
            .modifier
            .contains(Modifier::DIM)
    );
    assert!(lines[parent_row + 1].contains("Managed worker"));
    for (code, expected) in [
        (KeyCode::Enter, Action::OpenTerminal(key("thread-12".into()))),
        (KeyCode::Right, Action::OpenTerminal(key("thread-12".into()))),
        (
            KeyCode::Char('o'),
            Action::OpenTerminal(key("thread-12".into())),
        ),
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Some(expected)
        );
    }
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x as u16,
        row: parent_row as u16 + 1,
        modifiers: KeyModifiers::NONE,
    };
    let release = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..click
    };
    assert_eq!(view.handle_mouse(click), None);
    assert_eq!(view.handle_mouse(release), None);
    assert_eq!(view.handle_mouse(click), None);
    assert_eq!(view.handle_mouse(release), None);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        None
    );
    assert_eq!(view.selected_id(), Some(&parent_key));
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(Action::OpenTerminal(parent_key.clone()))
    );
    view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(view.selected_id(), Some(&key("thread-12".into())));
    for (code, expected) in [
        (KeyCode::Char('f'), Action::Fork(key("thread-12".into()))),
        (KeyCode::Char('x'), Action::Stop(key("thread-12".into()))),
        (
            KeyCode::Char('p'),
            Action::TogglePin {
                id: parent_key.clone(),
                pinned: true,
            },
        ),
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Some(expected)
        );
        assert_eq!(view.selected_id(), Some(&key("thread-12".into())));
    }
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
        None
    );
    view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    // Pending and inactive children never populate the parent's Background section.
    while view.selected_id() != Some(&parent_key) {
        let previous = view.selected_id().cloned();
        view.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_ne!(view.selected_id(), previous.as_ref());
    }
    assert!(!view.has_background());
}

#[test]
fn manager_children_launch_like_ordinary_threads() {
    let (dir, mut provider) = fixture();
    manager_fixture(dir.path(), &mut provider, "attached");
    let mut session = provider
        .snapshot(0)
        .unwrap()
        .sessions
        .into_iter()
        .find(|session| session.id.id == "thread-12")
        .unwrap();
    session.cwd = dir.path().into();
    let command = provider
        .launch(&Action::Resume(session.id.clone()), Some(&session))
        .unwrap();
    assert_eq!(command.get_program(), "codex");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["resume", "thread-12"]
    );
    assert_eq!(command.get_current_dir(), Some(dir.path()));
    let command = provider
        .launch(&Action::Fork(session.id.clone()), Some(&session))
        .unwrap();
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["fork", "thread-12"]
    );
    assert_eq!(command.get_current_dir(), Some(dir.path()));
}

#[test]
fn spawn_children_cannot_resume_in_the_view() {
    use crate::{model::Grouping, ui::View};
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{buffer::Buffer, layout::Rect};
    let (_dir, mut provider) = fixture();
    let sessions = provider.refresh(2).sessions;
    let mut child = sessions
        .iter()
        .find(|session| session.id.id == "thread-13")
        .unwrap()
        .clone();
    let mut parent = sessions
        .iter()
        .find(|session| session.id.id == "thread-12")
        .unwrap()
        .clone();
    parent.status = SessionStatus::Working;
    child.status = SessionStatus::Ready;
    let id = child.id.clone();
    let mut view = View::new(Grouping::Project, false);
    view.set_capabilities(HashMap::from([(
        ProviderId::Codex,
        provider.capabilities(),
    )]));
    view.set_sessions(vec![parent, child], false);
    assert_eq!(view.selected_id(), Some(&id));
    for (code, name) in [(KeyCode::Enter, "Terminal"), (KeyCode::Char('o'), "Terminal")] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Some(Action::Unsupported(name))
        );
    }
    let mut buffer = Buffer::empty(Rect::new(0, 0, 160, 40));
    view.render(buffer.area, &mut buffer);
    let (column, row) = view.marker_pos().unwrap();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let release = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..click
    };
    assert_eq!(view.handle_mouse(click), None);
    assert_eq!(view.handle_mouse(release), None);
    assert_eq!(view.handle_mouse(click), None);
    assert_eq!(view.handle_mouse(release), None);
}

#[test]
fn names_follow_history_mode_and_manager_precedence() {
    let (dir, mut provider) = fixture();
    manager_fixture(dir.path(), &mut provider, "attached");
    // Rows use the captured state schema; index lines use the live rename PoC shape.
    std::fs::write(
        dir.path().join("session_index.jsonl"),
        include_str!("../../../tests/fixtures/codex-names.jsonl"),
    )
    .unwrap();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    for (history, title, first, name, expected) in [
        (
            "paginated",
            "Preview",
            "Preview",
            Some("  Codex name  "),
            Some("Codex name"),
        ),
        (
            "paginated",
            "Preview",
            "Preview",
            None,
            Some("Managed worker"),
        ),
        (
            "legacy",
            "Preview",
            "Preview",
            Some("Ignored column"),
            Some("Index name"),
        ),
        (
            "legacy",
            "  Metadata name  ",
            "Preview",
            None,
            Some("Metadata name"),
        ),
        ("legacy", "   ", "Preview", None, Some("Index name")),
        ("legacy", " Preview ", " Preview ", None, Some("Index name")),
    ] {
        db.execute("UPDATE threads SET history_mode=?1,title=?2,first_user_message=?3,name=?4 WHERE id='thread-12'", rusqlite::params![history,title,first,name]).unwrap();
        let session = provider
            .snapshot(0)
            .unwrap()
            .sessions
            .into_iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap();
        assert_eq!(session.name.as_deref(), expected, "{history} {title}");
        assert_eq!(session.display_title(), expected.unwrap());
    }
    std::fs::remove_file(dir.path().join("session_index.jsonl")).unwrap();
    db.execute(
        "UPDATE threads SET title=first_user_message WHERE id='thread-12'",
        [],
    )
    .unwrap();
    assert_eq!(
        provider
            .snapshot(0)
            .unwrap()
            .sessions
            .into_iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap()
            .name
            .as_deref(),
        Some("Managed worker")
    );
}

#[test]
fn file_change_without_start_preserves_session_list() {
    let (dir, mut provider) = fixture();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codex-inner/file-change-without-start.jsonl");
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path=? WHERE id='thread-0'",
            [path.to_str().unwrap()],
        )
        .unwrap();
    let snapshot = provider.refresh(2);
    assert_eq!(snapshot.sessions.len(), 14);
    let session = snapshot
        .sessions
        .iter()
        .find(|s| s.id.id == "thread-0")
        .unwrap();
    assert_eq!(
        session.activity.as_ref().unwrap().label,
        "Apply patch /workspace/project/design.md"
    );
}

#[test]
fn manager_thread_operations_use_the_provider_paths() {
    let (dir, mut provider) = fixture();
    manager_fixture(dir.path(), &mut provider, "attached");
    let mut session = provider
        .snapshot(0)
        .unwrap()
        .sessions
        .into_iter()
        .find(|session| session.id.id == "thread-12")
        .unwrap();
    // No daemon is present in this captured fixture; reaching it reports a connection error.
    for result in [
        provider.rename(&session, "probe"),
        provider.archive(&session).unwrap()(),
        provider.send(&session, SessionInput::Interrupt).unwrap()(),
    ] {
        assert_ne!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
    session.process = Some(Process {
        pid: std::process::id(),
        label: Some("codex".into()),
        started_at: None,
    });
    assert_eq!(
        provider
            .delete(&session)
            .and_then(|delete| delete())
            .unwrap_err()
            .to_string(),
        "Codex's app-server daemon isn't running"
    );
}

#[test]
fn model_catalog_labels_refresh_only_when_modified() {
    let (dir, mut provider) = fixture();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute("UPDATE threads SET model='gpt-6.1-sol'", [])
        .unwrap();
    let label = |provider: &mut CodexProvider| {
        let session = provider.refresh(1).sessions.remove(0);
        provider
            .input_choices()
            .label(session.model.as_deref().unwrap())
            .map(str::to_owned)
    };
    assert_eq!(label(&mut provider), None);
    let path = dir.path().join("models_cache.json");
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/codex-models-cache.json"),
    )
    .unwrap();
    assert_eq!(label(&mut provider).as_deref(), Some("GPT-6.1-Sol"));
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    // Invalid content would fail parsing if an unchanged file were read again.
    std::fs::write(&path, "invalid").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(label(&mut provider).as_deref(), Some("GPT-6.1-Sol"));
    let mut catalog: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/codex-models-cache.json"
    ))
    .unwrap();
    catalog["models"].as_array_mut().unwrap().remove(0);
    std::fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new().set_modified(modified + std::time::Duration::from_secs(1)),
        )
        .unwrap();
    assert_eq!(label(&mut provider), None);
    db.execute("UPDATE threads SET model='gpt-6-astra'", [])
        .unwrap();
    assert_eq!(label(&mut provider).as_deref(), Some("GPT-6-Astra"));
    std::fs::remove_file(path).unwrap();
    assert_eq!(label(&mut provider), None);
}

#[test]
fn input_choices_list_cached_models_by_priority_with_their_efforts() {
    let (dir, provider) = fixture();
    assert_eq!(
        provider.input_choices(),
        crate::providers::InputChoices::default()
    );
    std::fs::write(
        dir.path().join("models_cache.json"),
        include_str!("../../../tests/fixtures/codex-models-cache.json"),
    )
    .unwrap();
    let choices = provider.input_choices();
    assert_eq!(
        choices
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol"]
    );
    let model = choices.model("gpt-6.1-sol").unwrap();
    assert_eq!(
        model.efforts,
        ["low", "medium", "high", "xhigh", "max", "ultra"]
    );
    assert_eq!(model.default_effort.as_deref(), Some("low"));
}

#[test]
fn collaboration_and_session_models_share_cached_display_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("models_cache.json");
    std::fs::write(&path, include_str!("../../../tests/fixtures/codex-models-cache.json")).unwrap();
    let mut catalog = ModelCatalog::default();
    catalog.refresh(&path).unwrap();
    assert_eq!(catalog.choices.label("gpt-6.1-sol"), Some("GPT-6.1-Sol"));
    assert_eq!(catalog.choices.label("unknown-model"), None);
}

#[test]
fn native_agent_title_keeps_its_recorded_path_when_a_manager_link_names_it() {
    let (dir, mut provider) = fixture();
    let meta: serde_json::Value = serde_json::from_str(
        include_str!("../../../tests/fixtures/codex-subagents/rollout.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let manager = dir.path().join("manager");
    std::fs::create_dir_all(manager.join("threads")).unwrap();
    std::fs::create_dir_all(manager.join("claude-parent")).unwrap();
    std::fs::write(
        manager.join("threads/thread-12.json"),
        include_str!("../../../tests/fixtures/codex-manager/attached-supervisor.json"),
    )
    .unwrap();
    std::fs::write(
        manager.join("claude-parent/state.json"),
        include_str!("../../../tests/fixtures/codex-manager/attached-state.json"),
    )
    .unwrap();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute(
        "UPDATE threads SET source=?, name=NULL WHERE id='thread-12'",
        [meta["payload"]["source"].to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_spawn_edges VALUES ('thread-0','thread-12','running')",
        [],
    )
    .unwrap();
    let sessions = provider.refresh(1).sessions;
    let child = sessions
        .iter()
        .find(|session| session.id.id == "thread-12")
        .unwrap();
    assert_eq!(child.display_title(), "provider_adapters");
    assert_eq!(child.name.as_deref(), Some("Managed worker"));
}
