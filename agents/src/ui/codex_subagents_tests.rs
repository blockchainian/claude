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
fn nested_tree_filters_status_and_follows_root_search_group_and_pin() {
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
    assert_eq!(ids(&view), ["root", "child"]);
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

fn branching_tree() -> View {
    let mut view = tree();
    let child = view.rows.iter_mut().find(|s| s.id.id == "child").unwrap();
    child.status = SessionStatus::Working;
    let mut rows = view.rows.clone();
    for (id, title, parent, age) in [
        ("sibling", "Sibling task", "root", 200),
        ("cousin", "Cousin task", "sibling", 0),
        ("grand-sibling", "Grand sibling", "child", 200),
    ] {
        let mut sibling = session(id, title, SessionStatus::Working, age);
        sibling.parent = Some(session_key(parent));
        rows.push(sibling);
    }
    view.set_sessions(rows, false);
    view
}

fn connector_row(view: &View, title: &str, height: u16) -> Vec<String> {
    let area = Rect::new(0, 0, 80, height);
    let mut buf = Buffer::empty(area);
    view.render_center_rows(area, &mut buf, Utc::now());
    let y = (0..area.height)
        .find(|&y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains(title)
        })
        .unwrap();
    (0..area.width)
        .map(|x| buf[(x, y)].symbol().to_owned())
        .collect()
}

#[test]
fn nested_connectors_continue_through_ancestors_with_visible_siblings() {
    let mut view = branching_tree();
    let row = connector_row(&view, "Provider adapters", 12);
    assert_eq!(row[0], "│");
    let mut snapshot = String::new();
    for (title, prefix) in [
        ("Codex task", "├─ "),
        ("Provider adapters", "│  ├─ "),
        ("Deep worker", "│  │  └─ "),
        ("Grand sibling", "│  └─ "),
        ("Sibling task", "└─ "),
        ("Cousin task", "   └─ "),
    ] {
        let row = connector_row(&view, title, 12);
        let actual = row[..prefix.chars().count()].concat();
        assert_eq!(actual, prefix, "{title}");
        snapshot.push_str(&format!("{actual}{title}\n"));
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/nested_tree_connectors.txt");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());

    // Search and tabs use the same visible tree as the list.
    view.search.set_text("Root task");
    assert_eq!(
        connector_row(&view, "Deep worker", 12)[..9].concat(),
        "│  │  └─ "
    );
    view.search.clear();
    view.rows
        .iter_mut()
        .find(|s| s.id.id == "root")
        .unwrap()
        .status = SessionStatus::Inactive;
    view.status_filter = 0;
    assert_eq!(
        connector_row(&view, "Deep worker", 12)[..9].concat(),
        "│  │  └─ "
    );
    view.hidden.insert(session_key("grand-sibling"));
    view.hidden.insert(session_key("sibling"));
    assert_eq!(
        connector_row(&view, "Deep worker", 12)[..9].concat(),
        "      └─ "
    );
}

#[test]
fn nested_connectors_include_siblings_outside_the_viewport() {
    let mut view = branching_tree();
    view.selected = view.rows.iter().position(|s| s.id.id == "deep").unwrap();
    assert_eq!(
        connector_row(&view, "Deep worker", 3)[..9].concat(),
        "│  │  └─ "
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

#[test]
fn child_box_and_task_list_share_the_first_line_title() {
    let mut view = tree();
    let record: serde_json::Value = serde_json::from_str(
        include_str!("../../tests/fixtures/codex-subagents/rollout.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let title = crate::log::agent::name(
        record["payload"]["source"]["subagent"]["thread_spawn"]["agent_path"]
            .as_str()
            .unwrap(),
    );
    let child = view
        .rows
        .iter_mut()
        .find(|row| row.id.id == "child")
        .unwrap();
    child.title = format!("{title}\nHidden second line");
    child.status = SessionStatus::Working;
    let area = Rect::new(0, 0, 200, 55);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, Utc::now());
    let text = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches(title).count(), 2, "{text}");
    assert!(!text.contains("Hidden second line"));
}
