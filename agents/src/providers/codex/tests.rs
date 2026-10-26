use super::*;
use crate::{model::SessionStatus, providers::SessionKey};
use rusqlite::Connection;

pub(super) fn fixture() -> (tempfile::TempDir, CodexProvider) {
    let dir = tempfile::tempdir().unwrap();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let provider = CodexProvider::new(dir.path().to_owned());
    (dir, provider)
}

#[test]
fn thread_clock_uses_latest_recorded_turn_without_changing_thread_birth() {
    let (dir, mut provider) = fixture();
    let path = dir.path().join("clock.jsonl");
    let records = include_str!("../../../tests/fixtures/codex-thread-clock.jsonl");
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
        let snapshot = provider.refresh(2);
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap();
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
            assert_eq!(
                session.status,
                if state == "pending" {
                    SessionStatus::NeedsInput
                } else {
                    SessionStatus::Inactive
                }
            );
            assert_eq!(session.ask.is_some(), state == "pending");
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
fn question_refresh_uses_rollout_without_a_daemon_connection() {
    let (dir, provider) = fixture();
    let path = dir.path().join("pending.jsonl");
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/codex-input/daemon-pending.jsonl"),
    )
    .unwrap();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path=? WHERE id='thread-0'",
            [path.to_str().unwrap()],
        )
        .unwrap();
    let control = dir.path().join("app-server-control");
    std::fs::create_dir(&control).unwrap();
    // A filesystem entry is present, but no daemon can accept a connection here.
    std::fs::write(control.join("app-server-control.sock"), b"").unwrap();
    for _ in 0..2 {
        let snapshot = provider.snapshot(1).unwrap();
        let session = snapshot
            .sessions
            .iter()
            .find(|session| session.id.id == "thread-0")
            .unwrap();
        let ask = session.ask.as_ref().expect("rollout question is available");
        assert_eq!(ask.id, "call_3d3e7995122343ac8f2f117588d23898");
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
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let mut opening = View::new(Grouping::Project, false);
        opening.set_capabilities(HashMap::from([(
            ProviderId::Codex,
            provider.capabilities(),
        )]));
        opening.set_sessions(vec![session.clone()], false);
        assert_eq!(
            opening.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            None
        );
        assert!(opening.pasting_prompt());
    }
    for (code, expected) in [
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
    let area = ratatui::layout::Rect::new(0, 0, 200, 45);
    view.render(area, &mut ratatui::buffer::Buffer::empty(area));
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        None
    );
    view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
    view.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
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
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            None
        );
        assert!(!view.pasting_prompt());
    }
    for (code, label) in [
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
        CodexProvider::new(path.clone())
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
    let mut provider = CodexProvider::new(dir.path().to_owned());
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
    // Parallel process launches can inherit this descriptor until exec finishes.
    writer.unlock().unwrap();
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
    let events = provider.event_panel(session, 100, false).unwrap();
    let bodies: Vec<_> = events.iter().map(|event| event.body.as_str()).collect();
    assert_eq!(bodies, ["Check the dashboard.", "Checks passed."]);
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
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, KeyModifiers::NONE)),
            None
        );
        assert!(!view.pasting_prompt());
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
fn names_follow_history_mode() {
    let (dir, provider) = fixture();
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
        ("paginated", "Preview", "Preview", None, None),
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
            .snapshot(2)
            .unwrap()
            .sessions
            .into_iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap();
        assert_eq!(session.name.as_deref(), expected, "{history} {title}");
        assert_eq!(session.display_title(), expected.unwrap_or("Preview"));
    }
    std::fs::remove_file(dir.path().join("session_index.jsonl")).unwrap();
    db.execute(
        "UPDATE threads SET title=first_user_message WHERE id='thread-12'",
        [],
    )
    .unwrap();
    assert_eq!(
        provider
            .snapshot(2)
            .unwrap()
            .sessions
            .into_iter()
            .find(|s| s.id.id == "thread-12")
            .unwrap()
            .name
            .as_deref(),
        None
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
fn sessions_load_while_codex_rewrites_the_model_cache() {
    let (dir, mut provider) = fixture();
    // Codex truncates models_cache.json before rewriting it in place.
    std::fs::write(dir.path().join("models_cache.json"), "").unwrap();
    assert!(!provider.refresh(1).sessions.is_empty());
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
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/codex-models-cache.json"),
    )
    .unwrap();
    let mut catalog = ModelCatalog::default();
    catalog.refresh(&path).unwrap();
    assert_eq!(catalog.choices.label("gpt-6.1-sol"), Some("GPT-6.1-Sol"));
    assert_eq!(catalog.choices.label("unknown-model"), None);
}

