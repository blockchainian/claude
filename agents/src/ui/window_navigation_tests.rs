use super::*;

fn chord(view: &mut View, direction: char) -> Option<Action> {
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        None
    );
    view.handle_key(KeyEvent::new(KeyCode::Char(direction), KeyModifiers::NONE))
}

#[test]
fn window_navigation_all_directions_in_both_layouts() {
    use Panel::*;
    for width in [100, 200] {
        for terminal in [false, true] {
            let bottom = if terminal { Terminal } else { LogDetail };
            let expected = if width == 200 {
                vec![
                    (TaskList, [TaskList, Prompt, TaskList, Details]),
                    (Details, [TaskList, Prompt, Details, LogList]),
                    (LogList, [Details, Prompt, LogList, LogList]),
                    (Prompt, [Prompt, bottom, Details, Prompt]),
                    (bottom, [bottom, bottom, Prompt, bottom]),
                ]
            } else {
                vec![
                    (TaskList, [TaskList, Prompt, TaskList, Details]),
                    (Details, [TaskList, Prompt, Details, Details]),
                    (Prompt, [Prompt, LogList, TaskList, Prompt]),
                    (LogList, [LogList, bottom, Prompt, LogList]),
                    (bottom, [bottom, bottom, LogList, bottom]),
                ]
            };
            for (source, targets) in expected {
                for (direction, target) in ['h', 'j', 'k', 'l'].into_iter().zip(targets) {
                    let (mut view, now) = log_fixture();
                    view.rows[0].process = None;
                    if terminal {
                        let screen = crate::session_terminal::TerminalScreen::new(10, 80);
                        view.show_terminal(
                            session_key("log"),
                            std::sync::Arc::new(std::sync::Mutex::new(screen)),
                        );
                    }
                    log_screen(&view, width, 45, now);
                    view.active_panel = source;

                    assert_eq!(chord(&mut view, direction), None);
                    assert!(
                        view.active_panel == target,
                        "width={width} terminal={terminal} key={direction}"
                    );
                    assert_eq!(view.prompt_focused(), target == Prompt);
                    assert_eq!((view.active_panel == Panel::LogList), target == LogList);
                    assert_eq!((view.active_panel == Panel::LogDetail), target == LogDetail);
                }
            }
        }
    }
}

#[test]
fn window_navigation_preserves_editor_ctrl_w_before_other_key() {
    for search in [false, true] {
        let (mut view, _) = log_fixture();
        if search {
            key(&mut view, KeyCode::Char('/'));
            for c in "one two".chars() {
                key(&mut view, KeyCode::Char(c));
            }
        } else {
            view.active_panel = Panel::Prompt;
            view.input_box.editor.insert("one two");
        }
        assert_eq!(chord(&mut view, 'x'), None);
        assert_eq!(
            if search {
                view.search.text()
            } else {
                view.input_box.editor.text()
            },
            "one x"
        );
    }
}

#[test]
fn window_navigation_terminal_ctrl_w_l_is_not_forwarded() {
    let (mut view, _) = log_fixture();
    view.show_terminal(
        session_key("log"),
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::session_terminal::TerminalScreen::new(10, 40),
        )),
    );
    // A horizontal arrangement checks that terminal input cannot consume navigation.
    {
        let v = &mut *view.viewport.borrow_mut();
        v.terminal = Rect::new(0, 0, 40, 10);
        v.list = Rect::new(42, 0, 40, 10);
    }
    assert_eq!(chord(&mut view, 'l'), None);
    assert!(view.active_panel == Panel::TaskList);
}

#[test]
fn window_navigation_terminal_fallback_preserves_both_encoded_keys() {
    let (mut view, _) = log_fixture();
    view.show_terminal(
        session_key("log"),
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::session_terminal::TerminalScreen::new(10, 40),
        )),
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT)),
        Some(Action::Batch(vec![
            Action::TerminalInput(vec![23]),
            Action::TerminalInput(b"\x1b[1;3A".to_vec())
        ]))
    );
}

