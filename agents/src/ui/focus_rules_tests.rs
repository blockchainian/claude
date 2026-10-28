use super::super::task_ui::Focus;
use super::*;

fn tasks() -> View {
    let source = fixture();
    let mut view = test_view(Grouping::Project, false);
    let mut rows = source.rows.iter().take(2).cloned().collect::<Vec<_>>();
    for row in &mut rows {
        row.status = SessionStatus::Ready;
        row.parent = None;
        row.process = None;
    }
    view.set_sessions(rows, false);
    view.select(view.selectable_indices()[0]);
    view
}
fn remembered(view: &View) -> Option<Focus> {
    view.task_uis
        .get(&view.selected_task_key().unwrap())
        .and_then(|ui| ui.focus)
}
fn pending() -> View {
    let mut view = tasks();
    let first = view.selected;
    let other = *view
        .selectable_indices()
        .iter()
        .find(|&&i| i != first)
        .unwrap();
    view.set_active_panel(Panel::EventDetail);
    view.set_active_panel(Panel::TaskList);
    view.select(other);
    view.select(first);
    assert!(view.task_entry_pending);
    view
}
fn panel_mouse(view: &mut View, panel: Panel, kind: MouseEventKind) {
    screen(view, 200, 45);
    let rect = {
        let v = view.viewport.borrow();
        match panel {
            Panel::TaskList => v.list,
            Panel::EventList => v.event_panel,
            Panel::EventDetail => v.event_detail,
            Panel::TaskDetails => v.task_details,
            Panel::Prompt => v.prompt,
        }
    };
    assert!(!rect.is_empty());
    view.handle_mouse(mouse(kind, rect.x + 2, rect.y + 1));
}
fn click_selected(view: &mut View) {
    let rendered = screen(view, 200, 45);
    let title = view.selected_row().unwrap().display_title().to_owned();
    let (x, y) = rendered_cell(&rendered, &title);
    assert_eq!(
        click_mouse(view, mouse(MouseEventKind::Down(MouseButton::Left), x, y)),
        None
    );
}
#[test]
fn rule_1_list_wheel_focuses_list_and_keeps_parked_focus() {
    let mut view = tasks();
    view.prepare_prompt();
    let before = view.selected_task_key();
    panel_mouse(&mut view, Panel::TaskList, MouseEventKind::ScrollDown);
    assert_ne!(view.selected_task_key(), before);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(!view.task_entry_pending);
    assert_eq!(remembered(&view), None);
    screen(&view, 200, 45);
    assert!(view.cursor_pos(Rect::new(0, 0, 200, 45)).is_none());
    panel_mouse(&mut view, Panel::TaskList, MouseEventKind::ScrollUp);
    assert_eq!(view.selected_task_key(), before);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
    assert_eq!(remembered(&view), Some(Focus::Prompt));
}
#[test]
fn rule_2_open_keys_restore_pending_without_terminal() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let mut view = pending();
        assert_eq!(key(&mut view, code), None, "{code:?}");
        assert_eq!(view.active_panel, Panel::EventDetail);
        assert!(!view.task_entry_pending);
        assert!(view.shown_terminal().is_none());
    }
}
#[test]
fn rule_2_click_pending_row_keeps_list_bindings() {
    let mut view = pending();
    click_selected(&mut view);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
    assert!(matches!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin { .. })
    ));
}
#[test]
fn rule_2_selected_row_click_from_task_panel_keeps_parked_focus_pending() {
    let mut view = tasks();
    view.set_active_panel(Panel::EventDetail);
    click_selected(&mut view);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
    assert_eq!(remembered(&view), Some(Focus::EventDetail));
    assert!(matches!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin { .. })
    ));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.active_panel, Panel::EventDetail);
}
#[test]
fn rule_2_row_click_from_prompt_on_another_task_focuses_list() {
    let mut view = tasks();
    let first = view.selected;
    let other = *view
        .selectable_indices()
        .iter()
        .find(|&&i| i != first)
        .unwrap();
    view.prepare_prompt();
    let rendered = screen(&view, 200, 45);
    let title = view.rows[other].display_title().to_owned();
    let (x, y) = rendered_cell(&rendered, &title);
    click_mouse(
        &mut view,
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
    );
    assert_eq!(view.selected, other);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(matches!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin { .. })
    ));
}
#[test]
fn rule_3_open_keys_without_pending_enter_enabled_prompt() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let mut view = tasks();
        assert!(!view.task_entry_pending);
        assert!(view.input_disabled().is_none());
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.active_panel, Panel::Prompt);
        assert!(view.shown_terminal().is_none());
    }
}
#[test]
fn rule_3_open_keys_without_pending_leave_disabled_prompt_alone() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let (mut view, _) = event_fixture();
        assert!(view.input_disabled().is_some());
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.active_panel, Panel::TaskList);
        assert!(!view.task_entry_pending);
    }
}
#[test]
fn rule_3_click_unvisited_row_only_selects() {
    let mut view = tasks();
    assert_eq!(remembered(&view), None);
    click_selected(&mut view);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert_eq!(remembered(&view), None);
    assert!(!view.task_entry_pending);
}
#[test]
fn rule_3_draft_open_keys_enter_prompt() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().join("cwd");
        std::fs::create_dir(&cwd).unwrap();
        let mut view = tasks();
        let id = view.add_draft(DraftSetup {
            provider: ProviderId::Claude,
            cwd,
            model: None,
            effort: None,
            branch: String::new(),
        });
        view.rename.set_text("Focus draft");
        assert_eq!(key(&mut view, KeyCode::Enter), None);
        assert_eq!(view.selected_draft, Some(id));
        view.close_selector();
        view.set_active_panel(Panel::TaskList);
        assert!(!view.task_entry_pending);
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.active_panel, Panel::Prompt);
    }
}
#[test]
fn rule_3_subagent_open_keys_respect_disabled_prompt() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let mut view = tasks();
        let mut rows = view.rows.clone();
        let mut child = rows[0].clone();
        child.id = session_key("child");
        child.title = "Child task".into();
        child.parent = Some(rows[0].id.clone());
        rows.push(child);
        view.set_sessions(rows, false);
        view.select(
            view.rows
                .iter()
                .position(|row| row.id == session_key("child"))
                .unwrap(),
        );
        assert!(view.input_disabled().is_some());
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.active_panel, Panel::TaskList);
    }
}
#[test]
fn rule_4_show_more_open_keys_and_click_keep_loading_behavior() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let mut view = fixture();
        view.status_filter = 1;
        view.has_more = true;
        view.selected = usize::MAX;
        assert_eq!(key(&mut view, code), Some(Action::ShowMore));
        assert!(view.loading);
    }
    let mut view = fixture();
    view.status_filter = 1;
    view.has_more = true;
    view.reconcile_selection();
    let rendered = screen(&view, 200, 45);
    let (x, y) = rendered_cell(&rendered, "Show more");
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), x, y)
        ),
        Some(Action::ShowMore)
    );
}
#[test]
fn rule_4_event_detail_right_click_still_toggles_terminal() {
    use crate::session_terminal::TerminalScreen;
    let (mut view, _) = event_fixture();
    screen(&view, 200, 45);
    let detail = view.viewport.borrow().event_detail;
    assert_eq!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Right),
            detail.x + 2,
            detail.y + 2
        )),
        None
    );
    assert_eq!(
        view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Right),
            detail.x + 2,
            detail.y + 2
        )),
        Some(Action::OpenTerminal(session_key("log")))
    );
    view.show_terminal(
        session_key("log"),
        std::sync::Arc::new(std::sync::Mutex::new(TerminalScreen::new(10, 40))),
    );
    screen(&view, 200, 45);
    let terminal = view.viewport.borrow().terminal;
    assert_eq!(
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Right),
            terminal.x + 2,
            terminal.y + 2
        )),
        None
    );
    assert_eq!(
        view.handle_mouse(mouse(
            MouseEventKind::Up(MouseButton::Right),
            terminal.x + 2,
            terminal.y + 2
        )),
        Some(Action::CloseTerminal)
    );
}
#[test]
fn rule_5_task_mouse_operations_replace_pending_focus() {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::ScrollDown,
        MouseEventKind::ScrollUp,
    ] {
        let mut view = pending();
        panel_mouse(&mut view, Panel::EventList, kind);
        assert_eq!(view.active_panel, Panel::EventList, "{kind:?}");
        assert!(!view.task_entry_pending);
        assert_eq!(remembered(&view), Some(Focus::EventList));
    }
}
#[test]
fn rule_5_hover_keeps_pending_focus() {
    let mut view = pending();
    panel_mouse(&mut view, Panel::EventList, MouseEventKind::Moved);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
    assert_eq!(remembered(&view), Some(Focus::EventDetail));
}
#[test]
fn rule_6_rename_overrides_pending_focus() {
    let mut view = pending();
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert_eq!(view.active_panel, Panel::TaskDetails);
    assert!(view.title_editing());
    assert!(!view.task_entry_pending);
    assert_eq!(remembered(&view), Some(Focus::Title));
}
#[test]
fn rule_6_ctrl_w_overrides_pending_focus() {
    let mut view = pending();
    screen(&view, 200, 45);
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(key(&mut view, KeyCode::Char('l')), None);
    assert_ne!(view.active_panel, Panel::TaskList);
    assert!(!view.task_entry_pending);
    assert_eq!(remembered(&view), Some(view.task_focus()));
}
#[test]
fn rule_6_list_search_does_not_enter_pending_task() {
    let mut view = pending();
    assert_eq!(key(&mut view, KeyCode::Char('/')), None);
    assert!(view.search.focused());
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
    assert_eq!(remembered(&view), Some(Focus::EventDetail));
}
#[test]
fn rule_7_escape_only_selects_previous_task() {
    let mut view = pending();
    let first = view.selected;
    let other = *view
        .selectable_indices()
        .iter()
        .find(|&&i| i != first)
        .unwrap();
    view.select(other);
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.selected, first);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
}
#[test]
fn rule_8_panel_changes_synchronize_focus_and_list_preserves_it() {
    let mut view = tasks();
    for (panel, focus) in [
        (Panel::TaskDetails, Focus::TaskDetails),
        (Panel::EventList, Focus::EventList),
        (Panel::EventDetail, Focus::EventDetail),
        (Panel::Prompt, Focus::Prompt),
    ] {
        view.set_active_panel(panel);
        assert_eq!(remembered(&view), Some(focus));
        view.set_active_panel(Panel::TaskList);
        assert_eq!(remembered(&view), Some(focus));
    }
}
#[test]
fn rule_8_search_in_same_panel_synchronizes_focus() {
    for (panel, focus) in [
        (Panel::EventList, Focus::EventSearch),
        (Panel::EventDetail, Focus::EventDetailSearch),
    ] {
        let (mut view, _) = event_fixture();
        view.set_active_panel(panel);
        assert_eq!(key(&mut view, KeyCode::Char('/')), None);
        assert_eq!(remembered(&view), Some(focus));
    }
}
#[test]
fn rule_9_inactive_clears_only_focus_and_keeps_active_panel() {
    let mut view = tasks_with_working_child();
    view.set_active_panel(Panel::Prompt);
    view.set_active_panel(Panel::TaskList);
    view.set_active_panel(Panel::Prompt);
    assert_eq!(remembered(&view), Some(Focus::Prompt));
    view.input_box.editor.set_text("Unsent draft");
    let id = view.selected_id().unwrap().clone();
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|row| row.id == id).unwrap().status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(view.active_panel, Panel::Prompt);
    assert_eq!(view.input_box.editor.text(), "Unsent draft");
    assert_eq!(remembered(&view), None);
}
#[test]
fn rule_9_inactive_revisit_click_stays_list_and_backspace_opens_delete() {
    let mut view = pending();
    let id = view.selected_id().unwrap().clone();
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|row| row.id == id).unwrap().status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    key(&mut view, KeyCode::Tab);
    view.select(view.rows.iter().position(|row| row.id == id).unwrap());
    assert_eq!(remembered(&view), None);
    assert!(!view.task_entry_pending);
    click_selected(&mut view);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert_eq!(key(&mut view, KeyCode::Backspace), None);
    assert!(
        matches!(view.confirm_dialog.as_ref().map(|dialog| &dialog.action), Some(ConfirmAction::Delete(target)) if target == &id)
    );
    key(&mut view, KeyCode::Esc);
    assert!(view.confirm_dialog.is_none());
}
#[test]
fn rule_10_task_switch_restores_last_panel() {
    let mut view = tasks();
    let first = view.selected;
    let other = *view
        .selectable_indices()
        .iter()
        .find(|&&i| i != first)
        .unwrap();
    view.set_active_panel(Panel::EventList);
    view.set_active_panel(Panel::EventDetail);
    view.select(other);
    assert_eq!(view.active_panel, Panel::Prompt);
    view.select(first);
    assert_eq!(view.active_panel, Panel::EventDetail);
}
#[test]
fn rule_10_unremembered_disabled_task_switch_returns_to_list() {
    let mut view = tasks();
    let other = *view
        .selectable_indices()
        .iter()
        .find(|&&i| i != view.selected)
        .unwrap();
    view.rows[other].process = Some(crate::model::Process {
        pid: 1,
        label: None,
        started_at: None,
    });
    view.set_active_panel(Panel::EventDetail);
    view.select(other);
    assert!(view.input_disabled().is_some());
    assert_eq!(view.active_panel, Panel::TaskList);
}
#[test]
fn rule_10_finished_child_handback_respects_disabled_parent_prompt() {
    for remember_parent in [false, true] {
        let mut view = tasks();
        let parent = view.selected_id().unwrap().clone();
        view.rows[view.selected].process = Some(crate::model::Process {
            pid: 1,
            label: None,
            started_at: None,
        });
        let mut child = session("child", "Running child", SessionStatus::Working, 0);
        child.parent = Some(parent.clone());
        let mut rows = view.rows.clone();
        rows.push(child);
        view.set_sessions(rows, false);
        if remember_parent {
            view.set_active_panel(Panel::Prompt);
            view.set_active_panel(Panel::TaskList);
        }
        view.select(
            view.rows
                .iter()
                .position(|row| row.id == session_key("child"))
                .unwrap(),
        );
        view.set_active_panel(Panel::EventDetail);
        let mut rows = view.rows.clone();
        rows.iter_mut()
            .find(|row| row.id == session_key("child"))
            .unwrap()
            .status = SessionStatus::Inactive;
        view.set_sessions(rows, false);
        assert_eq!(view.selected_id(), Some(&parent));
        assert_eq!(
            view.active_panel,
            Panel::TaskList,
            "remember_parent={remember_parent}"
        );
    }
}
#[test]
fn rule_11_focus_has_only_task_destinations() {
    let mut view = tasks();
    view.set_active_panel(Panel::EventDetail);
    view.set_active_panel(Panel::TaskList);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert_eq!(remembered(&view), Some(Focus::EventDetail));
    let first = view.selected;
    let other = *view
        .selectable_indices()
        .iter()
        .find(|&&i| i != first)
        .unwrap();
    view.select(other);
    view.select(first);
    assert_eq!(view.active_panel, Panel::TaskList);
    assert_eq!(remembered(&view), Some(Focus::EventDetail));
}

