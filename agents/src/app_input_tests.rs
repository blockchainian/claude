use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::fs;

// Registry fields come from the captured registry payload; PID, cwd and process
// start describe this test process so the production liveness reader accepts it.
fn registry_app(kind: &str) -> (tempfile::TempDir, App, PathBuf, serde_json::Value) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let project = root.join("projects/project");
    fs::create_dir_all(&project).unwrap();
    let mut registry: serde_json::Value =
        serde_json::from_str(include_str!("../tests/fixtures/registry.json")).unwrap();
    let id = registry["sessionId"].as_str().unwrap().to_owned();
    fs::write(
        project.join(format!("{id}.jsonl")),
        include_bytes!("../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let pid = std::process::id();
    let start = crate::processes::start_time(pid).expect("read test process start time");
    registry["pid"] = pid.into();
    registry["procStart"] = start.format("%a %b %e %H:%M:%S %Y").to_string().into();
    registry["cwd"] = root.to_str().unwrap().into();
    registry["kind"] = kind.into();
    registry["jobId"] = "e772d00b".into();
    fs::create_dir_all(root.join("sessions")).unwrap();
    let path = root.join(format!("sessions/{pid}.json"));
    fs::write(&path, serde_json::to_vec(&registry).unwrap()).unwrap();
    let job = root.join("jobs/e772d00b");
    fs::create_dir_all(&job).unwrap();
    // This state is a redacted real Claude Code 2.1.299 payload, with its
    // session ID replaced to point at the captured transcript above.
    let mut state: serde_json::Value = serde_json::from_str(r#"{"state":"done","template":"bg","sessionId":"e772d00b-c424-4632-a8df-74bc9c7b1629","respawnFlags":[],"backend":"daemon"}"#).unwrap();
    state["sessionId"] = id.into();
    fs::write(job.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    let app = open_app(root);
    (directory, app, path, registry)
}

fn open_app(root: &std::path::Path) -> App {
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(root.to_owned()),
        )]),
        root.to_owned(),
    );
    app.refresh();
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
    app.refresh_event(8);
    super::event_search_tests::finish_search(&mut app);
    app
}

fn pin_first_source_event(app: &mut App, id: &SessionKey) -> PinnedEvent {
    let event = app
        .view
        .event_panel
        .iter()
        .find(|event| event.source.is_some() && !event.source_records.is_empty())
        .expect("a pinnable event")
        .clone();
    let pin = PinnedEvent {
        source: event.source.clone().unwrap(),
        records: event.source_records.clone(),
    };
    app.handle_action(Some(Action::ToggleEventPin {
        task: id.clone(),
        pinned: vec![pin.clone()],
    }));
    pin
}

#[test]
fn toggling_an_event_pin_saves_the_task_pins() {
    let (directory, mut app, _, _) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    let pin = pin_first_source_event(&mut app, &id);
    assert_eq!(
        Preferences::load(directory.path()).task_pinned_events(&id),
        [pin]
    );
    app.handle_action(Some(Action::ToggleEventPin {
        task: id.clone(),
        pinned: Vec::new(),
    }));
    assert!(
        Preferences::load(directory.path())
            .task_pinned_events(&id)
            .is_empty()
    );
}

#[test]
fn stored_pins_are_read_back_when_the_task_opens() {
    let (directory, mut app, _, _) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    let pin = pin_first_source_event(&mut app, &id);
    drop(app);
    let app = open_app(directory.path());
    assert!(app.view.has_pinned_events(&id));
    assert_eq!(
        Preferences::load(directory.path()).task_pinned_events(&id),
        [pin]
    );
}

#[test]
fn stored_pins_that_no_longer_match_their_records_are_forgotten() {
    let (directory, mut app, _, _) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    let mut pin = pin_first_source_event(&mut app, &id);
    pin.source = "not-the-recorded-source".into();
    app.handle_action(Some(Action::ToggleEventPin {
        task: id.clone(),
        pinned: vec![pin],
    }));
    drop(app);
    let app = open_app(directory.path());
    assert!(!app.view.has_pinned_events(&id));
    assert!(
        Preferences::load(directory.path())
            .task_pinned_events(&id)
            .is_empty()
    );
}

