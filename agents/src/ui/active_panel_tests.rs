use super::*;

fn navigation_pairs() -> [(KeyEvent, KeyEvent); 7] {
    let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let control = |character| KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
    [
        (control('f'), control('b')),
        (plain(KeyCode::PageDown), plain(KeyCode::PageUp)),
        (plain(KeyCode::Down), plain(KeyCode::Up)),
        (plain(KeyCode::Char('j')), plain(KeyCode::Char('k'))),
        (control('n'), control('p')),
        (control('j'), control('k')),
        (plain(KeyCode::End), plain(KeyCode::Home)),
    ]
}

fn panel_fixture(width: u16) -> (View, DateTime<Utc>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transcript.jsonl");
    let events = crate::providers::claude::event_reader::tail(&path, usize::MAX).unwrap();
    let prompt = events
        .iter()
        .find(|event| event.kind == crate::event::Kind::User)
        .unwrap()
        .body
        .repeat(2);
    let event = events
        .iter()
        .find(|event| event.kind == crate::event::Kind::Assistant)
        .unwrap();
    let now = event.started_at;
    let mut view = fixture();
    let row = &mut view.rows[view.selected];
    row.prompt = prompt;
    row.last_message = Some(event.body.repeat(2));
    row.transcript = Some(path);
    view.event_session = view.selected_id().cloned();
    // Only the selected event and the neighbours a wheel at the detail's edge
    // steps to need a detail that overflows its panel; the rest stay the
    // fixture's real size.
    // At height 45 the widest layout shows 24 event rows, so 25 still pages.
    view.set_event(
        (0..25)
            .map(|index| {
                let mut event = event.clone();
                event.source = Some(format!("active-panel-{index}"));
                if (19..=21).contains(&index) {
                    event.body = event.body.repeat(8);
                }
                event
            })
            .collect(),
    );
    view.select_event(Some(20));
    event_screen(&view, width, 45, now);
    assert!(view.event_panel.len() > usize::from(view.viewport.borrow().event_panel.height));
    (view, now)
}

fn task_details_position(view: &View, panel: Panel) -> usize {
    let viewport = view.viewport.borrow();
    match panel {
        Panel::TaskDetails => viewport.task_details_scroll.position(),
        Panel::EventDetail => view.event_detail.scroll.borrow().state.position(),
        _ => unreachable!("details panel"),
    }
}

fn assert_lists_unchanged(view: &View, selected: usize, event: Option<usize>, scroll: usize) {
    assert_eq!(view.selected, selected, "task selection changed");
    assert_eq!(view.event_list.cursor, event, "log selection changed");
    assert_eq!(view.event_list.scroll.get(), scroll, "log list moved");
}

#[test]
fn active_panel_task_details_navigation_scrolls_only_its_content() {
    for width in [100, 200] {
        for panel in [Panel::TaskDetails, Panel::EventDetail] {
            let (mut view, now) = panel_fixture(width);
            view.set_active_panel(panel);
            for (forward, backward) in navigation_pairs() {
                key(&mut view, KeyCode::Home);
                let selected = view.selected;
                let event = view.event_list.cursor;
                let scroll = view.event_list.scroll.get();
                let other = if panel == Panel::TaskDetails {
                    Panel::EventDetail
                } else {
                    Panel::TaskDetails
                };
                let other_before = task_details_position(&view, other);
                let before = task_details_position(&view, panel);
                assert_eq!(view.handle_key(forward), None);
                let after = task_details_position(&view, panel);
                assert!(after > before, "width={width}, key={forward:?}");
                assert_lists_unchanged(&view, selected, event, scroll);
                assert_eq!(task_details_position(&view, other), other_before);
                event_screen(&view, width, 45, now);
                assert_eq!(view.handle_key(backward), None);
                assert!(
                    task_details_position(&view, panel) < after,
                    "width={width}, key={backward:?}"
                );
                assert_lists_unchanged(&view, selected, event, scroll);
                assert_eq!(task_details_position(&view, other), other_before);
            }
        }
    }
}