#[test]
fn confirmed_list_ctrl_l_does_nothing() {
    let mut view = pending();
    let id = view.selected_id().cloned();
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(view.selected_id(), id.as_ref());
    assert_eq!(view.active_panel, Panel::TaskList);
    assert!(view.task_entry_pending);
}

#[test]
fn rule_2_disabled_pending_prompt_open_keys_stay_on_list() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let mut view = tasks();
        let first = view.selected;
        let other = *view
            .selectable_indices()
            .iter()
            .find(|&&i| i != first)
            .unwrap();
        view.prepare_prompt();
        view.set_active_panel(Panel::TaskList);
        view.select(other);
        view.rows[first].process = Some(crate::model::Process {
            pid: 1,
            label: None,
            started_at: None,
        });
        view.select(first);
        assert!(view.task_entry_pending);
        assert_eq!(remembered(&view), Some(Focus::Prompt));
        assert!(view.input_disabled().is_some());
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.active_panel, Panel::TaskList);
    }
}

#[test]
fn rule_2_disabled_pending_prompt_click_stays_on_list() {
    let mut view = tasks();
    let first = view.selected;
    let other = *view
        .selectable_indices()
        .iter()
        .find(|&&i| i != first)
        .unwrap();
    view.prepare_prompt();
    view.set_active_panel(Panel::TaskList);
    view.select(other);
    view.rows[first].process = Some(crate::model::Process {
        pid: 1,
        label: None,
        started_at: None,
    });
    view.select(first);
    assert!(view.task_entry_pending);
    assert!(view.input_disabled().is_some());
    click_selected(&mut view);
    assert_eq!(view.active_panel, Panel::TaskList);
}

