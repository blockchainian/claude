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
                    (Prompt, [Prompt, bottom, TaskList, Prompt]),
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
        assert_eq!(chord(&mut view, 'x'), Some(Action::Cut("two".into())));
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
    assert_eq!(
        view.handle_key(prefix),
        Some(Action::Cut("two three".into()))
    );
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

fn zoom_rects(layout: render::CenterLayout) -> [Rect; 15] {
    [
        layout.header,
        layout.top,
        layout.list,
        layout.gap,
        layout.task_details,
        layout.search,
        layout.usage_chart,
        layout.footer,
        layout.event_panel,
        layout.prompt,
        layout.event_detail,
        layout.prompt_top_rule,
        layout.prompt_bottom_rule,
        layout.event_detail_top_rule,
        layout.footer_rule,
    ]
}

fn zoom_key(view: &mut View, key: char, modifiers: KeyModifiers) {
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char(key), modifiers)),
        None
    );
}

#[test]
fn panel_zoom_narrow_detail_has_no_collapsed_frame() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 92, 110, now);
    view.set_active_panel(Panel::EventDetail);
    zoom_key(&mut view, '_', KeyModifiers::SHIFT);
    let screen = event_screen(&view, 92, 110, now);
    let rows: Vec<_> = screen.lines().collect();
    assert!(
        rows[2].contains("Task log example") && rows[4].contains("run tests"),
        "expected the prompt and enlarged detail without collapsed frame rows:\n{}",
        rows[..8].join("\n")
    );
}

#[test]
fn panel_zoom_collapsed_panels_leave_no_decorations() {
    for width in [92, 149, 150, 200] {
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            for commands in ["", "_", "|", "_|", "|_"] {
                let (mut view, now) = event_fixture();
                view.set_activity(
                    (0..400)
                        .map(|age| crate::usage::bin_of(now) - age)
                        .collect(),
                );
                event_screen(&view, width, 110, now);
                view.set_active_panel(panel);
                for command in commands.chars() {
                    zoom_key(&mut view, command, KeyModifiers::SHIFT);
                }
                let layout = view.center_layout(Rect::new(0, 0, width, 110));
                let screen = event_screen(&view, width, 110, now);
                let rows: Vec<_> = screen.lines().collect();
                let context = format!("{width} {panel:?} {commands}");
                if layout.top.is_empty() {
                    assert!(!rows[1].contains(['┬', '┴', '┼']), "{context}: {}", rows[1]);
                    assert_eq!(layout.prompt.y, layout.header.bottom(), "{context}");
                }
                if layout.list.is_empty() {
                    assert!(
                        !screen
                            .chars()
                            .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
                        "{context}: {screen}"
                    );
                }
            }
        }
    }
}