#[test]
fn window_navigation_release_and_second_ctrl_w_preserve_input() {
    let (mut view, _) = log_fixture();
    view.active_panel = Panel::Prompt;
    view.input_box.editor.insert("one two three");
    let prefix = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(view.handle_key(prefix), None);
    assert_eq!(view.input_box.editor.text(), "one two three");
    let mut release = prefix;
    release.kind = KeyEventKind::Release;
    assert_eq!(view.handle_key(release), None);
    assert_eq!(view.input_box.editor.text(), "one two three");
    assert_eq!(view.handle_key(prefix), None);
    assert_eq!(view.input_box.editor.text(), "one ");
}

#[test]
fn window_navigation_leaving_search_cancels_it_like_a_click() {
    let (mut view, now) = log_fixture();
    log_screen(&view, 200, 45, now);
    key(&mut view, KeyCode::Char('/'));
    key(&mut view, KeyCode::Char('x'));
    assert!(view.searching);
    chord(&mut view, 'l');
    assert!(!view.searching);
    assert!(view.search.is_empty());
    assert!(view.active_panel == Panel::Details);
}

#[test]
fn window_navigation_help_lists_directions() {
    let view = View::default();
    assert!(
        view.center_help_lines(200)
            .iter()
            .any(|line| line.to_string().contains("⌃w h/j/k/l"))
    );
}

#[test]
#[ignore = "requires a real PTY"]
fn window_navigation_real_terminal_probe() {
    use crate::session_terminal::SessionTerminal;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut command = std::process::Command::new("/bin/bash");
    command.args(["--noprofile", "--norc", "-i"]);
    command.env("PS1", "WINDOW-PROBE> ");
    let mut terminal =
        SessionTerminal::spawn(session_key("log"), command, None, (12, 100), move || {
            let _ = tx.send(());
        })
        .unwrap();
    let wait = |needle: &str, terminal: &SessionTerminal| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let text = terminal.screen.lock().unwrap().screen().contents();
            if text.contains(needle) {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "{text}");
            terminal.drawn();
            let _ = rx.recv_timeout(std::time::Duration::from_millis(100));
        }
    };
    wait("WINDOW-PROBE>", &terminal);
    for width in [100, 200] {
        let (mut view, now) = log_fixture();
        view.rows[0].process = None;
        view.show_terminal(session_key("log"), terminal.screen.clone());
        log_screen(&view, width, 45, now);
        assert_eq!(chord(&mut view, 'l'), None);
        assert!(view.active_panel == Panel::Terminal); // no panel to the right
        assert_eq!(chord(&mut view, 'k'), None);
        assert!(
            view.active_panel
                == if width == 200 {
                    Panel::Prompt
                } else {
                    Panel::LogList
                }
        );
        view.show_terminal(session_key("log"), terminal.screen.clone());
        // A real readline Ctrl-W deletes the last word before the following key.
        let command = format!("echo one{width} two");
        terminal.write(command.as_bytes()).unwrap();
        wait(&command, &terminal);
        let Some(Action::Batch(actions)) = chord(&mut view, 'x') else {
            panic!("missing terminal fallback")
        };
        for action in actions {
            let Action::TerminalInput(bytes) = action else {
                panic!("not terminal input")
            };
            terminal.write(&bytes).unwrap();
        }
        terminal.write(b"\r").unwrap();
        wait(&format!("one{width} x"), &terminal);
        terminal.drawn();
    }
    terminal.close().unwrap();
    let verdict = serde_json::json!({"verdict":"pass", "widths":[100,200], "terminal_navigation":true, "readline_ctrl_w_fallback":true});
    std::fs::write(
        std::env::temp_dir().join("agents-window-terminal-verdict.json"),
        verdict.to_string(),
    )
    .unwrap();
    println!("{verdict}");
}