#[test]
fn rule_9_inactive_focus_clear_survives_leaving_task() {
    let mut view = tasks_with_working_child();
    let id = view.selected_id().unwrap().clone();
    view.set_active_panel(Panel::EventDetail);
    view.set_active_panel(Panel::TaskList);
    view.set_active_panel(Panel::EventDetail);
    assert_eq!(remembered(&view), Some(Focus::EventDetail));
    let mut rows = view.rows.clone();
    rows.iter_mut().find(|row| row.id == id).unwrap().status = SessionStatus::Inactive;
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(view.active_panel, Panel::EventDetail);
    assert_eq!(remembered(&view), None);
    view.set_active_panel(Panel::TaskList);
    assert_eq!(remembered(&view), None);
    let other = view
        .rows
        .iter()
        .position(|row| row.parent.is_none() && row.id != id)
        .unwrap();
    view.select(other);
    view.select(view.rows.iter().position(|row| row.id == id).unwrap());
    assert_eq!(view.selected_id(), Some(&id));
    assert_eq!(remembered(&view), None);
    assert!(!view.task_entry_pending);
}

#[test]
fn rule_8_search_exit_updates_focus_without_leaving_panel() {
    for (panel, focus) in [
        (Panel::EventList, Focus::EventList),
        (Panel::EventDetail, Focus::EventDetail),
    ] {
        let (mut view, _) = event_fixture();
        view.set_active_panel(panel);
        assert_eq!(key(&mut view, KeyCode::Char('/')), None);
        assert_eq!(key(&mut view, KeyCode::Esc), None);
        assert_eq!(view.active_panel, panel);
        assert_eq!(remembered(&view), Some(focus));
    }
}

