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
        db.execute(
            "UPDATE threads SET cwd=?1, title=?2 WHERE id=?3",
            rusqlite::params![
                directory
                    .path()
                    .join(format!("project-{index}"))
                    .to_str()
                    .unwrap(),
                format!("Overlay task {index}"),
                format!("thread-{index}")
            ],
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
