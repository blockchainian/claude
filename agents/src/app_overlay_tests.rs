use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::Backend, buffer::Buffer, layout::Rect};

fn key(app: &mut App, code: KeyCode) {
    assert!(!app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
}

fn terminal_bytes(before: &Buffer, after: &Buffer, area: Rect) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut backend = CrosstermBackend::new(&mut bytes);
    backend
        .draw(
            before
                .diff(after)
                .into_iter()
                .filter(|(x, y, _)| area.contains((*x, *y).into())),
        )
        .unwrap();
    bytes
}

#[test]
fn delete_dialog_covers_project_link_underlines_through_app() {
    let directory = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(directory.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id NOT IN ('thread-0', 'thread-1', 'thread-2', 'thread-3', 'thread-4', 'thread-5')", []).unwrap();
    for index in 0..6 {
        let project = directory.path().join(format!("project-{index}"));
        std::fs::create_dir(&project).unwrap();
        db.execute(
            "UPDATE threads SET cwd=?1, title=?2 WHERE id=?3",
            rusqlite::params![
                project.to_str().unwrap(),
                format!("Overlay task {index}"),
                format!("thread-{index}")
            ],
        )
        .unwrap();
    }
    let mut app = App::new(
        Hub::new(vec![Box::new(crate::providers::codex::CodexProvider::new(
            directory.path().into(),
        ))]),
        directory.path().into(),
    );
    app.refresh();
    assert_eq!(app.sessions.len(), 6);
    key(&mut app, KeyCode::BackTab);
    key(&mut app, KeyCode::Home);

    for (width, height) in [(120, 60), (100, 50), (149, 60)] {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        app.draw(&mut terminal).unwrap();
        let before = terminal.backend().buffer().clone();
        assert!(
            before
                .content
                .iter()
                .any(|cell| cell.symbol().contains("\x1b[4:4m"))
        );

        key(&mut app, KeyCode::Backspace);
        app.draw(&mut terminal).unwrap();
        let after = terminal.backend().buffer().clone();
        let find = |symbol| {
            after
                .content
                .iter()
                .position(|cell| cell.symbol().contains(symbol))
                .map(|index| (index as u16 % width, index as u16 / width))
                .unwrap()
        };
        let (left, top) = find('┌');
        let (right, bottom) = find('┘');
        let dialog = Rect::new(left, top, right - left + 1, bottom - top + 1);
        let covered_headings: Vec<_> = before
            .content
            .iter()
            .enumerate()
            .filter(|(index, cell)| {
                dialog.contains(((*index as u16 % width), (*index as u16 / width)).into())
                    && cell.symbol().contains("\x1b[4:4m")
            })
            .collect();
        assert!(
            !covered_headings.is_empty(),
            "a project heading must be behind the dialog"
        );

        let bytes = terminal_bytes(&before, &after, dialog);
        let output = String::from_utf8(bytes).unwrap();
        assert!(
            !output.contains("\x1b[4:4m"),
            "dotted underline inside {dialog:?}: {output:?}"
        );
        assert!(
            !output.contains("\x1b]8;"),
            "OSC 8 inside the dialog: {output:?}"
        );
        for (index, cell) in before.content.iter().enumerate() {
            if !dialog.contains((index as u16 % width, index as u16 / width).into())
                && cell.symbol().contains("\x1b[4:4m")
            {
                assert!(after.content[index].symbol().contains("\x1b[4:4m"));
            }
        }
        assert!(
            after
                .content
                .iter()
                .any(|cell| cell.symbol().contains("\x1b[4:4m")),
            "uncovered links keep their underlines"
        );

        key(&mut app, KeyCode::Esc);
        app.draw(&mut terminal).unwrap();
        let restored = terminal.backend().buffer();
        for (index, _) in covered_headings {
            assert!(
                restored.content[index].symbol().contains("\x1b[4:4m"),
                "heading underline returns after closing the dialog"
            );
        }
    }
}

#[test]
fn delete_dialog_worktree_removal_through_app() {
    for (choice, dirty_file) in [
        ('2', None),
        ('3', None),
        ('3', Some("sub/file")),
        ('3', Some("untracked")),
    ] {
        let dirty = dirty_file.is_some();
        let (repo, linked) = crate::tasks::tests::worktree();
        let (_directory, mut app, transcript) = deletion_fixture(&linked.join("sub"));
        let children = transcript.with_extension("").join("subagents");
        std::fs::create_dir_all(&children).unwrap();
        std::fs::copy(&transcript, children.join("agent-fixture.jsonl")).unwrap();
        if let Some(file) = dirty_file {
            std::fs::write(linked.join(file), "keep me").unwrap();
        }
        let id = app.sessions[0].id.clone();
        super::live_tests::select(&mut app, &id);
        key(&mut app, KeyCode::Backspace);
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 48)).unwrap();
        app.draw(&mut terminal).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("3. Permanently delete task, child agents and worktree"));
        key(&mut app, KeyCode::Char(choice));
        let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let event = app.next_input(deadline.saturating_duration_since(Instant::now()));
            let deleted = matches!(event, Input::Deleted { .. });
            app.handle_inputs(event, &mut None, &mut schedule).unwrap();
            if deleted {
                break;
            }
        }
        app.finish_refreshes();
        assert!(!transcript.exists());
        assert!(!children.exists());
        assert!(app.sessions.is_empty());
        assert_eq!(linked.exists(), choice == '2' || dirty);
        assert!(
            crate::tasks::tests::git(repo.path(), &["branch", "--list", "linked"])
                .contains("linked")
        );
        app.draw(&mut terminal).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert_eq!(rendered.contains("Cannot remove worktree:"), dirty);
        if let Some(file) = dirty_file {
            assert_eq!(
                std::fs::read_to_string(linked.join(file)).unwrap(),
                "keep me"
            );
        }
    }
}

fn deletion_fixture(cwd: &std::path::Path) -> (tempfile::TempDir, App, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/probe");
    std::fs::create_dir_all(&project).unwrap();
    crate::providers::claude::write_fixture_cache(directory.path());
    let transcript = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    let payload = include_str!("../tests/fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut value: serde_json::Value = serde_json::from_str(line).unwrap();
            if value.get("cwd").is_some() {
                value["cwd"] = cwd.to_string_lossy().as_ref().into();
            }
            value.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&transcript, payload).unwrap();
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(directory.path().into()),
        )]),
        directory.path().into(),
    );
    app.refresh();
    assert_eq!(app.sessions.len(), 1);
    (directory, app, transcript)
}

#[test]
fn delete_dialog_failed_task_deletion_preserves_worktree() {
    let (_repo, linked) = crate::tasks::tests::worktree();
    let (_directory, mut app, transcript) = deletion_fixture(&linked);
    let session = app.sessions[0].clone();
    // A directory at the transcript path makes the real provider's file deletion fail.
    std::fs::remove_file(&transcript).unwrap();
    std::fs::create_dir(&transcript).unwrap();
    app.delete(session, Some(linked.clone()));
    let event = app.next_input(Duration::from_secs(10));
    assert!(matches!(event, Input::Deleted { result: Err(_), .. }));
    let mut schedule = RefreshSchedule::new(Instant::now(), POLLING_INTERVAL);
    app.handle_inputs(event, &mut None, &mut schedule).unwrap();
    assert!(linked.exists());
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(120, 48)).unwrap();
    app.draw(&mut terminal).unwrap();
    let rendered = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("Cannot delete:"));
    assert!(!rendered.contains("Cannot remove worktree:"));
}