#[test]
fn rule_8_title_exit_updates_focus_without_leaving_panel() {
    let mut view = tasks();
    view.set_active_panel(Panel::TaskDetails);
    view.set_active_panel(Panel::TaskList);
    assert_eq!(key(&mut view, KeyCode::Char('r')), None);
    assert_eq!(remembered(&view), Some(Focus::Title));
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert_eq!(view.active_panel, Panel::TaskDetails);
    assert_eq!(remembered(&view), Some(Focus::TaskDetails));
}

#[test]
fn rule_8_draft_branch_editor_updates_same_panel_focus() {
    use super::super::draft_controls::Field;
    let directory = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    let mut view = tasks();
    view.add_draft(DraftSetup {
        provider: ProviderId::Claude,
        cwd: directory.path().into(),
        model: None,
        effort: None,
        branch: "main".into(),
    });
    view.rename.set_text("Branch focus");
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    view.close_selector();
    view.set_active_panel(Panel::TaskDetails);
    view.open_field(Field::Branch);
    assert!(view.selector.is_some());
    assert_eq!(remembered(&view), Some(Focus::Selector));
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert!(view.selector.as_ref().unwrap().input_active());
    assert_eq!(remembered(&view), Some(Focus::Branch));
    assert_eq!(key(&mut view, KeyCode::Esc), None);
    assert!(!view.selector.as_ref().unwrap().input_active());
    assert_eq!(remembered(&view), Some(Focus::Selector));
}