#[test]
fn active_panel_detail_bottom_forward_keys_do_not_navigate_lists() {
    for width in [100, 200] {
        for panel in [Panel::TaskDetails, Panel::EventDetail] {
            let (mut view, now) = panel_fixture(width);
            view.set_active_panel(panel);
            let selected = view.selected;
            let event = view.event_list.cursor;
            let scroll = view.event_list.scroll.get();
            key(&mut view, KeyCode::End);
            event_screen(&view, width, 45, now);
            let bottom = task_details_position(&view, panel);
            assert!(bottom > 0);
            for (forward, _) in navigation_pairs() {
                if panel == Panel::EventDetail && forward.code != KeyCode::End {
                    continue;
                }
                key(&mut view, KeyCode::End);
                view.handle_key(forward);
                assert_eq!(task_details_position(&view, panel), bottom);
                assert_lists_unchanged(&view, selected, event, scroll);
            }
        }
    }
}

#[test]
fn active_panel_detail_top_backward_keys_do_not_navigate_lists() {
    for width in [100, 200] {
        for panel in [Panel::TaskDetails, Panel::EventDetail] {
            let (mut view, now) = panel_fixture(width);
            view.set_active_panel(panel);
            let selected = view.selected;
            let event = view.event_list.cursor;
            let scroll = view.event_list.scroll.get();
            key(&mut view, KeyCode::End);
            event_screen(&view, width, 45, now);
            let bottom = task_details_position(&view, panel);
            assert!(bottom > 0);
            key(&mut view, KeyCode::Home);
            event_screen(&view, width, 45, now);
            assert_eq!(task_details_position(&view, panel), 0);
            for (forward, backward) in navigation_pairs() {
                if panel == Panel::EventDetail && forward.code != KeyCode::End {
                    continue;
                }
                key(&mut view, KeyCode::Home);
                view.handle_key(backward);
                assert_eq!(task_details_position(&view, panel), 0);
                assert_lists_unchanged(&view, selected, event, scroll);
            }
        }
    }
}

#[test]
fn active_panel_event_detail_bottom_steps_to_next_event() {
    for width in [100, 200] {
        let (mut view, now) = panel_fixture(width);
        let panel = Panel::EventDetail;
        view.set_active_panel(panel);
        let event = view.event_list.cursor;
        for (forward, _) in navigation_pairs() {
            if forward.code == KeyCode::End {
                continue;
            }
            view.select_event(Some(20));
            key(&mut view, KeyCode::End);
            event_screen(&view, width, 45, now);
            let bottom = task_details_position(&view, panel);
            assert!(bottom > 0);
            // Like the wheel, the event detail's step keys leave for the next event.
            view.handle_key(forward);
            assert_eq!(view.event_list.cursor, event.map(|e| e + 1));
        }
    }
}

#[test]
fn active_panel_short_event_detail_steps_between_events() {
    for width in [100, 200] {
        let (mut view, now) = panel_fixture(width);
        let mut events = view.event_panel.clone();
        for event in &mut events {
            event.body = "short".into();
        }
        view.set_event(events);
        view.set_active_panel(Panel::EventDetail);
        for (forward, backward) in navigation_pairs() {
            view.select_event(Some(20));
            event_screen(&view, width, 45, now);
            let selected = view.selected;
            let event = view.event_list.cursor;
            let scroll = view.event_list.scroll.get();
            view.handle_key(forward);
            if forward.code != KeyCode::End {
                assert_eq!(view.event_list.cursor, event.map(|e| e + 1));
                event_screen(&view, width, 45, now);
            }
            view.handle_key(backward);
            assert_lists_unchanged(&view, selected, event, scroll);
        }
    }
}

