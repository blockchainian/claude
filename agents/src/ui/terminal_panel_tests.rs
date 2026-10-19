use super::*;
use crate::session_terminal::{Screen, TerminalScreen};

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
    let screen: Screen =
        std::sync::Arc::new(std::sync::Mutex::new(TerminalScreen::new(rows, cols)));
    screen.lock().unwrap().process(text);
    screen
        .lock()
        .unwrap()
        .check_ready(std::time::Instant::now(), |_| false);
    view.show_terminal(session_key("log"), screen.clone());
    view.set_active_panel(Panel::Output);
    log_screen(&view, 100, 45, now);
    (view, now, screen)
}

#[test]
fn showing_and_hiding_terminal_preserves_the_active_panel() {
    for panel in [
        Panel::TaskList,
        Panel::Details,
        Panel::LogList,
        Panel::Prompt,
        Panel::Output,
    ] {
        let (mut view, _) = log_fixture();
        view.set_active_panel(panel);
        let screen = std::sync::Arc::new(std::sync::Mutex::new(TerminalScreen::new(10, 40)));
        view.show_terminal(session_key("log"), screen);
        assert!(view.active_panel == panel);
        view.hide_terminal();
        assert!(view.active_panel == panel);
    }
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
fn synchronized_output_keeps_the_displayed_screen_and_cursor_until_the_frame_ends() {
    let (view, now, screen) = open_terminal(b"READY\x1b[12;3H\x1b[?25h");
    let area = Rect::new(0, 0, 100, 45);
    let before = log_screen(&view, 100, 45, now);
    let cursor = view.cursor_pos(area);
    assert!(cursor.is_some());

    // One timer update captured from Codex's PTY, split at every byte boundary.
    let frame = b"\x1b[?2026h\x1b[?25l\x1b[14;31H\x1b[1m2\x1b[0m\x1b[12;3H\x1b[?25h\x1b[?2026l";
    for byte in &frame[..frame.len() - 1] {
        screen.lock().unwrap().process(&[*byte]);
        assert_eq!(view.cursor_pos(area), cursor);
        assert_eq!(log_screen(&view, 100, 45, now), before);
    }
    screen.lock().unwrap().process(&frame[frame.len() - 1..]);
    assert_eq!(view.cursor_pos(area), cursor);
    assert_ne!(log_screen(&view, 100, 45, now), before);
    assert!(screen.lock().unwrap().screen().contents().contains('2'));
}

#[test]
fn a_synchronized_redraw_retains_the_last_published_frame_before_its_clear_prefix() {
    let (view, now, screen) = open_terminal(b"READY\x1b[12;3H\x1b[?25h");
    let area = Rect::new(0, 0, 100, 45);
    let before = log_screen(&view, 100, 45, now);
    let cursor = view.cursor_pos(area);
    // Codex's full-history redraw clears immediately before beginning its update.
    screen
        .lock()
        .unwrap()
        .process(b"\x1b[r\x1b[0m\x1b[H\x1b[2J\x1b[3J\x1b[H\x1b[?2026h\x1b[1;1H\x1b[J");
    assert_eq!(view.cursor_pos(area), cursor);
    assert_eq!(log_screen(&view, 100, 45, now), before);
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
    let (mut view, _now, _screen) = open_terminal(b"\x1b[?1000h\x1b[?1006h");
    let list = view.viewport.borrow().list;
    click_mouse(
        &mut view,
        mouse(MouseEventKind::Down(MouseButton::Left), list.x + 1, list.y),
    );
    assert_ne!(
        view.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        Some(Action::TerminalInput(b"q".to_vec()))
    );
    let terminal = view.viewport.borrow().terminal;
    // The focusing click reaches the program too.
    assert_eq!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            terminal.x + 1,
            terminal.y + 1
        )),
        Some(Action::TerminalInput(b"\x1b[<0;2;2M".to_vec()))
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
    let press = mouse(
        MouseEventKind::Down(MouseButton::Left),
        terminal.x + 4,
        terminal.y + 2,
    );
    assert_eq!(
        view.handle_mouse(press),
        Some(Action::TerminalInput(b"\x1b[<0;5;3M".to_vec()))
    );
}

#[test]
fn the_wheel_does_nothing_over_a_program_without_mouse_reports() {
    let lines: Vec<u8> = (0..80)
        .flat_map(|n| format!("line {n}\r\n").into_bytes())
        .collect();
    let (mut view, now, _screen) = open_terminal(&lines);
    let terminal = view.viewport.borrow().terminal;
    let before = log_screen(&view, 100, 45, now);
    assert_eq!(
        view.handle_mouse(mouse(
            MouseEventKind::ScrollUp,
            terminal.x + 1,
            terminal.y + 1
        )),
        None
    );
    assert_eq!(log_screen(&view, 100, 45, now), before);
}

#[test]
fn a_focused_terminal_takes_pastes() {
    let (view, _now, _screen) = open_terminal(b"");
    assert_eq!(
        view.terminal_paste("text"),
        Some(Action::TerminalPaste("text".into()))
    );
}

#[test]
fn middle_paste_follows_the_program_mouse_mode_and_focuses_terminal() {
    for (protocol, expected, release) in [
        (&b""[..], Some(Action::PasteText), None),
        (
            &b"\x1b[?1000h\x1b[?1006h"[..],
            Some(Action::TerminalInput(b"\x1b[<1;2;2M".to_vec())),
            Some(Action::TerminalInput(b"\x1b[<1;2;2m".to_vec())),
        ),
    ] {
        let (mut view, now, _screen) = open_terminal(protocol);
        let terminal = view.viewport.borrow().terminal;
        let middle = mouse(
            MouseEventKind::Down(MouseButton::Middle),
            terminal.x + 1,
            terminal.y + 1,
        );
        assert_eq!(view.handle_mouse(middle), expected);
        assert!(view.active_panel == Panel::Output);
        assert_eq!(
            view.terminal_paste("text"),
            Some(Action::TerminalPaste("text".into()))
        );

        let list = view.viewport.borrow().list;
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), list.x + 1, list.y),
        );
        log_screen(&view, 100, 45, now);
        let focus = view.active_panel;
        assert!(focus != Panel::Output);
        assert_eq!(view.handle_mouse(middle), expected);
        assert!(view.active_panel == Panel::Output);
        assert_eq!(
            view.terminal_paste("text"),
            Some(Action::TerminalPaste("text".into()))
        );
        assert_eq!(
            view.handle_mouse(mouse(
                MouseEventKind::Up(MouseButton::Middle),
                middle.column,
                middle.row
            )),
            release
        );
        assert!(view.active_panel == Panel::Output);
    }
}

#[test]
fn loading_terminal_shows_running_dots_without_a_cursor() {
    let (view, now, _screen) = open_terminal(b"");
    let drawn = log_screen(&view, 100, 45, now);
    assert!(
        drawn.contains(super::super::tool_detail::running_frame(now)),
        "{drawn}"
    );
    assert_eq!(view.cursor_pos(Rect::new(0, 0, 100, 45)), None);
    assert_eq!(
        drawn,
        include_str!("../../tests/ui_snapshots/terminal_loading.txt")
    );
}