#[test]
fn inactive_sessions_lose_their_stored_state() {
    let (directory, mut app, registry, _) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    pin_first_source_event(&mut app, &id);
    let saved =
        app.preferences
            .save_task_sort(directory.path(), &id, crate::model::EventSort::Duration);
    saved.unwrap();
    fs::remove_file(registry).unwrap();
    fs::remove_file(directory.path().join("jobs/e772d00b/state.json")).unwrap();
    app.refresh();
    assert_eq!(
        app.session(&id).unwrap().status,
        crate::model::SessionStatus::Inactive
    );
    let stored = Preferences::load(directory.path());
    assert!(stored.task_pinned_events(&id).is_empty());
    assert!(stored.tasks.is_empty());
    assert!(!app.view.has_pinned_events(&id));
}

fn screen(app: &mut App) -> String {
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(240, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn complete_input(app: &mut App, expected: &str) {
    let event = loop {
        let input = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        if let Input::EventsLoaded {
            token,
            reader,
            result,
            pinned,
        } = input
        {
            app.finish_event_load(token, reader, result, pinned);
        } else {
            break input;
        }
    };
    assert!(
        matches!(&event, Input::Sent { result: Err(error), .. } if error.to_string().contains(expected)),
        "unexpected provider completion"
    );
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(app.pending_inputs.is_empty());
    let rendered = screen(app);
    assert!(rendered.contains(expected), "{rendered}");
    assert!(!rendered.contains(" · sending"), "{rendered}");
}

#[test]
fn enter_prompt_reaches_background_provider_and_displays_identity_failure() {
    let (_directory, mut app, path, mut registry) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    let session = app.session(&id).unwrap();
    assert_eq!(
        session.process.as_ref().unwrap().label.as_deref(),
        Some("bg")
    );
    assert_eq!(session.background_job.as_ref().unwrap().id, "e772d00b");
    assert!(session.transcript.as_ref().unwrap().is_file());
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(240, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    let buffer = terminal.backend().buffer();
    let prompt_y = (0..40)
        .find(|&y| (0..240).any(|x| buffer[(x, y)].symbol() == "❯" && x > 5))
        .expect("rendered prompt prefix");
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column: 230,
            row: prompt_y,
            modifiers: KeyModifiers::NONE,
        }));
    }
    assert!(
        app.view
            .cursor_pos(ratatui::layout::Rect::new(0, 0, 240, 40))
            .is_some()
    );
    app.handle_event(Event::Paste("registry race prompt".into()));
    registry["sessionId"] = "a-different-session".into();
    fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
    assert!(!app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE
    ))));
    assert!(
        app.pending_inputs.contains(&id),
        "Enter must dispatch SendPrompt to Provider::send"
    );
    let pending = screen(&mut app);
    assert!(!pending.contains(" · sending"));
    assert!(!pending.contains("registry race prompt"), "{pending}");
    assert!(!app.handle_action(Some(Action::Resume(id.clone()))));
    assert!(app.launch.is_none());
    assert!(!screen(&mut app).contains("Input is still being sent"));
    complete_input(&mut app, "Claude background session identity changed");
    assert!(screen(&mut app).contains("registry race prompt"));
    assert!(app.handle_action(Some(Action::Resume(id))));
    assert!(app.launch.is_some());
}

#[test]
fn background_send_rechecks_kind_and_job_id_and_allows_retry() {
    let (_directory, mut app, path, mut registry) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    registry["kind"] = "interactive".into();
    fs::write(&path, serde_json::to_vec(&registry).unwrap()).unwrap();
    app.handle_action(Some(Action::SendPrompt {
        id: id.clone(),
        text: "kind changed".into(),
    }));
    complete_input(&mut app, "Claude background session identity changed");
    registry["kind"] = "bg".into();
    registry["jobId"] = "".into();
    fs::write(&path, serde_json::to_vec(&registry).unwrap()).unwrap();
    app.handle_action(Some(Action::SendPrompt {
        id: id.clone(),
        text: "job id missing".into(),
    }));
    assert!(app.pending_inputs.contains(&id));
    complete_input(&mut app, "Claude background session has no job id");
    fs::remove_file(path).unwrap();
    app.handle_action(Some(Action::SendPrompt {
        id,
        text: "registry removed".into(),
    }));
    complete_input(&mut app, "No such file or directory");
}

