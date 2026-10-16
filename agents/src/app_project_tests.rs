use super::*;
use crate::model::SessionStatus;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;

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
            directory.path().join("manager"),
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
        SessionStatus::NeedsInput,
        SessionStatus::Inactive,
    ] {
        // Keep provider-loaded sessions and exercise each tab's status filter.
        for session in &mut app.sessions {
            session.status = status;
        }
        app.view.set_sessions(app.sessions.clone(), false);
        if status == SessionStatus::Inactive {
            app.handle_event(Event::Key(KeyEvent::new(
                KeyCode::BackTab,
                KeyModifiers::NONE,
            )));
        }
        app.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        let mut terminal = Terminal::new(TestBackend::new(120, 60)).unwrap();
        app.draw(&mut terminal).unwrap();
        let buffer = terminal.backend().buffer();
        let task_rows: Vec<String> = (3..buffer.area.height)
            .map(|y| {
                (0..35)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .replace("\u{1b}[4:4m", "")
                    .replace("\u{1b}[24m", "")
            })
            .take_while(|line| !line.starts_with(" Project task"))
            .filter(|line| {
                line.contains("Project task")
                    || line.contains("/tmp/alpha")
                    || line.contains("/tmp/zulu")
            })
            .map(|line| match line.find("Project task") {
                Some(start) => line[start..].trim_end().to_owned(),
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
