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
    let list = view.center_layout(Rect::new(0, 0, 120, 30)).list;
    let rendered = subagent_rows::screen(view, 120, 30)
        .lines()
        .map(|line| line.chars().take(usize::from(list.right())).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let label = ["●", "✚"]
        .map(|symbol| format!("{symbol} {title}"))
        .into_iter()
        .find(|label| rendered.contains(label))
        .expect("task status indicator must be rendered");
    let (x, y) = rendered_cell(&rendered, &label);
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

fn mixed_status_tree() -> View {
    let root = session("root", "Root task", SessionStatus::Inactive, 30);
    let mut middle = session("middle", "Middle task", SessionStatus::Working, 20);
    middle.parent = Some(root.id.clone());
    let mut leaf = session("leaf", "Leaf task", SessionStatus::Inactive, 10);
    leaf.parent = Some(middle.id.clone());
    let mut grandchild = session("grandchild", "Grandchild task", SessionStatus::Working, 0);
    grandchild.parent = Some(leaf.id.clone());
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![root, middle, leaf, grandchild], false);
    view
}

#[test]
fn folding_is_independent_between_status_tabs() {
    let mut view = mixed_status_tree();
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
fn folded_indicator_preserves_style_and_only_changes_in_current_tab() {
    let mut view = mixed_status_tree();
    view.use_colors = true;
    view.status_filter = 1;
    view.reconcile_selection();
    let rendered = subagent_rows::screen(&view, 120, 30);
    let (x, y) = rendered_cell(&rendered, "● Middle task");
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    view.render_at(buf.area, &mut buf, "2026-10-13T12:00:00Z".parse().unwrap());
    let dot_style = buf[(x, y)].style();

    click_dot(&mut view, "Middle task");
    let rendered = subagent_rows::screen(&view, 120, 30);
    let (x, y) = rendered_cell(&rendered, "✚ Middle task");
    view.render_at(buf.area, &mut buf, "2026-10-13T12:00:00Z".parse().unwrap());
    assert_eq!(buf[(x, y)].style(), dot_style);

    view.status_filter = 0;
    view.reconcile_selection();
    let rendered = subagent_rows::screen(&view, 120, 30);
    rendered_cell(&rendered, "● Middle task");
    assert!(visible_titles(&view).contains(&"Leaf task".to_owned()));

    view.status_filter = 1;
    view.reconcile_selection();
    let rendered = subagent_rows::screen(&view, 120, 30);
    rendered_cell(&rendered, "✚ Middle task");
    click_dot(&mut view, "Middle task");
    let rendered = subagent_rows::screen(&view, 120, 30);
    rendered_cell(&rendered, "● Middle task");
    assert!(visible_titles(&view).contains(&"Leaf task".to_owned()));
}

#[test]
fn title_double_click_and_childless_dot_click_do_not_fold() {
    let mut view = tree();
    let all = ["Root task", "Middle task", "Leaf task", "Grandchild task"];
    double_click_title(&mut view, "Middle task");
    click_dot(&mut view, "Grandchild task");
    assert_eq!(visible_titles(&view), all);
}