#[test]
fn active_panel_lists_keep_their_navigation_bindings() {
    for width in [100, 200] {
        for panel in [Panel::TaskList, Panel::EventList] {
            let (mut view, _) = panel_fixture(width);
            let initial_selected = view.selected;
            for (forward, backward) in navigation_pairs() {
                view.set_selected(initial_selected);
                view.select_event(Some(20));
                view.set_active_panel(panel);
                if panel == Panel::TaskList {
                    let selected = view.selected;
                    view.handle_key(forward);
                    assert_ne!(view.selected, selected, "width={width}, key={forward:?}");
                    let after = view.selected;
                    view.handle_key(backward);
                    assert_ne!(view.selected, after, "width={width}, key={backward:?}");
                } else {
                    let selected = view.selected;
                    let event = view.event_list.cursor;
                    view.handle_key(forward);
                    assert_ne!(
                        view.event_list.cursor, event,
                        "width={width}, key={forward:?}"
                    );
                    let after = view.event_list.cursor;
                    view.handle_key(backward);
                    assert_ne!(
                        view.event_list.cursor, after,
                        "width={width}, key={backward:?}"
                    );
                    assert_eq!(view.selected, selected);
                }
            }
        }
    }
}

#[test]
fn active_panel_disabled_prompt_consumes_navigation_without_fallback() {
    for width in [100, 200] {
        let (mut view, now) = panel_fixture(width);
        view.rows[view.selected].process = Some(crate::model::Process {
            pid: 1,
            label: None,
            started_at: None,
        });
        view.set_active_panel(Panel::Prompt);
        assert!(view.input_disabled().is_some());
        event_screen(&view, width, 45, now);
        let selected = view.selected;
        let event = view.event_list.cursor;
        let scroll = view.event_list.scroll.get();
        let task_details = task_details_position(&view, Panel::TaskDetails);
        let event_detail_position = task_details_position(&view, Panel::EventDetail);
        for (forward, backward) in navigation_pairs() {
            assert_eq!(view.handle_key(forward), None);
            assert_eq!(view.handle_key(backward), None);
            assert_lists_unchanged(&view, selected, event, scroll);
            assert_eq!(
                task_details_position(&view, Panel::TaskDetails),
                task_details
            );
            assert_eq!(
                task_details_position(&view, Panel::EventDetail),
                event_detail_position
            );
        }
    }
}

#[test]
fn active_panel_question_keys_require_prompt_focus() {
    let record: serde_json::Value =
        include_str!("../../tests/fixtures/claude-questions/transcript.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| {
                record["message"]["content"]
                    .as_array()
                    .is_some_and(|parts| {
                        parts
                            .iter()
                            .any(|part| part["input"]["questions"][0]["header"] == "Single")
                    })
            })
            .unwrap();
    let input = record["message"]["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["name"] == "AskUserQuestion")
        .unwrap();
    let question = &input["input"]["questions"][0];
    for width in [100, 200] {
        let (mut view, now) = panel_fixture(width);
        let row = &mut view.rows[view.selected];
        row.status = SessionStatus::NeedsInput;
        row.background_job = Some(crate::model::BackgroundJob {
            id: "active-panel-question".into(),
            needs: None,
            awaits_prompt: false,
        });
        row.process = Some(crate::model::Process {
            pid: 1,
            label: Some("bg".into()),
            started_at: None,
        });
        row.ask = Some(crate::ask::Ask {
            id: input["id"].as_str().unwrap().into(),
            session: row.id.clone(),
            since: row.updated_at,
            kind: crate::ask::AskKind::Questions {
                blocking: true,
                questions: vec![crate::ask::Question {
                    id: question["question"].as_str().unwrap().into(),
                    text: question["question"].as_str().unwrap().into(),
                    header: question["header"].as_str().unwrap().into(),
                    multi: false,
                    other: true,
                    secret: false,
                    options: serde_json::from_value(question["options"].clone()).unwrap(),
                }],
            },
        });
        view.show_questions();
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
            Panel::Prompt,
        ] {
            view.set_active_panel(panel);
            event_screen(&view, width, 45, now);
            assert!(view.active_question().is_some());
            let action = key(&mut view, KeyCode::Char('2'));
            assert_eq!(
                matches!(action, Some(Action::AnswerQuestions { .. })),
                panel == Panel::Prompt,
                "width={width}"
            );
        }
    }
}