#[test]
fn panel_zoom_rects_and_reset_in_both_layouts() {
    for width in [120, 200] {
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
                let (mut view, now) = event_fixture();
                let area = Rect::new(0, 0, width, 45);
                event_screen(&view, width, 45, now);
                view.set_active_panel(panel);
                let original = view.center_layout(area);
                let fixed = (original.header, original.footer, original.prompt.height);
                let content_height = original.top.height
                    + original.event_detail.height
                    + if width < 150 {
                        original.event_panel.height
                    } else {
                        0
                    };
                let in_top =
                    panel != Panel::EventDetail && (width >= 150 || panel != Panel::EventList);
                let height = content_height
                    + if in_top {
                        u16::from(width < 150)
                    } else if width < 150 && panel == Panel::EventDetail {
                        1
                    } else {
                        0
                    }
                    + u16::from(!in_top);
                let rect = |l: &render::CenterLayout| match panel {
                    Panel::TaskList => l.list,
                    Panel::TaskDetails => l.task_details,
                    Panel::EventList => l.event_panel,
                    Panel::EventDetail => l.event_detail,
                    Panel::Prompt => unreachable!(),
                };
                zoom_key(&mut view, '_', modifiers);
                let tall = view.center_layout(area);
                assert_eq!(rect(&tall).height, height, "{width} {panel:?}");
                let y = if in_top {
                    original.header.bottom()
                } else {
                    original.header.bottom()
                        + original.prompt.height
                        + u16::from(width < 150 && panel == Panel::EventList)
                };
                assert_eq!(
                    rect(&tall),
                    Rect::new(rect(&original).x, y, rect(&original).width, height)
                );
                assert_eq!(
                    tall.prompt,
                    Rect::new(
                        0,
                        original.header.bottom() + if in_top { height + 1 } else { 0 },
                        width,
                        original.prompt.height
                    )
                );
                assert_eq!(tall.top.height, if in_top { height } else { 0 });
                assert_eq!(
                    tall.event_detail.height,
                    if panel == Panel::EventDetail {
                        height
                    } else {
                        0
                    }
                );
                if width < 150 {
                    assert_eq!(
                        tall.event_panel.height,
                        if panel == Panel::EventList { height } else { 0 }
                    );
                }
                assert_eq!((tall.header, tall.footer, tall.prompt.height), fixed);
                zoom_key(&mut view, '|', modifiers);
                let both = view.center_layout(area);
                assert_eq!(rect(&both).height, height);
                assert_eq!(
                    rect(&both),
                    Rect::new(
                        if panel == Panel::EventDetail { 0 } else { 1 },
                        y,
                        if panel == Panel::EventDetail {
                            width
                        } else {
                            width - 2
                        },
                        height
                    )
                );
                assert_eq!(
                    rect(&both).width,
                    if panel == Panel::EventDetail {
                        width
                    } else {
                        width - 2
                    }
                );
                if in_top {
                    assert!(both.gap.is_empty());
                    for (other, r) in [
                        (Panel::TaskList, both.list),
                        (Panel::TaskDetails, both.task_details),
                        (Panel::EventList, both.event_panel),
                    ] {
                        if other != panel {
                            assert!(r.is_empty());
                        }
                    }
                }
                assert_eq!(both.usage_chart, both.list);
                event_screen(&view, width, 45, now);
                zoom_key(&mut view, '=', KeyModifiers::NONE);
                assert_eq!(zoom_rects(view.center_layout(area)), zoom_rects(original));
                zoom_key(&mut view, '|', modifiers);
                let wide = view.center_layout(area);
                assert_eq!(
                    rect(&wide).width,
                    if panel == Panel::EventDetail {
                        width
                    } else {
                        width - 2
                    }
                );
                assert_eq!(rect(&wide).height, rect(&original).height);
                assert_eq!(wide.prompt, original.prompt);
                zoom_key(&mut view, '=', KeyModifiers::NONE);
                assert_eq!(zoom_rects(view.center_layout(area)), zoom_rects(original));
            }
        }
    }
}

#[test]
fn panel_zoom_reveals_hidden_focus_and_ignores_prompt() {
    for width in [120, 200] {
        for direction in ['j', 'l'] {
            let (mut view, now) = event_fixture();
            let area = Rect::new(0, 0, width, 45);
            event_screen(&view, width, 45, now);
            let baseline = zoom_rects(view.center_layout(area));
            zoom_key(&mut view, '_', KeyModifiers::SHIFT);
            zoom_key(&mut view, '|', KeyModifiers::SHIFT);
            event_screen(&view, width, 45, now);
            if direction == 'l' {
                chord(&mut view, direction);
            } else {
                view.set_active_panel(Panel::EventDetail);
            }
            assert_eq!(zoom_rects(view.center_layout(area)), baseline);
            view.set_active_panel(Panel::Prompt);
            zoom_key(&mut view, '_', KeyModifiers::SHIFT);
            zoom_key(&mut view, '|', KeyModifiers::SHIFT);
            assert_eq!(zoom_rects(view.center_layout(area)), baseline);
        }
    }
}

#[test]
fn panel_zoom_zero_sized_regions_render_without_panics() {
    for width in [1, 8, 120, 200] {
        for height in [0, 1, 5, 18, 45] {
            for panel in [
                Panel::TaskList,
                Panel::TaskDetails,
                Panel::EventList,
                Panel::EventDetail,
            ] {
                let (mut view, now) = event_fixture();
                event_screen(&view, width, height, now);
                view.set_active_panel(panel);
                for c in ['_', '|', '='] {
                    zoom_key(&mut view, c, KeyModifiers::SHIFT);
                    event_screen(&view, width, height, now);
                }
            }
        }
    }
}