#[test]
fn terminal_registry_session_does_not_open_a_session_terminal() {
    let (_directory, mut app, _path, _registry) = registry_app("interactive");
    let id = app.view.selected_id().unwrap().clone();
    screen(&mut app);
    app.handle_action(Some(Action::OpenTerminal(id)));
    assert!(app.terminal.is_none());
    assert!(screen(&mut app).contains(crate::model::FOREGROUND_SESSION));
}

#[test]
fn terminal_registry_session_refuses_provider_input() {
    let (_directory, mut app, _path, _registry) = registry_app("interactive");
    let id = app.view.selected_id().unwrap().clone();
    assert!(app.session(&id).unwrap().background_job.is_none());
    assert_eq!(
        app.session(&id).unwrap().input_disabled_reason(),
        Some("/bg")
    );
    let rendered = screen(&mut app);
    assert!(rendered.contains("❮ /bg"));
    assert!(!rendered.contains("Running in a terminal"));
    app.handle_action(Some(Action::SendPrompt {
        id,
        text: "must not reach terminal".into(),
    }));
    let event = app.receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(&event, Input::Sent { result: Err(error), .. }
        if error.to_string() == crate::model::FOREGROUND_SESSION));
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(app.pending_inputs.is_empty());
    assert!(screen(&mut app).contains("Running in a terminal"));
}

#[test]
fn batch_dispatches_in_order_and_stops_when_action_exits() {
    let (directory, mut app, _path, _registry) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    assert!(!app.handle_action(Some(Action::Batch(vec![
        Action::TogglePin {
            id: id.clone(),
            pinned: true
        },
        Action::TogglePin {
            id: id.clone(),
            pinned: false
        },
    ]))));
    assert!(Preferences::load(directory.path()).pinned.is_empty());
    assert!(app.handle_action(Some(Action::Batch(vec![
        Action::TogglePin {
            id: id.clone(),
            pinned: true
        },
        Action::Quit,
        Action::TogglePin {
            id: id.clone(),
            pinned: false
        },
    ]))));
    let preferences = Preferences::load(directory.path());
    assert_eq!(preferences.pinned, vec![id]);
}

#[test]
fn pinning_puts_the_session_first() {
    let (directory, mut app, _path, _registry) = registry_app("bg");
    let first = app.view.selected_id().unwrap().clone();
    let second = SessionKey {
        id: "second".into(),
        ..first.clone()
    };
    let pin = |id: &SessionKey, pinned| Action::TogglePin {
        id: id.clone(),
        pinned,
    };
    app.handle_action(Some(pin(&first, true)));
    app.handle_action(Some(pin(&second, true)));
    assert_eq!(
        Preferences::load(directory.path()).pinned,
        vec![second.clone(), first.clone()]
    );
    app.handle_action(Some(pin(&first, false)));
    app.handle_action(Some(pin(&first, true)));
    assert_eq!(
        Preferences::load(directory.path()).pinned,
        vec![first, second]
    );
}

