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
            let bottom = EventDetail;
            let expected = if width == 200 {
                vec![
                    (TaskList, [TaskList, Prompt, TaskList, TaskDetails]),
                    (TaskDetails, [TaskList, Prompt, TaskDetails, EventList]),
                    (EventList, [TaskDetails, Prompt, EventList, EventList]),
                    (Prompt, [Prompt, bottom, TaskDetails, Prompt]),
                    (bottom, [bottom, bottom, Prompt, bottom]),
                ]
            } else {
                vec![
                    (TaskList, [TaskList, Prompt, TaskList, TaskDetails]),
                    (TaskDetails, [TaskList, Prompt, TaskDetails, TaskDetails]),
                    (Prompt, [Prompt, EventList, TaskList, Prompt]),
                    (EventList, [EventList, bottom, Prompt, EventList]),
                    (bottom, [bottom, bottom, EventList, bottom]),
                ]
            };
            for (source, targets) in expected {
                for (direction, target) in ['h', 'j', 'k', 'l'].into_iter().zip(targets) {
                    let (mut view, now) = event_fixture();
                    view.rows[0].process = None;
                    if terminal {
                        let screen = crate::session_terminal::TerminalScreen::new(10, 80);
                        view.show_terminal(
                            session_key("log"),
                            std::sync::Arc::new(std::sync::Mutex::new(screen)),
                        );
                    }
                    event_screen(&view, width, 45, now);
                    view.set_active_panel(source);

                    assert_eq!(chord(&mut view, direction), None);
                    assert!(
                        view.active_panel == target,
                        "width={width} terminal={terminal} key={direction}"
                    );
                    assert_eq!(view.prompt_focused(), target == Prompt);
                    assert_eq!((view.active_panel == Panel::EventList), target == EventList);
                    assert_eq!(
                        (view.active_panel == Panel::EventDetail),
                        target == EventDetail
                    );
                }
            }
        }
    }
}

#[test]
fn window_navigation_preserves_editor_ctrl_w_before_other_key() {
    for search in [false, true] {
        let (mut view, _) = event_fixture();
        if search {
            key(&mut view, KeyCode::Char('/'));
            for c in "one two".chars() {
                key(&mut view, KeyCode::Char(c));
            }
        } else {
            view.rows[view.selected].process = None;
            view.set_active_panel(Panel::Prompt);
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
    let (mut view, _) = event_fixture();
    view.show_terminal(
        session_key("log"),
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::session_terminal::TerminalScreen::new(10, 40),
        )),
    );
    view.set_active_panel(Panel::EventDetail);
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
    let (mut view, _) = event_fixture();
    view.show_terminal(
        session_key("log"),
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::session_terminal::TerminalScreen::new(10, 40),
        )),
    );
    view.set_active_panel(Panel::EventDetail);
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
    let (mut view, _) = event_fixture();
    view.rows[view.selected].process = None;
    view.set_active_panel(Panel::Prompt);
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
    let (mut view, now) = event_fixture();
    event_screen(&view, 200, 45, now);
    key(&mut view, KeyCode::Char('/'));
    key(&mut view, KeyCode::Char('x'));
    assert!(view.searching);
    chord(&mut view, 'l');
    assert!(!view.searching);
    assert!(view.search.is_empty());
    assert!(view.active_panel == Panel::TaskDetails);
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
        let (mut view, now) = event_fixture();
        view.rows[0].process = None;
        view.show_terminal(session_key("log"), terminal.screen.clone());
        assert_eq!(view.active_panel, Panel::EventDetail);
        event_screen(&view, width, 45, now);
        assert_eq!(chord(&mut view, 'l'), None);
        assert!(view.active_panel == Panel::EventDetail);
        assert_eq!(chord(&mut view, 'k'), None);
        assert!(
            view.active_panel
                == if width == 200 {
                    Panel::Prompt
                } else {
                    Panel::EventList
                }
        );
        view.show_terminal(session_key("log"), terminal.screen.clone());
        view.set_active_panel(Panel::EventDetail);
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

