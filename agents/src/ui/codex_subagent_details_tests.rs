use super::*;

#[test]
fn nested_selection_is_remembered_in_its_root_tab() {
    let mut view = tree();
    let child = view
        .rows
        .iter()
        .position(|row| row.id.id == "child")
        .unwrap();
    view.select(child);
    for _ in 0..2 {
        view.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    assert_eq!(view.selected_id(), Some(&session_key("child")));
}

#[test]
fn nested_tabs_count_rows_using_root_status() {
    let view = tree();
    let rendered = screen(&view, 160, 35);
    assert!(rendered.contains("Active 4"), "{rendered}");
    assert!(rendered.contains("Inactive 0"), "{rendered}");
}

#[test]
fn codex_subagent_details_omit_unreadable_prompt() {
    let mut view = tree();
    let index = view
        .rows
        .iter()
        .position(|row| row.id.id == "deep")
        .unwrap();
    view.rows[index].id.provider = ProviderId::Codex;
    view.rows[index].parent.as_mut().unwrap().provider = ProviderId::Codex;
    let parent = view
        .rows
        .iter()
        .position(|row| row.id.id == "grandchild")
        .unwrap();
    view.rows[parent].id.provider = ProviderId::Codex;
    view.rows[index].prompt.clear();
    view.select(index);
    let rendered = screen(&view, 160, 35);
    assert!(!rendered.contains("No prompt available."), "{rendered}");
    assert!(!rendered.contains("Prompt"), "{rendered}");
}
