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
    let events = crate::providers::claude::log::tail(&path, usize::MAX).unwrap();
    let prompt = events
        .iter()
        .find(|event| event.kind == crate::log::Kind::User)
        .unwrap()
        .body
        .repeat(8);
    let event = events
        .iter()
        .find(|event| event.kind == crate::log::Kind::Assistant)
        .unwrap();
    let now = event.started_at;
    let mut view = fixture();
    let row = &mut view.rows[view.selected];
    row.prompt = prompt;
    row.last_message = Some(event.body.repeat(8));
    row.transcript = Some(path);
    view.log_session = view.selected_id().cloned();
    view.set_log(
        (0..60)
            .map(|index| {
                let mut event = event.clone();
                event.source = Some(format!("active-panel-{index}"));
                event.body = event.body.repeat(8);
                event
            })
            .collect(),
    );
    view.select_log_event(Some(20));
    log_screen(&view, width, 45, now);
    (view, now)
}

fn details_position(view: &View, panel: Panel) -> usize {
    let viewport = view.viewport.borrow();
    match panel {
        Panel::Details => viewport.details_scroll.position(),
        Panel::Output => viewport.log_detail_state.position(),
        _ => unreachable!("details panel"),
    }
}

fn assert_lists_unchanged(view: &View, selected: usize, event: Option<usize>, scroll: usize) {
    assert_eq!(view.selected, selected, "task selection changed");
    assert_eq!(view.log_selected, event, "log selection changed");
    assert_eq!(view.viewport.borrow().log_scroll, scroll, "log list moved");
}

#[test]
fn active_panel_details_navigation_scrolls_only_its_content() {
    for width in [100, 200] {
        for panel in [Panel::Details, Panel::Output] {
            for (forward, backward) in navigation_pairs() {
                let (mut view, now) = panel_fixture(width);
                view.set_active_panel(panel);
                let selected = view.selected;
                let event = view.log_selected;
                let scroll = view.viewport.borrow().log_scroll;
                let other = if panel == Panel::Details {
                    Panel::Output
                } else {
                    Panel::Details
                };
                let other_before = details_position(&view, other);
                let before = details_position(&view, panel);
                assert_eq!(view.handle_key(forward), None);
                let after = details_position(&view, panel);
                assert!(after > before, "width={width}, key={forward:?}");
                assert_lists_unchanged(&view, selected, event, scroll);
                assert_eq!(details_position(&view, other), other_before);
                log_screen(&view, width, 45, now);
                assert_eq!(view.handle_key(backward), None);
                assert!(
                    details_position(&view, panel) < after,
                    "width={width}, key={backward:?}"
                );
                assert_lists_unchanged(&view, selected, event, scroll);
                assert_eq!(details_position(&view, other), other_before);
            }
        }
    }
}

#[test]
fn active_panel_detail_boundaries_do_not_navigate_lists() {
    for width in [100, 200] {
        for panel in [Panel::Details, Panel::Output] {
            for (forward, backward) in navigation_pairs() {
                let (mut view, now) = panel_fixture(width);
                view.set_active_panel(panel);
                let selected = view.selected;
                let event = view.log_selected;
                let scroll = view.viewport.borrow().log_scroll;
                key(&mut view, KeyCode::End);
                log_screen(&view, width, 45, now);
                let bottom = details_position(&view, panel);
                assert!(bottom > 0);
                view.handle_key(forward);
                assert_eq!(details_position(&view, panel), bottom);
                assert_lists_unchanged(&view, selected, event, scroll);
                key(&mut view, KeyCode::Home);
                log_screen(&view, width, 45, now);
                assert_eq!(details_position(&view, panel), 0);
                view.handle_key(backward);
                assert_eq!(details_position(&view, panel), 0);
                assert_lists_unchanged(&view, selected, event, scroll);
            }
        }
    }
}