#[test]
fn active_panel_unsupported_task_keys_do_not_fall_through() {
    for width in [100, 200] {
        for panel in [Panel::EventList, Panel::EventDetail, Panel::Prompt] {
            let (mut view, _) = panel_fixture(width);
            view.set_active_panel(panel);
            // A disabled prompt and an absent terminal must not fall back to tasks.
            if panel == Panel::Prompt {
                view.rows[view.selected].process = Some(crate::model::Process {
                    pid: 1,
                    label: None,
                    started_at: None,
                });
            }
            let selected = view.selected;
            let event = view.selected_event_row();
            for code in [
                KeyCode::Right,
                KeyCode::Char('f'),
                KeyCode::Char('r'),
                KeyCode::Char('a'),
                KeyCode::Char('x'),
                KeyCode::Char('h'),
                KeyCode::Char('p'),
                KeyCode::Backspace,
            ] {
                let action = key(&mut view, code);
                if matches!(panel, Panel::EventList | Panel::EventDetail)
                    && code == KeyCode::Char('p')
                {
                    let selected = view.visible_event().unwrap();
                    assert!(matches!(
                        action,
                        Some(Action::ToggleEventPin { task, pinned })
                            if Some(&task) == view.selected_id()
                                && pinned.len() == 1
                                && Some(&pinned[0].source) == selected.source.as_ref()
                                && pinned[0].records == selected.source_records
                    ));
                } else {
                    assert_eq!(action, None, "panel={panel:?} code={code:?}");
                }
                assert_eq!(view.selected, selected);
                assert_eq!(view.selected_event_row(), event);
                assert!(view.confirm_dialog.is_none());
                assert!(view.rename_target.is_none());
            }
        }
    }
}

#[test]
fn active_panel_sort_key_belongs_to_event_list() {
    for panel in [Panel::TaskList, Panel::TaskDetails, Panel::EventDetail] {
        let (mut view, _) = panel_fixture(200);
        view.set_active_panel(panel);
        let sort = view.event_list.sort;
        assert_eq!(key(&mut view, KeyCode::Char('t')), None);
        assert_eq!(view.event_list.sort, sort);
    }
}

#[test]
fn active_panel_title_editor_does_not_take_other_panels_keys_or_cursor() {
    for width in [100, 200] {
        let (mut view, now) = panel_fixture(width);
        view.rename_target = view.selected_id().cloned();
        view.rename.set_text("rename draft");
        view.set_active_panel(Panel::TaskList);
        let selected = view.selected;
        event_screen(&view, width, 45, now);
        assert_eq!(view.cursor_pos(Rect::new(0, 0, width, 45)), None);
        view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert_ne!(view.selected, selected);
        view.select(selected);
        assert_eq!(view.rename.text(), "rename draft");
    }
}

#[test]
fn active_panel_prompt_moves_its_caret_without_moving_other_panels() {
    for width in [100, 200] {
        let (mut view, now) = panel_fixture(width);
        view.rows[view.selected].process = None;
        view.set_active_panel(Panel::Prompt);
        view.input_box
            .editor
            .set_text(include_str!("../../tests/fixtures/claude-peer-message.txt"));
        event_screen(&view, width, 45, now);
        key(&mut view, KeyCode::Home);
        let caret = view.input_box.editor.cursor();
        let selected = view.selected;
        let event = view.event_list.cursor;
        let scroll = view.event_list.scroll.get();
        let task_details = task_details_position(&view, Panel::TaskDetails);
        let output = task_details_position(&view, Panel::EventDetail);
        view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(view.input_box.editor.cursor() > caret);
        view.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(view.input_box.editor.cursor(), caret);
        assert_lists_unchanged(&view, selected, event, scroll);
        assert_eq!(
            task_details_position(&view, Panel::TaskDetails),
            task_details
        );
        assert_eq!(task_details_position(&view, Panel::EventDetail), output);
    }
}

