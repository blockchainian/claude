use super::*;

fn tree() -> View {
    let root = session("root", "Root task", SessionStatus::Working, 30);
    let mut middle = session("middle", "Middle task", SessionStatus::Working, 20);
    middle.parent = Some(root.id.clone());
    let mut leaf = session("leaf", "Leaf task", SessionStatus::Working, 10);
    leaf.parent = Some(middle.id.clone());
    let mut grandchild = session("grandchild", "Grandchild task", SessionStatus::Working, 0);
    grandchild.parent = Some(leaf.id.clone());
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![root, middle, leaf, grandchild], false);
    view
}

fn visible_titles(view: &View) -> Vec<String> {
    view.visible_indices()
        .into_iter()
        .map(|index| view.rows[index].title.clone())
        .collect()
}

fn click_dot(view: &mut View, title: &str) {
    let rendered = subagent_rows::screen(view, 120, 30);
    let (x, y) = rendered_cell(&rendered, &format!("● {title}"));
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(click_mouse(view, click).is_none());
}

fn double_click_title(view: &mut View, title: &str) {
    let rendered = subagent_rows::screen(view, 120, 30);
    let (x, y) = rendered_cell(&rendered, title);
    let click = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(click_mouse(view, click).is_none());
    assert!(click_mouse(view, click).is_none());
}

#[test]
fn dot_click_folds_only_that_parent_and_unfolding_keeps_nested_state() {
    let mut view = tree();
    let all = ["Root task", "Middle task", "Leaf task", "Grandchild task"];
    assert_eq!(visible_titles(&view), all);

    click_dot(&mut view, "Leaf task");
    assert_eq!(visible_titles(&view), all[..3]);

    click_dot(&mut view, "Middle task");
    assert_eq!(visible_titles(&view), all[..2]);

    click_dot(&mut view, "Middle task");
    assert_eq!(visible_titles(&view), all[..3]);

    click_dot(&mut view, "Leaf task");
    assert_eq!(visible_titles(&view), all);
}

#[test]
fn folding_is_independent_between_status_tabs() {
    let root = session("root", "Root task", SessionStatus::Inactive, 30);
    let mut middle = session("middle", "Middle task", SessionStatus::Working, 20);
    middle.parent = Some(root.id.clone());
    let mut leaf = session("leaf", "Leaf task", SessionStatus::Inactive, 10);
    leaf.parent = Some(middle.id.clone());
    let mut grandchild = session("grandchild", "Grandchild task", SessionStatus::Working, 0);
    grandchild.parent = Some(leaf.id.clone());
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![root, middle, leaf, grandchild], false);
    let all = ["Root task", "Middle task", "Leaf task", "Grandchild task"];

    view.status_filter = 1;
    view.reconcile_selection();
    assert_eq!(visible_titles(&view), all[..3]);
    click_dot(&mut view, "Middle task");
    assert_eq!(visible_titles(&view), all[..2]);
    assert_eq!(view.visible_indices_for_filter(0).len(), 4);

    view.status_filter = 0;
    view.reconcile_selection();
    assert_eq!(visible_titles(&view), all);

    view.status_filter = 1;
    view.reconcile_selection();
    assert_eq!(visible_titles(&view), all[..2]);
}

#[test]
fn title_double_click_and_childless_dot_click_do_not_fold() {
    let mut view = tree();
    let all = ["Root task", "Middle task", "Leaf task", "Grandchild task"];
    double_click_title(&mut view, "Middle task");
    click_dot(&mut view, "Grandchild task");
    assert_eq!(visible_titles(&view), all);
}