#[test]
fn active_panel_short_log_detail_does_not_change_event() {
    for width in [100, 200] {
        for (forward, backward) in navigation_pairs() {
            let (mut view, now) = panel_fixture(width);
            let mut events = view.log.clone();
            for event in &mut events {
                event.body = "short".into();
            }
            view.set_log(events);
            view.select_log_event(Some(20));
            view.set_active_panel(Panel::Output);
            log_screen(&view, width, 45, now);
            let selected = view.selected;
            let event = view.log_selected;
            let scroll = view.viewport.borrow().log_scroll;
            view.handle_key(forward);
            view.handle_key(backward);
            assert_lists_unchanged(&view, selected, event, scroll);
        }
    }
}

#[test]
fn active_panel_lists_keep_their_navigation_bindings() {
    for width in [100, 200] {
        for panel in [Panel::TaskList, Panel::LogList] {
            for (forward, backward) in navigation_pairs() {
                let (mut view, _) = panel_fixture(width);
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
                    let event = view.log_selected;
                    view.handle_key(forward);
                    assert_ne!(view.log_selected, event, "width={width}, key={forward:?}");
                    let after = view.log_selected;
                    view.handle_key(backward);
                    assert_ne!(view.log_selected, after, "width={width}, key={backward:?}");
                    assert_eq!(view.selected, selected);
                }
            }
        }
    }
}

#[test]
fn active_panel_disabled_prompt_consumes_navigation_without_fallback() {
    for width in [100, 200] {
        for (forward, backward) in navigation_pairs() {
            let (mut view, now) = panel_fixture(width);
            view.rows[view.selected].process = Some(crate::model::Process {
                pid: 1,
                label: None,
                started_at: None,
            });
            view.set_active_panel(Panel::Prompt);
            assert!(view.input_disabled().is_some());
            log_screen(&view, width, 45, now);
            let selected = view.selected;
            let event = view.log_selected;
            let scroll = view.viewport.borrow().log_scroll;
            let details = details_position(&view, Panel::Details);
            let log_details = details_position(&view, Panel::Output);
            assert_eq!(view.handle_key(forward), None);
            assert_eq!(view.handle_key(backward), None);
            assert_lists_unchanged(&view, selected, event, scroll);
            assert_eq!(details_position(&view, Panel::Details), details);
            assert_eq!(details_position(&view, Panel::Output), log_details);
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
        for panel in [
            Panel::TaskList,
            Panel::Details,
            Panel::LogList,
            Panel::Output,
            Panel::Prompt,
        ] {
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
            view.sync_questions();
            view.set_active_panel(panel);
            log_screen(&view, width, 45, now);
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
        for panel in [Panel::LogList, Panel::Output, Panel::Prompt] {
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
            let event = view.log_selected;
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
                assert_eq!(key(&mut view, code), None, "panel={panel:?} code={code:?}");
                assert_eq!(view.selected, selected);
                assert_eq!(view.log_selected, event);
                assert!(view.confirm_dialog.is_none());
                assert!(view.rename_target.is_none());
            }
        }
    }
}

#[test]
fn active_panel_sort_key_belongs_to_log_list() {
    for panel in [Panel::TaskList, Panel::Details, Panel::Output] {
        let (mut view, _) = panel_fixture(200);
        view.set_active_panel(panel);
        let sort = view.log_sort;
        assert_eq!(key(&mut view, KeyCode::Char('t')), None);
        assert_eq!(view.log_sort, sort);
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
        log_screen(&view, width, 45, now);
        assert_eq!(view.cursor_pos(Rect::new(0, 0, width, 45)), None);
        view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert_ne!(view.selected, selected);
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
        log_screen(&view, width, 45, now);
        key(&mut view, KeyCode::Home);
        let caret = view.input_box.editor.cursor();
        let selected = view.selected;
        let event = view.log_selected;
        let scroll = view.viewport.borrow().log_scroll;
        let details = details_position(&view, Panel::Details);
        let output = details_position(&view, Panel::Output);
        view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(view.input_box.editor.cursor() > caret);
        view.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(view.input_box.editor.cursor(), caret);
        assert_lists_unchanged(&view, selected, event, scroll);
        assert_eq!(details_position(&view, Panel::Details), details);
        assert_eq!(details_position(&view, Panel::Output), output);
    }
}