#[test]
fn panel_zoom_frame_rules_survive_collapsed_panels() {
    for width in [120, 200] {
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            for commands in ["", "_", "|", "_|", "|_"] {
                let (mut view, now) = event_fixture();
                let area = Rect::new(0, 0, width, 50);
                event_screen(&view, width, 50, now);
                view.set_active_panel(panel);
                for command in commands.chars() {
                    zoom_key(&mut view, command, KeyModifiers::SHIFT);
                }
                let mut buf = Buffer::empty(area);
                view.render_at(area, &mut buf, now);
                let layout = view.center_layout(area);
                let context = format!("{width} {panel:?} {commands}");
                for y in [
                    layout.prompt.y - 1,
                    layout.prompt.bottom(),
                    layout.footer.y - 1,
                ] {
                    for x in [0, width - 1] {
                        assert!(
                            matches!(buf[(x, y)].symbol(), "─" | "┴"),
                            "{context}: ({x}, {y}) = {:?}",
                            buf[(x, y)].symbol()
                        );
                    }
                }
                let viewport = view.viewport.borrow();
                if !layout.task_details.is_empty() {
                    assert_eq!(
                        viewport.task_details_title.y,
                        layout.prompt.y - 1,
                        "{context}"
                    );
                    if width == 200 && !layout.event_panel.is_empty() {
                        assert_eq!(
                            viewport.task_details_title.y,
                            layout.event_panel.bottom(),
                            "{context}"
                        );
                    }
                }
                if layout.event_detail.is_empty() {
                    assert!(
                        viewport.output_borders.iter().all(|rect| rect.is_empty()),
                        "{context}"
                    );
                    if width == 120 && !layout.event_panel.is_empty() {
                        assert_eq!(
                            layout.event_panel.bottom(),
                            layout.footer.y - 1,
                            "{context}"
                        );
                    } else {
                        assert_eq!(layout.prompt.bottom(), layout.footer.y - 1, "{context}");
                    }
                } else {
                    assert_eq!(
                        viewport.output_borders[0].y, layout.event_detail.y,
                        "{context}"
                    );
                    assert_eq!(
                        viewport.output_borders[1].y,
                        layout.footer.y - 1,
                        "{context}"
                    );
                }
                if !layout.event_panel.is_empty() {
                    assert_eq!(
                        viewport.event_borders[1].y,
                        layout.event_panel.bottom(),
                        "{context}"
                    );
                } else {
                    assert!(
                        viewport.event_borders.iter().all(|rect| rect.is_empty()),
                        "{context}"
                    );
                }
                let borders = [
                    (Panel::EventDetail, viewport.output_borders),
                    (Panel::EventList, viewport.event_borders),
                ];
                drop(viewport);
                for (panel, borders) in borders {
                    for border in borders.into_iter().filter(|rect| !rect.is_empty()) {
                        let position = (border.right() - 1, border.y).into();
                        let output = view
                            .viewport
                            .borrow()
                            .output_borders
                            .iter()
                            .any(|rect| rect.contains(position));
                        view.handle_mouse(mouse(
                            MouseEventKind::ScrollRight,
                            border.right() - 1,
                            border.y,
                        ));
                        assert_eq!(
                            view.active_panel,
                            if output { Panel::EventDetail } else { panel },
                            "{context} {border:?}"
                        );
                    }
                }
            }
        }
    }
}

fn prompt_up_without_zoom(width: u16) {
    let (mut view, now) = event_fixture();
    event_screen(&view, width, 50, now);
    view.set_active_panel(Panel::Prompt);
    chord(&mut view, 'k');
    assert_eq!(view.active_panel, Panel::TaskList);
}

#[test]
fn wide_prompt_up_without_zoom_keeps_task_list_destination() {
    prompt_up_without_zoom(200);
}

#[test]
fn narrow_prompt_up_without_zoom_keeps_task_list_destination() {
    prompt_up_without_zoom(120);
}

#[test]
fn panel_zoom_prompt_up_returns_to_width_zoomed_top_panel() {
    for width in [120, 200] {
        for panel in [Panel::TaskList, Panel::TaskDetails, Panel::EventList] {
            if width == 120 && panel == Panel::EventList {
                continue;
            }
            for commands in ["_", "|", "_|", "|_"] {
                let (mut view, now) = event_fixture();
                event_screen(&view, width, 50, now);
                view.set_active_panel(panel);
                for command in commands.chars() {
                    zoom_key(&mut view, command, KeyModifiers::SHIFT);
                }
                event_screen(&view, width, 50, now);
                chord(&mut view, 'j');
                assert_eq!(view.active_panel, Panel::Prompt);
                event_screen(&view, width, 50, now);
                let zoom = (view.height_zoom, view.width_zoom);
                chord(&mut view, 'k');
                let expected = if commands.contains('|') {
                    panel
                } else {
                    Panel::TaskList
                };
                assert_eq!(view.active_panel, expected, "{width} {panel:?} {commands}");
                assert_eq!((view.height_zoom, view.width_zoom), zoom);
            }
        }
    }
}