#[test]
fn confirmed_list_ctrl_l_show_more_and_subagent_do_nothing() {
    let mut view = fixture();
    view.status_filter = 1;
    view.has_more = true;
    view.selected = usize::MAX;
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        None
    );
    assert!(!view.loading);
    assert_eq!(view.selected, usize::MAX);
    let mut view = tasks();
    let mut rows = view.rows.clone();
    let mut child = rows[0].clone();
    child.id = session_key("child");
    child.parent = Some(rows[0].id.clone());
    rows.push(child);
    view.set_sessions(rows, false);
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == session_key("child"))
            .unwrap(),
    );
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
        None
    );
    assert_eq!(view.active_panel, Panel::TaskList);
}

fn question_session() -> Session {
    let mut row = session("question", "Question task", SessionStatus::NeedsInput, 0);
    row.background_job = Some(crate::model::BackgroundJob {
        id: "focus-question".into(),
        needs: None,
        awaits_prompt: false,
    });
    row.process = Some(crate::model::Process {
        pid: 1,
        label: None,
        started_at: None,
    });
    row.transcript = Some("question.jsonl".into());
    let record = include_str!("../../tests/fixtures/claude-questions/transcript.jsonl")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
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
    row
}

#[test]
fn rule_1_list_browsing_question_does_not_activate_or_remember_it() {
    let mut view = test_view(Grouping::Project, false);
    let original = session("original", "Original task", SessionStatus::Ready, 10);
    let question = question_session();
    view.set_sessions(vec![original.clone(), question.clone()], false);
    view.set_active_panel(Panel::TaskList);
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == original.id)
            .unwrap(),
    );
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == question.id)
            .unwrap(),
    );
    assert!(view.active_question().is_some());
    assert_eq!(view.active_panel, Panel::TaskList);
    assert_eq!(remembered(&view), None);
    assert!(!view.task_entry_pending);
}