fn mouse_panel_area(view: &View, panel: Panel) -> Rect {
    let viewport = view.viewport.borrow();
    match panel {
        Panel::TaskList => viewport.list,
        Panel::TaskDetails => viewport.task_details,
        Panel::EventList => viewport.event_panel,
        Panel::EventDetail => viewport.event_detail,
        Panel::Prompt => viewport.prompt,
    }
}

fn assert_mouse_panel_paging(view: &mut View, panel: Panel) {
    assert_eq!(view.active_panel, panel);
    key(view, KeyCode::Home);
    let before = (
        view.selected,
        view.event_list.cursor,
        task_details_position(view, Panel::TaskDetails),
        task_details_position(view, Panel::EventDetail),
    );
    view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    let after = (
        view.selected,
        view.event_list.cursor,
        task_details_position(view, Panel::TaskDetails),
        task_details_position(view, Panel::EventDetail),
    );
    match panel {
        Panel::TaskList => assert_ne!(after.0, before.0),
        Panel::EventList => {
            assert_ne!(after.1, before.1);
            assert_eq!(after.0, before.0);
        }
        Panel::TaskDetails | Panel::EventDetail => {
            assert_eq!((after.0, after.1), (before.0, before.1));
            if panel == Panel::TaskDetails {
                assert!(after.2 > before.2);
                assert_eq!(after.3, before.3);
            } else {
                assert!(after.3 > before.3);
                assert_eq!(after.2, before.2);
            }
        }
        Panel::Prompt => assert_eq!(after, before),
    }
}

const MOUSE_ACTIVATION_PANELS: [Panel; 5] = [
    Panel::TaskList,
    Panel::TaskDetails,
    Panel::EventList,
    Panel::EventDetail,
    Panel::Prompt,
];

const MOUSE_ACTIVATION_EVENTS: [MouseEventKind; 13] = [
    MouseEventKind::ScrollUp,
    MouseEventKind::ScrollDown,
    MouseEventKind::ScrollLeft,
    MouseEventKind::ScrollRight,
    MouseEventKind::Down(MouseButton::Left),
    MouseEventKind::Up(MouseButton::Left),
    MouseEventKind::Down(MouseButton::Right),
    MouseEventKind::Up(MouseButton::Right),
    MouseEventKind::Down(MouseButton::Middle),
    MouseEventKind::Up(MouseButton::Middle),
    MouseEventKind::Drag(MouseButton::Left),
    MouseEventKind::Drag(MouseButton::Right),
    MouseEventKind::Drag(MouseButton::Middle),
];

fn activate_mouse_panel(view: &mut View, width: u16, panel: Panel, kind: MouseEventKind) {
    let area = mouse_panel_area(view, panel);
    assert!(!area.is_empty());
    view.set_active_panel(if panel == Panel::TaskList {
        Panel::EventDetail
    } else {
        Panel::TaskList
    });
    view.handle_mouse(mouse(kind, area.x, area.y + (area.height - 1).min(1)));
    assert_eq!(view.active_panel, panel, "width={width} kind={kind:?}");
}

#[test]
fn mouse_activation_every_event_activates_its_panel() {
    for width in [100, 200] {
        let (mut view, _) = panel_fixture(width);
        for panel in MOUSE_ACTIVATION_PANELS {
            for kind in MOUSE_ACTIVATION_EVENTS {
                activate_mouse_panel(&mut view, width, panel, kind);
            }
        }
    }
}