#[test]
fn lazy_discovery_does_not_parse_history_outside_the_page() {
    let (dir, provider) = fixture();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    // Duplicate a captured row without changing the upstream schema. Invalid optional
    // values outside the requested page expose eager row decoding, not just enrichment.
    db.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1600)
        INSERT INTO threads (id,rollout_path,created_at,updated_at,source,model_provider,cwd,title,sandbox_policy,approval_mode,model)
        SELECT 'history-'||x,rollout_path,created_at,1,source,model_provider,cwd,title,sandbox_policy,approval_mode,X'FF' FROM threads,n WHERE id='thread-0';").unwrap();
    let snapshot = provider
        .discover_sessions(1)
        .expect("old rows are never decoded");
    assert_eq!(snapshot.sessions.len(), PAGE_SIZE);
    assert!(snapshot.has_more);
    assert!(
        snapshot
            .sessions
            .iter()
            .all(|s| !s.id.id.starts_with("history-"))
    );
}

#[test]
fn lazy_discovery_counts_children_in_the_inactive_page() {
    let (dir, provider) = fixture();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute(
        "UPDATE threads SET updated_at_ms=9999999999999 WHERE id='thread-13'",
        [],
    )
    .unwrap();
    let snapshot = provider.discover_sessions(1).unwrap();
    assert_eq!(snapshot.sessions.len(), PAGE_SIZE);
    assert!(snapshot.sessions.iter().any(|s| s.id.id == "thread-13"));
    assert!(!snapshot.sessions.iter().any(|s| s.id.id == "thread-12"));
}

#[test]
fn lazy_history_budget_excludes_guardian_rows_before_limiting() {
    let (dir, provider) = fixture();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET source='vscode',updated_at_ms=9999999999999 WHERE id='thread-14'",
            [],
        )
        .unwrap();
    let snapshot = provider.discover_sessions(1).unwrap();
    assert_eq!(snapshot.sessions.len(), PAGE_SIZE);
    assert!(snapshot.sessions.iter().any(|s| s.id.id == "thread-9"));
}

#[test]
fn load_reads_requested_ids_outside_the_history_page_without_live_processes() {
    let (_dir, mut provider) = fixture();
    let snapshot = provider.load(&[
        key("thread-12".into()),
        key("missing".into()),
        SessionKey {
            provider: ProviderId::Claude,
            id: "thread-0".into(),
        },
    ]);
    assert!(!snapshot.has_more);
    assert!(snapshot.supervisions.is_empty());
    assert_eq!(snapshot.sessions.len(), 1);
    let session = &snapshot.sessions[0];
    assert_eq!(session.id, key("thread-12".into()));
    assert!(session.process.is_none());
    assert_eq!(session.status, SessionStatus::Inactive);
    assert!(provider.load(&[]).sessions.is_empty());
}

#[test]
fn load_missing_database_does_not_create_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = CodexProvider::new(dir.path().into());
    assert!(provider.load(&[key("missing".into())]).sessions.is_empty());
    assert!(!dir.path().join("state_5.sqlite").exists());
}

#[test]
fn hub_reports_visible_read_errors() {
    let (dir, provider) = fixture();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute(
        "DELETE FROM threads WHERE id NOT IN ('thread-0', 'thread-12', 'thread-13')",
        [],
    )
    .unwrap();
    db.execute_batch(
        "DELETE FROM thread_spawn_edges;
         INSERT INTO thread_spawn_edges VALUES ('thread-0', 'thread-13', 'open');",
    )
    .unwrap();
    let unreadable = dir.path().join("orphan.jsonl");
    std::fs::create_dir(&unreadable).unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=? WHERE id IN ('thread-0', 'thread-13')",
        [unreadable.to_str().unwrap()],
    )
    .unwrap();
    let visible = dir.path().join("visible.jsonl");
    std::fs::write(
        &visible,
        include_str!("../../../tests/fixtures/codex-latest-activity.jsonl"),
    )
    .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=? WHERE id='thread-12'",
        [visible.to_str().unwrap()],
    )
    .unwrap();

    let mut hub = crate::sessions::Hub::new(vec![Box::new(provider)]);
    assert!(hub.refresh().sessions.is_empty());
    db.execute(
        "UPDATE threads SET rollout_path=?",
        [visible.to_str().unwrap()],
    )
    .unwrap();
    let snapshot = hub.refresh();
    assert_eq!(snapshot.sessions.len(), 3);
    assert!(
        snapshot
            .sessions
            .iter()
            .find(|s| s.id.id == "thread-0")
            .unwrap()
            .parent
            .is_none()
    );

    db.execute(
        "UPDATE threads SET rollout_path=? WHERE id='thread-12'",
        [unreadable.to_str().unwrap()],
    )
    .unwrap();
    assert!(hub.refresh().sessions.is_empty());
}

#[test]
fn session_reads_pr_from_rollout() {
    let (dir, mut provider) = fixture();
    let path = dir.path().join("pr.jsonl");
    std::fs::write(
        &path,
        include_str!("../../../tests/fixtures/codex-pr-create.jsonl"),
    )
    .unwrap();
    Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path=? WHERE id='thread-0'",
            [path.to_str().unwrap()],
        )
        .unwrap();
    let snapshot = provider.refresh(1);
    let session = snapshot
        .sessions
        .iter()
        .find(|session| session.id.id == "thread-0")
        .unwrap();
    assert_eq!(
        session.pr,
        Some(crate::model::PullRequest {
            number: 850,
            url: "https://github.com/example/project/pull/850".into(),
        })
    );
}
