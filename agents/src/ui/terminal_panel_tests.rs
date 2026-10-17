use super::*;
use crate::session_terminal::{Replies, Screen};

fn right_click(view: &mut View, column: u16, row: u16) -> Option<Action> {
    assert!(
        view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), column, row))
            .is_none()
    );
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Right), column, row))
}

fn open_terminal(text: &[u8]) -> (View, chrono::DateTime<Utc>, Screen) {
    let (mut view, now) = log_fixture();
    log_screen(&view, 100, 45, now);
    let (rows, cols) = view.terminal_size().unwrap();
    let screen: Screen = std::sync::Arc::new(std::sync::Mutex::new(
        vt100::Parser::new_with_callbacks(rows, cols, 100, Replies::default()),
    ));
    screen.lock().unwrap().process(text);
    view.show_terminal(session_key("log"), screen.clone());
    log_screen(&view, 100, 45, now);
    (view, now, screen)
}

#[test]
fn right_click_on_the_log_detail_opens_and_closes_the_terminal() {
    let (mut view, now) = log_fixture();
    log_screen(&view, 100, 45, now);
    let detail = view.viewport.borrow().log_detail;
    assert_eq!(
        right_click(&mut view, detail.x + 2, detail.y + 2),
        Some(Action::OpenTerminal(session_key("log")))
    );
    let (mut view, _now, _screen) = open_terminal(b"");
    let terminal = view.viewport.borrow().terminal;
    assert_eq!(
        right_click(&mut view, terminal.x + 2, terminal.y + 2),
        Some(Action::CloseTerminal)
    );
}

#[test]
fn the_terminal_draws_the_program_screen_below_the_panel_rule() {
    let (view, now, _screen) = open_terminal(b"hello from codex");
    let terminal = view.viewport.borrow().terminal;
    let screen = log_screen(&view, 100, 45, now);
    let row = screen.lines().nth(usize::from(terminal.y)).unwrap();
    assert!(row.contains("hello from codex"), "{screen}");
    assert_eq!(
        view.cursor_pos(Rect::new(0, 0, 100, 45)),
        Some((terminal.x + 16, terminal.y))
    );
}

#[test]
fn the_focused_terminal_takes_every_key() {
    let (mut view, _now, _screen) = open_terminal(b"");
    for (code, modifiers, bytes) in [
        (KeyCode::Char('q'), KeyModifiers::NONE, b"q".to_vec()),
        (KeyCode::Char('/'), KeyModifiers::NONE, b"/".to_vec()),
        (KeyCode::Char('c'), KeyModifiers::CONTROL, vec![3]),
        (KeyCode::Esc, KeyModifiers::NONE, vec![0x1b]),
    ] {
        assert_eq!(
            view.handle_key(KeyEvent::new(code, modifiers)),
            Some(Action::TerminalInput(bytes))
        );
    }
}

#[test]
fn clicking_another_panel_takes_keys_away_until_the_terminal_is_clicked() {
    let (mut view, _now, _screen) = open_terminal(b"");
    let list = view.viewport.borrow().list;
    click_mouse(&mut view, mouse(MouseEventKind::Down(MouseButton::Left), list.x + 1, list.y));
    assert_ne!(
        view.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        Some(Action::TerminalInput(b"q".to_vec()))
    );
    let terminal = view.viewport.borrow().terminal;
    // The focusing click reaches no program.
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), terminal.x + 1, terminal.y + 1)
        ),
        None
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        Some(Action::TerminalInput(b"q".to_vec()))
    );
}

#[test]
fn clicks_reach_a_program_that_asked_for_mouse_reports() {
    // SGR reports for presses and releases.
    let (mut view, _now, _screen) = open_terminal(b"\x1b[?1000h\x1b[?1006h");
    let terminal = view.viewport.borrow().terminal;
    let press = mouse(MouseEventKind::Down(MouseButton::Left), terminal.x + 4, terminal.y + 2);
    assert_eq!(
        view.handle_mouse(press),
        Some(Action::TerminalInput(b"\x1b[<0;5;3M".to_vec()))
    );
}

#[test]
fn the_wheel_scrolls_the_scrollback_of_a_program_without_mouse_reports() {
    let lines: Vec<u8> = (0..80).flat_map(|n| format!("line {n}\r\n").into_bytes()).collect();
    let (mut view, now, screen) = open_terminal(&lines);
    let terminal = view.viewport.borrow().terminal;
    let before = log_screen(&view, 100, 45, now);
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, terminal.x + 1, terminal.y + 1));
    assert_eq!(screen.lock().unwrap().screen().scrollback(), 3);
    assert_ne!(log_screen(&view, 100, 45, now), before);
    // Typing returns to the live screen.
    view.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert_eq!(screen.lock().unwrap().screen().scrollback(), 0);
}

#[test]
fn a_focused_terminal_takes_pastes() {
    let (view, _now, _screen) = open_terminal(b"");
    assert_eq!(
        view.terminal_paste("text"),
        Some(Action::TerminalPaste("text".into()))
    );
}