#[test]
fn opening_editors_activates_them_but_updates_preserve_activation() {
    let (mut view, _) = event_fixture();
    view.rows[view.selected].process = None;
    view.set_active_panel(Panel::TaskDetails);
    view.prepare_prompt();
    assert_eq!(view.active_panel, Panel::Prompt);
    view.set_active_panel(Panel::TaskDetails);
    view.select_event(Some(0));
    assert!(view.active_panel == Panel::TaskDetails);
    view.set_sessions(view.rows.clone(), false);
    assert!(view.active_panel == Panel::TaskDetails);
    view.add_draft(DraftSetup {
        provider: ProviderId::Claude,
        cwd: std::env::temp_dir(),
        model: None,
        effort: None,
        branch: String::new(),
    });
    assert_eq!(view.active_panel, Panel::TaskDetails);
    assert!(view.name_target.is_some());
    view.close_selector();
    assert_eq!(view.active_panel, Panel::TaskDetails);
    view.set_active_panel(Panel::EventDetail);
    view.open_name(view.selected_draft.unwrap());
    assert_eq!(view.active_panel, Panel::TaskDetails);
}

#[test]
fn output_panel_keeps_activation_when_terminal_contents_change() {
    for width in [100, 200] {
        let (mut view, now) = event_fixture();
        view.set_active_panel(Panel::EventDetail);
        view.show_terminal(
            session_key("log"),
            std::sync::Arc::new(std::sync::Mutex::new(
                crate::session_terminal::TerminalScreen::new(10, 40),
            )),
        );
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
            Some(Action::TerminalInput(vec![6]))
        );
        assert_eq!(
            view.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
            Some(Action::TerminalInput(vec![2]))
        );
        view.hide_terminal();
        event_screen(&view, width, 45, now);
        chord(&mut view, 'k');
        assert_eq!(
            view.active_panel,
            if width == 200 {
                Panel::Prompt
            } else {
                Panel::EventList
            }
        );
    }
}

#[test]
fn task_search_editors_park_on_panel_leave_and_resume_typing() {
    for panel in [Panel::EventList, Panel::EventDetail] {
        let (mut view, now) = event_fixture();
        view.rows[view.selected].process = None;
        event_screen(&view, 200, 45, now);
        view.set_active_panel(panel);
        key(&mut view, KeyCode::Char('/'));
        key(&mut view, KeyCode::Char('a'));
        view.set_active_panel(Panel::Prompt);
        let search = if panel == Panel::EventList {
            &view.event_list.search
        } else {
            &view.event_detail.search
        };
        assert!(search.editing);
        assert_eq!(search.query.text(), "a");
        key(&mut view, KeyCode::Char('p'));
        assert_eq!(view.input_box.editor.text(), "p");
        view.set_active_panel(panel);
        key(&mut view, KeyCode::Char('b'));
        let search = if panel == Panel::EventList {
            &view.event_list.search
        } else {
            &view.event_detail.search
        };
        assert!(search.editing);
        assert_eq!(search.query.text(), "ab");
    }
}

#[test]
fn task_search_editors_survive_task_list_switches() {
    for panel in [Panel::EventList, Panel::EventDetail] {
        let (mut view, _) = event_fixture();
        let first = view.selected_id().unwrap().clone();
        let mut other = view.rows[0].clone();
        other.id = session_key("search-other");
        view.set_sessions(vec![view.rows[0].clone(), other.clone()], false);
        view.select(view.rows.iter().position(|row| row.id == first).unwrap());
        view.set_active_panel(panel);
        key(&mut view, KeyCode::Char('/'));
        key(&mut view, KeyCode::Char('a'));
        view.set_active_panel(Panel::TaskList);
        view.select(view.rows.iter().position(|row| row.id == other.id).unwrap());
        assert_eq!(view.active_panel, Panel::TaskList);
        view.select(view.rows.iter().position(|row| row.id == first).unwrap());
        assert_eq!(view.active_panel, Panel::TaskList);
        view.set_active_panel(panel);
        let search = if panel == Panel::EventList {
            &view.event_list.search
        } else {
            &view.event_detail.search
        };
        assert!(search.editing);
        assert_eq!(search.query.text(), "a");
    }
}