#[test]
fn mouse_activation_every_event_routes_subsequent_paging() {
    const CASES: [(u16, Panel, MouseEventKind); 12] = [
        (
            200,
            Panel::TaskList,
            MouseEventKind::Down(MouseButton::Left),
        ),
        (200, Panel::TaskList, MouseEventKind::ScrollDown),
        (
            200,
            Panel::TaskDetails,
            MouseEventKind::Down(MouseButton::Left),
        ),
        (200, Panel::TaskDetails, MouseEventKind::ScrollDown),
        (
            200,
            Panel::EventList,
            MouseEventKind::Down(MouseButton::Left),
        ),
        (200, Panel::EventList, MouseEventKind::ScrollDown),
        (
            200,
            Panel::EventDetail,
            MouseEventKind::Down(MouseButton::Left),
        ),
        (200, Panel::EventDetail, MouseEventKind::ScrollDown),
        (200, Panel::EventDetail, MouseEventKind::ScrollUp),
        (200, Panel::Prompt, MouseEventKind::Down(MouseButton::Left)),
        (
            200,
            Panel::Prompt,
            MouseEventKind::Down(MouseButton::Middle),
        ),
        (
            100,
            Panel::EventList,
            MouseEventKind::Down(MouseButton::Left),
        ),
    ];

    for (width, panel, kind) in CASES {
        let (mut view, _) = panel_fixture(width);
        activate_mouse_panel(&mut view, width, panel, kind);
        assert_mouse_panel_paging(&mut view, panel);
    }
}

fn press_mouse_panel(view: &mut View, panel: Panel) -> Rect {
    let area = mouse_panel_area(view, panel);
    view.set_active_panel(if panel == Panel::TaskList {
        Panel::EventDetail
    } else {
        Panel::TaskList
    });
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x,
        area.y + (area.height - 1).min(1),
    ));
    assert_eq!(view.active_panel, panel);
    area
}

#[test]
fn mouse_activation_double_click_keeps_the_pressed_panel() {
    for width in [100, 200] {
        for panel in MOUSE_ACTIVATION_PANELS {
            let (mut view, _) = panel_fixture(width);
            let area = press_mouse_panel(&mut view, panel);
            view.handle_mouse(mouse(
                MouseEventKind::Up(MouseButton::Left),
                area.x,
                area.y + (area.height - 1).min(1),
            ));
            view.handle_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                area.x,
                area.y + (area.height - 1).min(1),
            ));
            view.handle_mouse(mouse(
                MouseEventKind::Up(MouseButton::Left),
                area.x,
                area.y + (area.height - 1).min(1),
            ));
            assert_mouse_panel_paging(&mut view, panel);
        }
    }
}

#[test]
fn mouse_activation_drag_keeps_the_pressed_panel() {
    for width in [100, 200] {
        for panel in MOUSE_ACTIVATION_PANELS {
            let (mut view, _) = panel_fixture(width);
            press_mouse_panel(&mut view, panel);
            let other = mouse_panel_area(
                &view,
                if panel == Panel::TaskList {
                    Panel::TaskDetails
                } else {
                    Panel::TaskList
                },
            );
            view.handle_mouse(mouse(
                MouseEventKind::Drag(MouseButton::Left),
                other.x,
                other.y + 1,
            ));
            assert_eq!(view.active_panel, panel);
            view.handle_mouse(mouse(
                MouseEventKind::Up(MouseButton::Left),
                other.x,
                other.y + 1,
            ));
            assert_mouse_panel_paging(&mut view, panel);
        }
    }
}

#[test]
fn mouse_activation_outside_and_hover_preserve_focus() {
    for width in [100, 200] {
        let (mut view, _) = panel_fixture(width);
        view.set_active_panel(Panel::EventDetail);
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::ScrollDown,
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::Up(MouseButton::Middle),
        ] {
            view.handle_mouse(mouse(kind, width - 1, 44));
            assert_eq!(view.active_panel, Panel::EventDetail, "outside: {kind:?}");
        }
        let area = mouse_panel_area(&view, Panel::TaskDetails);
        view.handle_mouse(mouse(MouseEventKind::Moved, area.x, area.y));
        assert_eq!(view.active_panel, Panel::EventDetail);
        // A press outside a panel cannot acquire one by dragging into it.
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            width - 1,
            44,
        ));
        view.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            area.x,
            area.y,
        ));
        view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), area.x, area.y));
        assert_eq!(view.active_panel, Panel::EventDetail);
    }
}

