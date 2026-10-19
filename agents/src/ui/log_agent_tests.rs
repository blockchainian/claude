use super::*;

fn collaboration_view() -> (tempfile::TempDir, View) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parent.jsonl");
    std::fs::write(
        &path,
        include_str!("../../tests/fixtures/codex-interaction/parent.jsonl"),
    )
    .unwrap();
    let mut parent = session("parent", "Parent", SessionStatus::Ready, 0);
    parent.id.provider = ProviderId::Codex;
    parent.transcript = Some(path);
    let events = crate::providers::log_reader(&parent)
        .unwrap()
        .refresh(100)
        .unwrap()
        .events;
    let mut child = session(
        "01a14ff0-1c14-74c0-8f40-4d7f85ae6ecc",
        "demo_sub",
        SessionStatus::Inactive,
        0,
    );
    child.id.provider = ProviderId::Codex;
    child.parent = Some(parent.id.clone());
    let parent_id = parent.id.clone();
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![parent, child], false);
    let index = view
        .rows
        .iter()
        .position(|row| row.id == parent_id)
        .unwrap();
    view.select(index);
    view.set_log(events);
    (dir, view)
}

#[test]
fn log_enter_selects_spawned_child_by_thread_id() {
    let (_dir, mut view) = collaboration_view();
    let event = view
        .log
        .iter()
        .find(|event| event.calls.iter().any(|call| call.name == "spawn_agent"))
        .unwrap()
        .clone();
    view.select_fixture_event(Some(event));
    view.mouse_focus = Panel::LogList;
    view.log_list_focused = true;
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        None
    );
    assert_eq!(view.status_filter, 1);
    assert_eq!(
        view.selected_row().unwrap().id.id,
        "01a14ff0-1c14-74c0-8f40-4d7f85ae6ecc"
    );
}

#[test]
fn log_enter_selects_incoming_child_by_path() {
    let (_dir, mut view) = collaboration_view();
    let event = view
        .log
        .iter()
        .find(|event| event.calls.iter().any(|call| call.name == "FINAL_ANSWER"))
        .unwrap()
        .clone();
    view.select_fixture_event(Some(event));
    view.mouse_focus = Panel::LogList;
    view.log_list_focused = true;
    assert_eq!(
        view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        None
    );
    assert_eq!(view.status_filter, 1);
    assert_eq!(
        view.selected_row().unwrap().id.id,
        "01a14ff0-1c14-74c0-8f40-4d7f85ae6ecc"
    );
}

#[test]
fn log_agent_jump_requires_list_focus() {
    for focus in [Panel::TaskList, Panel::LogDetail, Panel::Prompt] {
        let (_dir, mut view) = collaboration_view();
        let event = view
            .log
            .iter()
            .find(|event| event.calls.iter().any(|call| call.name == "spawn_agent"))
            .unwrap()
            .clone();
        view.select_fixture_event(Some(event));
        view.mouse_focus = focus;
        view.log_list_focused = focus == Panel::LogList;
        view.jump_log_agent();
        assert_eq!(view.selected_row().unwrap().id.id, "parent");
    }
}
