use super::*;

fn dashboard_newline(entry: impl FnOnce(&mut crate::ui::View)) {
    use crate::model::{Action, SessionStatus};
    use crate::providers::Provider;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::{buffer::Buffer, layout::Rect};

    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/probe");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut provider = super::super::ClaudeProvider::new(directory.path().into());
    let mut sessions = provider.refresh(1).sessions;
    assert_eq!(sessions.len(), 1);
    sessions[0].status = SessionStatus::Ready;
    let mut view = crate::ui::View::default();
    view.set_sessions(sessions, false);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 240, 50));
    view.render_at(buffer.area, &mut buffer, chrono::Utc::now());
    let (column, row) = (0..50)
        .find_map(|y| {
            (0..240)
                .find(|&x| buffer[(x, y)].symbol() == "❯")
                .map(|x| (x, y))
        })
        .expect("prompt marker");
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        assert!(
            view.handle_mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE
            })
            .is_none()
        );
    }
    entry(&mut view);
    // The failure status retains the editable prompt, with separate rendered rows.
    view.input_failed("Input contains terminal control characters".into());
    view.render_at(buffer.area, &mut buffer, chrono::Utc::now());
    let rows: Vec<String> = buffer
        .content
        .chunks(240)
        .map(|cells| cells.iter().map(|cell| cell.symbol()).collect())
        .collect();
    let first = rows
        .iter()
        .position(|row| row.contains("first line"))
        .expect("first prompt line");
    assert!(rows[first + 1].contains("second line"), "{rows:?}");
    let action = view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Some(Action::SendPrompt { text, .. }) = action else {
        panic!("expected prompt submission: {action:?}")
    };
    assert_eq!(text, "first line\nsecond line");
    validate_text(&text).unwrap();
}

fn dashboard_newline_key(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) {
    dashboard_newline(|view| {
        view.handle_paste("first line");
        assert_eq!(
            view.handle_key(crossterm::event::KeyEvent::new(code, modifiers)),
            None
        );
        view.handle_paste("second line");
    });
}

#[test]
fn dashboard_newline_shift_enter() {
    use crossterm::event::*;
    dashboard_newline_key(KeyCode::Enter, KeyModifiers::SHIFT);
}

#[test]
fn dashboard_newline_option_enter() {
    use crossterm::event::*;
    dashboard_newline_key(KeyCode::Enter, KeyModifiers::ALT);
}

#[test]
fn dashboard_newline_control_j() {
    use crossterm::event::*;
    dashboard_newline_key(KeyCode::Char('j'), KeyModifiers::CONTROL);
}

#[test]
fn dashboard_newline_paste_cr() {
    dashboard_newline(|view| assert!(view.handle_paste("first line\rsecond line")));
}

#[test]
fn dashboard_newline_paste_crlf() {
    dashboard_newline(|view| assert!(view.handle_paste("first line\r\nsecond line")));
}

#[test]
fn dashboard_newline_paste_lf() {
    dashboard_newline(|view| assert!(view.handle_paste("first line\nsecond line")));
}