#[test]
fn inactive_sessions_lose_their_pin() {
    let (directory, mut app, registry, _) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    app.handle_action(Some(Action::TogglePin {
        id: id.clone(),
        pinned: true,
    }));
    fs::remove_file(registry).unwrap();
    fs::remove_file(directory.path().join("jobs/e772d00b/state.json")).unwrap();
    app.refresh();
    assert_eq!(
        app.session(&id).unwrap().status,
        crate::model::SessionStatus::Inactive
    );
    assert!(Preferences::load(directory.path()).pinned.is_empty());
    assert!(!screen(&mut app).contains("Pinned"));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
    assert_eq!(app.view.selected_id(), Some(&id));
    assert_eq!(
        app.view
            .handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        Some(Action::Unsupported("Pin"))
    );
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
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(
        app.pending_inputs.contains(&id),
        "stop remains pending until its refresh is applied"
    );
    db.execute_batch("COMMIT").unwrap();
    app.finish_refreshes();
    assert!(!app.pending_inputs.contains(&id));
    let area = ratatui::layout::Rect::new(0, 0, 120, 30);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    app.view.render(area, &mut buffer);
    let screen = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(screen.contains("Cannot stop"), "{screen}");
    assert!(!screen.contains(" · failed:"), "{screen}");
    assert!(!screen.contains(" · sending"), "{screen}");
    assert_eq!(app.session(&id).unwrap().title, "Refreshed after stop");
}

#[test]
fn app_refresh_reads_live_and_stopped_background_jobs_from_disk() {
    let (directory, mut app, registry, _) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    let session = app.session(&id).unwrap();
    assert_eq!(session.process.as_ref().unwrap().pid, std::process::id());
    assert_eq!(session.background_job.as_ref().unwrap().id, "e772d00b");
    fs::remove_file(registry).unwrap();
    app.refresh();
    let stopped = app.session(&id).unwrap();
    assert!(stopped.process.is_none());
    assert_eq!(stopped.background_job.as_ref().unwrap().id, "e772d00b");
    fs::remove_file(directory.path().join("jobs/e772d00b/state.json")).unwrap();
    app.refresh();
    assert!(app.session(&id).unwrap().background_job.is_none());
}

#[test]
#[ignore = "writes and restores the real Claude trust config; run serially with live CLI tests"]
fn app_creates_worktree_with_the_task_request() {
    struct RestoreTrust {
        path: PathBuf,
        bytes: Option<Vec<u8>>,
    }
    impl Drop for RestoreTrust {
        fn drop(&mut self) {
            match &self.bytes {
                Some(bytes) => fs::write(&self.path, bytes).unwrap(),
                None => match fs::remove_file(&self.path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => panic!("restore trust config: {error}"),
                },
            }
            assert_eq!(
                fs::read(&self.path).ok(),
                self.bytes,
                "trust config bytes were not restored"
            );
        }
    }
    let config = crate::providers::claude::trust_config_path();
    let backup = match fs::read(&config) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => panic!("read trust config: {error}"),
    };
    let _restore = RestoreTrust {
        path: config.clone(),
        bytes: backup,
    };
    let directory = tempfile::tempdir().unwrap();
    let repo = directory.path().join("repo");
    let trees = directory.path().join("trees");
    let preferences = directory.path().join("preferences");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&trees).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Agents Test",
            "-c",
            "user.email=agents@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::write(repo.join(".env"), "APP_WORKTREE_PROBE=1\n").unwrap();
    Preferences {
        worktree_root: Some(trees.clone()),
        ..Preferences::default()
    }
    .save(&preferences)
    .unwrap();
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(directory.path().join("claude")),
        )]),
        preferences,
    );
    app.handle_action(Some(Action::NewDraft { cwd: repo.clone() }));
    assert!(screen(&mut app).contains("New task"));
    app.handle_action(Some(Action::CreateTask {
        draft_key: DraftKey {
            millis: 0,
            counter: 1,
        },
        provider: ProviderId::Claude,
        request: crate::tasks::NewTask {
            cwd: repo,
            name: None,
            model: None,
            effort: None,
            prompt: "probe".into(),
            images: Vec::new(),
        },
        branch: Some(crate::tasks::BranchChoice::New("feature-path".into())),
    }));
    let worktree = trees.join("repo/feature-path");
    // The worktree arrives with the request; handling it would start a real task.
    let event = app.receiver.recv_timeout(Duration::from_secs(15)).unwrap();
    assert!(
        matches!(&event, Input::WorktreeCreated { draft_key: DraftKey { millis: 0, counter: 1 }, request, result: Ok((path, branch)), .. }
        if path == &worktree && branch == "feature-path" && request.prompt == "probe")
    );
    assert_eq!(
        fs::read(worktree.join(".env")).unwrap(),
        b"APP_WORKTREE_PROBE=1\n"
    );
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(&worktree)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "feature-path"
    );
    let trusted: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    let path = fs::canonicalize(&worktree).unwrap();
    assert_eq!(
        trusted["projects"][path.to_str().unwrap()]["hasTrustDialogAccepted"],
        true
    );
}

