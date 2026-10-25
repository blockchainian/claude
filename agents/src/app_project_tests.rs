use super::*;
use crate::model::SessionStatus;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;

#[test]
fn project_heading_returns_when_reselecting_first_inactive_task_after_paging() {
    let directory = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(directory.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute(
        "DELETE FROM threads WHERE CAST(SUBSTR(id, 8) AS INTEGER) > 10",
        [],
    )
    .unwrap();
    for index in 0..11 {
        // The affected live records have ordinary absolute cwd values in SQLite.
        let cwd = if index < 4 {
            "/workspace/alpha"
        } else {
            "/workspace/zulu"
        };
        db.execute(
            "UPDATE threads SET cwd=?1, title=?2, name=?2, rollout_path='', updated_at_ms=?3, recency_at_ms=?3 WHERE id=?4",
            rusqlite::params![cwd, format!("Project task {index}"), 1_700_000_000_000_i64 - index, format!("thread-{index}")],
        ).unwrap();
    }
    let mut app = App::new(
        Hub::new(vec![Box::new(crate::providers::codex::CodexProvider::new(
            directory.path().into(),
        ))]),
        directory.path().into(),
    );
    app.refresh();
    assert_eq!(app.sessions.len(), 10);
    let key = |app: &mut App, code| {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    };
    key(&mut app, KeyCode::Tab);
    for height in 24..44 {
        let mut terminal = Terminal::new(TestBackend::new(220, height)).unwrap();
        key(&mut app, KeyCode::Home);
        app.draw(&mut terminal).unwrap();
        for paging in [KeyCode::End, KeyCode::PageDown] {
            key(&mut app, paging);
            app.draw(&mut terminal).unwrap();
            key(&mut app, KeyCode::Home);
            app.draw(&mut terminal).unwrap();
            assert_eq!(app.view.selected_id().unwrap().id, "thread-0");
            let buffer = terminal.backend().buffer();
            let rows: Vec<_> = (2..height)
                .map(|y| {
                    (0..54)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                        .replace("\u{1b}[4:4m", "")
                        .replace("\u{1b}[24m", "")
                })
                .collect();
            let heading = rows.iter().position(|line| line.trim() == "alpha");
            let task = rows
                .iter()
                .position(|line| line.contains("Project task 0"))
                .unwrap();
            assert!(
                heading.is_some_and(|heading| heading < task),
                "height {height}, after {paging:?}: {rows:?}"
            );
        }
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        app.refresh();
        app.draw(&mut terminal).unwrap();
        assert_eq!(app.view.selected_id().unwrap().id, "thread-0");
    }
}

#[test]
fn project_groups_put_directoryless_sessions_last_in_every_tab_through_app() {
    let directory = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(directory.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id NOT IN ('thread-0', 'thread-1', 'thread-2', 'thread-3', 'thread-4', 'thread-5')", []).unwrap();
    for (index, cwd) in ["", "/tmp/zulu", "/tmp/alpha", "", "/tmp/alpha", "/tmp/zulu"]
        .into_iter()
        .enumerate()
    {
        db.execute(
            "UPDATE threads SET cwd=?1, title=?2, name=?2, rollout_path='', updated_at_ms=?3, recency_at_ms=?3 WHERE id=?4",
            rusqlite::params![cwd, format!("Project task {index}"), 1_700_000_000_000_i64 + index as i64, format!("thread-{index}")],
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
    assert_eq!(
        app.sessions
            .iter()
            .filter(|s| s.cwd.as_os_str().is_empty())
            .count(),
        2
    );
    let expected = [4, 2, 5, 1, 3, 0];
    for status in [
        SessionStatus::Ready,
        SessionStatus::Working,
        SessionStatus::Inactive,
    ] {
        // Keep provider-loaded sessions and exercise each tab's status filter.
        for session in &mut app.sessions {
            session.status = status;
        }
        app.view.set_sessions(app.sessions.clone(), false);
        if status == SessionStatus::Inactive {
            app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        }
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        let mut terminal = Terminal::new(TestBackend::new(120, 60)).unwrap();
        app.draw(&mut terminal).unwrap();
        let buffer = terminal.backend().buffer();
        let task_rows: Vec<String> = (2..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width / 2 - 1)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .replace("\u{1b}[4:4m", "")
                    .replace("\u{1b}[24m", "")
            })
            .take_while(|line| !line.starts_with(" Project task"))
            .filter(|line| {
                line.contains("Project task")
                    || line.trim() == "alpha"
                    || line.trim() == "zulu"
                    || line.contains("No directory")
            })
            .map(|line| match line.find("Project task") {
                Some(start) => line[start..]
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" "),
                None => line.trim().to_owned(),
            })
            .collect();
        for index in expected {
            assert_eq!(
                app.view.selected_id().unwrap().id,
                format!("thread-{index}"),
                "tab status {status:?}; rendered rows: {task_rows:?}"
            );
            app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        }
        assert_eq!(
            task_rows.join("\n") + "\n",
            include_str!("../tests/ui_snapshots/project_groups_directoryless_last.txt")
        );
    }
}

#[test]
fn project_groups_put_sessions_outside_repositories_after_repositories() {
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let repository = root.join("zulu");
    let plain = root.join("alpha");
    std::fs::create_dir(&repository).unwrap();
    std::fs::create_dir(&plain).unwrap();
    let status = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&repository)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .unwrap();
    assert!(status.success());
    let db = rusqlite::Connection::open(directory.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute(
        "DELETE FROM threads WHERE id NOT IN ('thread-0', 'thread-1', 'thread-2')",
        [],
    )
    .unwrap();
    for (index, cwd) in ["", plain.to_str().unwrap(), repository.to_str().unwrap()]
        .into_iter()
        .enumerate()
    {
        db.execute(
            "UPDATE threads SET cwd=?1, title=?2, name=?2, rollout_path='', updated_at_ms=?3, recency_at_ms=?3 WHERE id=?4",
            rusqlite::params![cwd, format!("Project task {index}"), 1_700_000_000_000_i64 + index as i64, format!("thread-{index}")],
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
    for session in &mut app.sessions {
        session.status = SessionStatus::Inactive;
    }
    app.view.set_sessions(app.sessions.clone(), false);
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
    let mut order = Vec::new();
    for _ in 0..3 {
        order.push(app.view.selected_id().unwrap().id.clone());
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
    }
    assert_eq!(order, ["thread-2", "thread-1", "thread-0"]);
}
