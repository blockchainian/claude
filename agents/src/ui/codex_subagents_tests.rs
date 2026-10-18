use super::*;

fn tree() -> View {
    let mut root = session("root", "Root task", SessionStatus::Ready, 100);
    root.model = Some("root-model".into());
    let mut child = session("child", "Codex task", SessionStatus::Inactive, 50);
    child.parent = Some(root.id.clone());
    child.started_at = Some(Utc::now());
    let mut grandchild = session(
        "grandchild",
        "Provider adapters",
        SessionStatus::Working,
        10,
    );
    grandchild.parent = Some(child.id.clone());
    grandchild.started_at = Some(Utc::now() - Duration::minutes(2));
    let mut deep = session("deep", "Deep worker", SessionStatus::Working, 0);
    deep.parent = Some(grandchild.id.clone());
    deep.started_at = Some(Utc::now() - Duration::minutes(1));
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![root, child, grandchild, deep], false);
    view
}

fn ids(view: &View) -> Vec<&str> {
    view.visible_indices()
        .iter()
        .map(|&i| view.rows[i].id.id.as_str())
        .collect()
}

#[test]
fn nested_tree_follows_root_filter_search_group_and_pin() {
    let mut view = tree();
    for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
        view.grouping = grouping;
        assert_eq!(ids(&view), ["root", "child", "grandchild", "deep"]);
        let deep = view.rows.iter().position(|s| s.id.id == "deep").unwrap();
        assert_eq!(view.rows[view.group_index(deep).unwrap()].id.id, "root");
        view.set_pinned(&[session_key("root")]);
        assert!(view.is_pinned(deep));
    }
    view.search.set_text("Root task");
    assert_eq!(ids(&view), ["root", "child", "grandchild", "deep"]);
    view.search.set_text("Provider adapters");
    assert!(ids(&view).is_empty());
    view.search.clear();
    view.status_filter = 1;
    assert!(ids(&view).is_empty());
}

#[test]
fn nested_background_includes_working_descendants_of_every_session() {
    let mut view = tree();
    let child = view.rows.iter().position(|s| s.id.id == "child").unwrap();
    view.rows[child].status = SessionStatus::Working;
    for (id, expected) in [
        ("root", vec!["grandchild", "deep", "child"]),
        ("child", vec!["grandchild", "deep"]),
        ("grandchild", vec!["deep"]),
    ] {
        let session = view.rows.iter().find(|s| s.id.id == id).unwrap();
        assert_eq!(
            view.subagents_for(session)
                .map(|s| s.id.id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn background_descendants_put_unknown_starts_last_then_use_id() {
    let mut view = tree();
    for child in &mut view.rows {
        if child.id.id != "root" {
            child.status = SessionStatus::Working;
            child.started_at = (child.id.id == "deep").then_some(Utc::now());
        }
    }
    let root = view.rows.iter().find(|s| s.id.id == "root").unwrap();
    assert_eq!(
        view.subagents_for(root)
            .map(|s| s.id.id.as_str())
            .collect::<Vec<_>>(),
        ["deep", "child", "grandchild"]
    );
}

#[test]
fn nested_rows_render_increasing_indent() {
    let view = tree();
    let area = Rect::new(0, 0, 160, 35);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    let list = view.center_layout(area).list;
    let positions: Vec<_> = [
        "Root task",
        "Codex task",
        "Provider adapters",
        "Deep worker",
    ]
    .iter()
    .map(|name| {
        (list.y..list.bottom())
            .find_map(|y| {
                let line = (list.x..list.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>();
                line.find(name).map(|offset| line[..offset].chars().count())
            })
            .unwrap()
    })
    .collect();
    assert_eq!(
        positions
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect::<Vec<_>>(),
        [3, 3, 3]
    );
}

#[path = "codex_subagent_details_tests.rs"]
mod details;

#[test]
fn nested_rows_align_status_and_time_columns() {
    let mut root = session("root", "Root task", SessionStatus::Working, 120);
    let mut child = session("child", "Child task", SessionStatus::Ready, 120);
    let mut grandchild = session("grandchild", "Deep task", SessionStatus::Ready, 120);
    child.parent = Some(root.id.clone());
    grandchild.parent = Some(child.id.clone());
    let reference = Utc::now();
    root.updated_at = reference - Duration::minutes(2);
    child.updated_at = root.updated_at;
    grandchild.updated_at = root.updated_at;
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![root, child, grandchild], false);
    for width in [46, 48, 50, 80] {
        let area = Rect::new(3, 2, width, 8);
        let mut buf = Buffer::empty(area);
        view.render_center_rows(area, &mut buf, reference);
        let rows: Vec<_> = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let root = rows.iter().find(|row| row.contains("Root task")).unwrap();
        let deep = rows.iter().find(|row| row.contains("Deep task")).unwrap();
        assert_eq!(
            root.find("Working").map(|offset| root[..offset].width()),
            deep.find("Ready").map(|offset| deep[..offset].width()),
            "width={width}:\n{root}\n{deep}"
        );
        for row in [root, deep] {
            assert!(row.ends_with("2m ago"), "width={width}: {row}");
        }
    }
}
