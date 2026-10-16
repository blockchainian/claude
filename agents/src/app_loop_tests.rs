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
fn idle_loop_opens_a_single_click_after_the_saved_deadline() {
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
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::NONE,
    )));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.draw(&mut terminal).unwrap();
    let before = screen(&terminal);
    let (row, line) = before
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains("Tool") && line.contains("持"))
        .unwrap_or_else(|| panic!("real tool event missing:\n{}", before.join("\n")));
    let column = line.chars().position(|c| c == '持').unwrap() as u16;
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
    app.draw(&mut terminal).unwrap();
    assert_eq!(
        screen(&terminal),
        before,
        "single click must remain deferred"
    );
    // No terminal or filesystem event is queued. The real receive timeout must wake
    // for the pending click, then dispatch it before the next frame.
    let started = Instant::now();
    let mut schedule = RefreshSchedule::new(started, Duration::from_secs(5));
    let mut watcher = None;
    while screen(&terminal) == before && started.elapsed() < Duration::from_secs(1) {
        assert!(
            !app.wait_for_input(&mut watcher, &mut schedule, Instant::now())
                .unwrap()
        );
        app.draw(&mut terminal).unwrap();
    }
    assert!(
        started.elapsed() >= Duration::from_millis(80),
        "saved double-click interval was ignored"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "idle receive did not wake for the click"
    );
    let after = screen(&terminal).join("\n");
    assert_ne!(screen(&terminal), before, "deferred detail never opened");
    assert!(
        after.contains("pkill") || after.contains("run_boxes"),
        "real tool body missing:\n{after}"
    );
}