#[test]
fn rule_2_pending_question_restores_despite_disabled_prompt() {
    for code in [KeyCode::Enter, KeyCode::Char('o')] {
        let mut view = test_view(Grouping::Project, false);
        let question = question_session();
        let other = session("other", "Other task", SessionStatus::Ready, 10);
        view.set_sessions(vec![question.clone(), other.clone()], false);
        view.select(
            view.rows
                .iter()
                .position(|row| row.id == question.id)
                .unwrap(),
        );
        view.set_active_panel(Panel::Prompt);
        view.set_active_panel(Panel::TaskList);
        view.select(view.rows.iter().position(|row| row.id == other.id).unwrap());
        view.select(
            view.rows
                .iter()
                .position(|row| row.id == question.id)
                .unwrap(),
        );
        assert!(view.input_disabled().is_some());
        assert!(view.active_question().is_some());
        assert!(view.task_entry_pending);
        assert_eq!(remembered(&view), Some(Focus::Question));
        assert_eq!(key(&mut view, code), None);
        assert_eq!(view.active_panel, Panel::Prompt);
        assert_eq!(view.task_focus(), Focus::Question);
    }
}

#[test]
fn confirmed_new_question_on_current_task_activates_question() {
    let mut view = test_view(Grouping::Project, false);
    let question = question_session();
    let mut working = question.clone();
    working.status = SessionStatus::Working;
    working.ask = None;
    view.set_sessions(vec![working], false);
    view.set_active_panel(Panel::EventDetail);
    view.set_sessions(vec![question], false);
    assert!(view.active_question().is_some());
    assert_eq!(view.active_panel, Panel::Prompt);
    assert_eq!(remembered(&view), Some(Focus::Question));
}

#[test]
fn rule_10_attention_jump_activates_answerable_question() {
    let mut view = tasks();
    let original = view.selected_id().unwrap().clone();
    let question = question_session();
    let mut working = question.clone();
    working.status = SessionStatus::Working;
    working.ask = None;
    let mut rows = view.rows.clone();
    rows.push(working);
    view.set_sessions(rows, false);
    view.select(view.rows.iter().position(|row| row.id == original).unwrap());
    view.set_active_panel(Panel::EventDetail);
    let mut rows = view.rows.clone();
    *rows.iter_mut().find(|row| row.id == question.id).unwrap() = question.clone();
    view.set_sessions(rows, false);
    assert_eq!(view.selected_id(), Some(&question.id));
    assert!(view.active_question().is_some());
    assert!(view.input_disabled().is_some());
    assert_eq!(view.active_panel, Panel::Prompt);
    assert_eq!(remembered(&view), Some(Focus::Question));
}