#[test]
fn mouse_activation_terminal_events_activate_before_forwarding() {
    for width in [100, 200] {
        let (mut view, now) = panel_fixture(width);
        let mut screen = crate::session_terminal::TerminalScreen::new(10, 80);
        screen.process(b"\x1b[?1002h\x1b[?1006h");
        view.show_terminal(
            view.selected_id().unwrap().clone(),
            std::sync::Arc::new(std::sync::Mutex::new(screen)),
        );
        event_screen(&view, width, 45, now);
        for kind in [
            MouseEventKind::ScrollUp,
            MouseEventKind::ScrollDown,
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Middle),
        ] {
            view.set_active_panel(Panel::TaskDetails);
            let area = view.viewport.borrow().terminal;
            assert!(!area.is_empty());
            assert!(
                matches!(
                    view.handle_mouse(mouse(kind, area.x, area.y)),
                    Some(Action::TerminalInput(_))
                ),
                "{kind:?}"
            );
            assert_eq!(view.active_panel, Panel::EventDetail, "{kind:?}");
            assert_eq!(
                view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
                Some(Action::TerminalInput(vec![6]))
            );
        }
    }
}

#[test]
fn mouse_activation_includes_output_and_event_borders() {
    for width in [100, 200] {
        let (mut view, _) = panel_fixture(width);
        let borders = {
            let viewport = view.viewport.borrow();
            [
                (Panel::EventDetail, viewport.output_borders),
                (Panel::EventList, viewport.event_borders),
            ]
        };
        for (panel, borders) in borders {
            for border in borders {
                view.set_active_panel(Panel::TaskList);
                view.handle_mouse(mouse(
                    MouseEventKind::ScrollRight,
                    border.right() - 1,
                    border.y,
                ));
                let expected = if view
                    .viewport
                    .borrow()
                    .output_borders
                    .iter()
                    .any(|output| output.contains((border.right() - 1, border.y).into()))
                {
                    Panel::EventDetail
                } else {
                    panel
                };
                assert_eq!(
                    view.active_panel, expected,
                    "width={width}, border={border:?}"
                );
            }
        }
    }
}

#[test]
fn mouse_activation_covers_panel_rules_and_side_padding() {
    for width in [100, 200] {
        let (mut view, _) = panel_fixture(width);
        for panel in [
            Panel::TaskList,
            Panel::TaskDetails,
            Panel::EventList,
            Panel::EventDetail,
        ] {
            let area = mouse_panel_area(&view, panel);
            let mut positions = vec![(area.x - 1, area.y + 1), (area.right(), area.y + 1)];
            if matches!(panel, Panel::TaskList | Panel::TaskDetails) {
                positions.extend([(area.x, area.y - 1), (area.x, area.bottom())]);
            }
            if panel == Panel::TaskDetails || (panel == Panel::EventList && width == 200) {
                positions.push((area.x - 2, area.y + 1));
            }
            for (x, y) in positions {
                view.set_active_panel(Panel::Prompt);
                view.handle_mouse(mouse(MouseEventKind::ScrollLeft, x, y));
                assert_eq!(view.active_panel, panel, "width={width}, x={x}, y={y}");
            }
        }
    }
}

#[test]
fn event_detail_p_toggles_pin_except_in_terminal_mode() {
    use super::task_ui::EventDetailMode;
    for mode in [EventDetailMode::Event, EventDetailMode::Transcript] {
        let (mut view, _) = panel_fixture(200);
        view.set_active_panel(Panel::EventDetail);
        view.event_detail.mode = mode;
        let pinned = |action| match action {
            Some(Action::ToggleEventPin { pinned, .. }) => pinned.len(),
            other => panic!("mode={mode:?} action={other:?}"),
        };
        assert_eq!(pinned(key(&mut view, KeyCode::Char('p'))), 1);
        assert_eq!(pinned(key(&mut view, KeyCode::Char('p'))), 0);
    }
    let (mut view, _) = panel_fixture(200);
    view.set_active_panel(Panel::EventDetail);
    view.event_detail.mode = EventDetailMode::Terminal;
    key(&mut view, KeyCode::Char('p'));
    assert!(view.event_list.pinned.is_empty());
}
