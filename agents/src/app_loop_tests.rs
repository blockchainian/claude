use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;

fn screen(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

#[test]
fn idle_loop_finishes_click_without_delaying_log_selection() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../tests/fixtures/claude-tool-calls.jsonl"),
    )
    .unwrap();
    let preferences = Preferences {
        double_click_ms: 120,
        ..Preferences::default()
    };
    preferences.save(directory.path()).unwrap();
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(directory.path().into()),
        )]),
        directory.path().into(),
    );
    app.refresh();
    let now = chrono::Utc::now();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::NONE,
    )));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.draw_at(&mut terminal, now).unwrap();
    let before = screen(&terminal);
    let (row, line) = before
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains("持"))
        .unwrap_or_else(|| panic!("real tool event missing:\n{}", before.join("\n")));
    let column = line.chars().position(|c| c == '持').unwrap() as u16;
    let monotonic_now = Instant::now();
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        assert!(!app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row: row as u16,
            modifiers: KeyModifiers::NONE
        })));
    }
    app.draw_at(&mut terminal, now).unwrap();
    assert_ne!(screen(&terminal), before, "row selection must be immediate");
    let selected = screen(&terminal);
    assert!(selected.join("\n").contains("pkill") || selected.join("\n").contains("run_boxes"));
    let deadline = monotonic_now
        + app
            .view
            .pending_click_in(monotonic_now)
            .expect("click must be pending");
    let mut schedule = RefreshSchedule::new(monotonic_now, Duration::from_secs(5));
    assert_eq!(
        app.input_timeout(
            &schedule,
            monotonic_now,
            deadline - Duration::from_millis(1),
            now,
        ),
        Duration::from_millis(1),
        "pending click must constrain the receive timeout"
    );
    assert_eq!(
        app.input_timeout(&schedule, monotonic_now, deadline, now),
        Duration::ZERO,
        "an expired click must not wait for the periodic refresh"
    );
    // Exercise the real empty channel without counting rendering or OS scheduling
    // time as a receive deadline. The injected instants above check the timeout.
    assert!(
        !app.wait_for_input(&mut None, &mut schedule, monotonic_now)
            .unwrap()
    );
    assert!(app.view.pending_click_in(Instant::now()).is_none());
    app.draw_at(&mut terminal, now).unwrap();
    let after = screen(&terminal).join("\n");
    assert_eq!(
        screen(&terminal),
        selected,
        "expired click must not replace the selected detail"
    );
    assert!(
        after.contains("pkill") || after.contains("run_boxes"),
        "real tool body missing:\n{after}"
    );
}

#[test]
fn right_detail_toggle_draws_without_pending_input() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_bytes!("../tests/fixtures/claude-raw-detail.jsonl"),
    )
    .unwrap();
    Preferences {
        double_click_ms: 200,
        ..Preferences::default()
    }
    .save(directory.path())
    .unwrap();
    let mut app = App::new(
        Hub::new(vec![Box::new(
            crate::providers::claude::ClaudeProvider::new(directory.path().into()),
        )]),
        directory.path().into(),
    );
    app.refresh();
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::NONE,
    )));
    let now = chrono::Utc::now();
    let mut terminal = Terminal::new(TestBackend::new(200, 45)).unwrap();
    app.draw_at(&mut terminal, now).unwrap();
    let parsed = screen(&terminal);
    let (row, line) = parsed
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.contains("cctail"))
        .unwrap_or_else(|| panic!("real tool result missing:\n{}", parsed.join("\n")));
    let column = line.find("cctail").unwrap() as u16;
    for raw in [true, false] {
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Up(MouseButton::Right),
        ] {
            assert!(!app.handle_event(Event::Mouse(MouseEvent {
                kind,
                column,
                row: row as u16,
                modifiers: KeyModifiers::NONE,
            })));
        }
        assert!(app.view.pending_click_in(Instant::now()).is_none());
        app.draw_at(&mut terminal, now).unwrap();
        if raw {
            assert_ne!(screen(&terminal), parsed);
            assert!(screen(&terminal).join("\n").contains("\"message\""));
        } else {
            assert_eq!(screen(&terminal), parsed);
        }
    }
}