#[test]
fn registry_background_title_needs_and_delete_warning_reach_the_screen() {
    let (directory, mut app, _path, _registry) = registry_app("bg");
    let state_path = directory.path().join("jobs/e772d00b/state.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["needs"] = "send a prompt to start".into();
    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    app.refresh();
    for width in [40, 80, 160] {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, 40)).unwrap();
        app.draw(&mut terminal).unwrap();
        let shown: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(!shown.contains("bg "), "{shown}");
        if width == 160 {
            assert!(shown.contains("send a prompt to start"), "{shown}");
            let title = app.sessions[0].display_title();
            let row = terminal
                .backend()
                .buffer()
                .content()
                .chunks(usize::from(width))
                .map(|cells| cells.iter().map(|cell| cell.symbol()).collect::<String>())
                .find(|row| row.contains(title) && row.contains("Working"))
                .expect("background task row");
            let title_and_status = row.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            assert_eq!(
                title_and_status,
                include_str!("../tests/ui_snapshots/background_session_row.txt").trim_end()
            );
        }
    }
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::NONE,
    )));
    let rendered = screen(&mut app);
    assert!(
        rendered.contains("Worktrees and")
            && rendered.contains("branches created by")
            && rendered.contains("agents remain."),
        "{rendered}"
    );
    app.handle_event(Event::Paste("blocked-paste-proof".into()));
    assert!(!screen(&mut app).contains("blocked-paste-proof"));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(app.sessions[0].transcript.as_ref().unwrap().is_file());
}

#[test]
fn codex_setting_actions_report_daemon_connection_failures() {
    let directory = tempfile::tempdir().unwrap();
    rusqlite::Connection::open(directory.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    let mut app = App::new(
        Hub::new(vec![Box::new(crate::providers::codex::CodexProvider::new(
            directory.path().into(),
            directory.path().join("manager"),
        ))]),
        directory.path().into(),
    );
    app.refresh();
    let id = app.sessions[0].id.clone();
    for _ in 0..5 {
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        for _ in 0..app.sessions.len() {
            if app.view.selected_id() == Some(&id) {
                break;
            }
            app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        }
        if app.view.selected_id() == Some(&id) {
            break;
        }
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    }
    assert_eq!(app.view.selected_id(), Some(&id));
    for action in [
        Action::SetModel {
            id: id.clone(),
            model: "gpt-6.1-sol".into(),
        },
        Action::SetEffort {
            id: id.clone(),
            effort: "high".into(),
        },
    ] {
        app.handle_action(Some(action));
        assert!(app.pending_inputs.contains(&id));
        complete_input(&mut app, "Codex's app-server daemon isn't running");
    }
}

#[test]
fn duplicate_inputs_keep_the_screen_and_do_not_start_another_job() {
    let (_directory, mut app, path, mut registry) = registry_app("bg");
    let id = app.view.selected_id().unwrap().clone();
    registry["sessionId"] = "a-different-session".into();
    fs::write(path, serde_json::to_vec(&registry).unwrap()).unwrap();
    app.send_input(id.clone(), SessionInput::Prompt("original prompt".into()));
    for input in [
        SessionInput::Prompt("duplicate prompt".into()),
        SessionInput::SetModel("claude-opus-5-5".into()),
        SessionInput::SetEffort("medium".into()),
        SessionInput::Interrupt,
    ] {
        app.send_input(id.clone(), input);
        assert_eq!(app.pending_inputs.len(), 1);
        assert!(!screen(&mut app).contains("Input is still being sent"));
    }
    complete_input(&mut app, "Claude background session identity changed");
    assert!(app.receiver.try_recv().is_err());
}