fn tasks_with_working_child() -> View {
    let mut view = tasks();
    let mut child = session("active-child", "Active child", SessionStatus::Working, 0);
    child.parent = view.selected_id().cloned();
    let mut rows = view.rows.clone();
    rows.push(child);
    view.set_sessions(rows, false);
    view
}

#[test]
fn confirmed_completed_question_restores_available_prompt() {
    let mut view = test_view(Grouping::Project, false);
    let mut question = question_session();
    question.process = None;
    let ask = question.ask.clone().unwrap();
    let other = session("other", "Other task", SessionStatus::Ready, 10);
    view.set_sessions(vec![question.clone(), other.clone()], false);
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == question.id)
            .unwrap(),
    );
    view.set_active_panel(Panel::Prompt);
    view.set_active_panel(Panel::TaskList);
    view.select(view.rows.iter().position(|row| row.id == other.id).unwrap());
    view.questions.complete(&ask, &Ok(()));
    view.select(
        view.rows
            .iter()
            .position(|row| row.id == question.id)
            .unwrap(),
    );
    assert!(view.active_question().is_none());
    assert!(view.input_disabled().is_none());
    assert_eq!(remembered(&view), Some(Focus::Question));
    assert!(view.task_entry_pending);
    assert_eq!(key(&mut view, KeyCode::Enter), None);
    assert_eq!(view.active_panel, Panel::Prompt);
    assert_eq!(remembered(&view), Some(Focus::Prompt));
}

#[test]
fn rule_8_question_completion_updates_current_input_focus_only() {
    for panel in [Panel::Prompt, Panel::EventDetail] {
        for current in [true, false] {
            let mut view = test_view(Grouping::Project, false);
            let question = question_session();
            let other = session("other", "Other task", SessionStatus::Ready, 10);
            view.set_sessions(vec![question.clone(), other.clone()], false);
            view.select(
                view.rows
                    .iter()
                    .position(|row| row.id == question.id)
                    .unwrap(),
            );
            view.set_active_panel(panel);
            if !current {
                view.select(view.rows.iter().position(|row| row.id == other.id).unwrap());
            }
            let before_panel = view.active_panel;
            let before_focus = remembered(&view);
            let input = crate::providers::SessionInput::Answer {
                ask: question.ask.clone().unwrap(),
                answer: Default::default(),
                transcript: question.transcript.clone().unwrap(),
            };
            view.input_sending(&question.id, &input);
            assert_eq!(remembered(&view), before_focus);
            view.input_complete(&question.id, &input, Ok(()));
            assert_eq!(view.active_panel, before_panel);
            assert_eq!(
                remembered(&view),
                if current && panel == Panel::Prompt {
                    Some(Focus::Prompt)
                } else {
                    before_focus
                }
            );
        }
    }
}

#[test]
fn rule_8_question_disappearance_updates_current_input_focus() {
    let mut view = test_view(Grouping::Project, false);
    let question = question_session();
    view.set_sessions(vec![question.clone()], false);
    view.set_active_panel(Panel::Prompt);
    assert_eq!(remembered(&view), Some(Focus::Question));
    let mut working = question;
    working.status = SessionStatus::Working;
    working.ask = None;
    view.set_sessions(vec![working], false);
    assert_eq!(view.active_panel, Panel::Prompt);
    assert!(view.active_question().is_none());
    assert_eq!(remembered(&view), Some(Focus::Prompt));
}

#[test]
fn rule_9_inactive_question_removal_keeps_focus_cleared() {
    let mut view = test_view(Grouping::Project, false);
    let question = question_session();
    let mut child = session("active-child", "Active child", SessionStatus::Working, 0);
    child.parent = Some(question.id.clone());
    view.set_sessions(vec![question.clone(), child.clone()], false);
    view.set_active_panel(Panel::Prompt);
    assert_eq!(remembered(&view), Some(Focus::Question));
    let mut inactive = question;
    inactive.status = SessionStatus::Inactive;
    inactive.ask = None;
    view.set_sessions(vec![inactive.clone(), child], false);
    assert_eq!(view.selected_id(), Some(&inactive.id));
    assert_eq!(view.active_panel, Panel::Prompt);
    assert!(view.active_question().is_none());
    assert_eq!(remembered(&view), None);
}
