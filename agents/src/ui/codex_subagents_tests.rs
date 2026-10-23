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
        view.set_pinned(&crate::state::TaskPins {
            active: vec![session_key("root")],
            inactive: vec![],
        });
        assert!(!view.is_pinned(deep));
        assert!(view.in_pinned_group(deep));
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

#[path = "codex_subagent_task_details_tests.rs"]
mod task_details;

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
            assert!(row.ends_with("2m ago "), "width={width}: {row}");
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
    let title = crate::event::agent::name(
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

#[test]
fn inactive_merged_tree_keeps_cross_provider_ancestors_and_flat_sessions() {
    let reference: chrono::DateTime<Utc> = "2026-10-19T12:00:00Z".parse().unwrap();
    let mut parent = session(
        "claude-parent",
        "Live Claude parent",
        SessionStatus::Working,
        0,
    );
    parent.updated_at = reference - Duration::minutes(2);
    let mut child = session(
        "managed-child",
        "Finished managed child",
        SessionStatus::Inactive,
        0,
    );
    child.id.provider = ProviderId::Codex;
    child.parent = Some(parent.id.clone());
    child.updated_at = reference - Duration::minutes(1);
    let mut grandchild = session(
        "native-child",
        "Finished native child",
        SessionStatus::Inactive,
        0,
    );
    grandchild.id.provider = ProviderId::Codex;
    grandchild.parent = Some(child.id.clone());
    grandchild.updated_at = reference - Duration::minutes(3);
    // The merged snapshot clears a missing parent before handing rows to the view.
    let mut flat = session(
        "flat-history",
        "Unattached history",
        SessionStatus::Inactive,
        0,
    );
    flat.id.provider = ProviderId::Codex;
    flat.updated_at = reference - Duration::minutes(10);

    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![flat, grandchild, child, parent], false);
    view.status_filter = tab_for_status(SessionStatus::Inactive);
    assert_eq!(
        ids(&view),
        [
            "claude-parent",
            "managed-child",
            "native-child",
            "flat-history"
        ]
    );

    let area = Rect::new(0, 0, 160, 12);
    let mut buffer = Buffer::empty(area);
    view.render_center_rows(area, &mut buffer, reference);
    let lines: Vec<_> = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    for (title, status, age, prefix) in [
        ("Live Claude parent", "Working", "2m ago", ""),
        ("Finished managed child", "Inactive", "1m ago", "└─ "),
        ("Finished native child", "Inactive", "3m ago", "   └─ "),
        ("Unattached history", "Inactive", "10m ago", ""),
    ] {
        let line = lines.iter().find(|line| line.contains(title)).unwrap();
        assert!(line.starts_with(prefix), "{line}");
        assert!(line.contains(status), "{line}");
        assert!(line.ends_with(age), "{line}");
        if prefix.is_empty() {
            assert!(!line.contains('└') && !line.contains('├'), "{line}");
        }
    }
    assert_eq!(
        view.group_index(
            view.rows
                .iter()
                .position(|row| row.id.id == "flat-history")
                .unwrap()
        ),
        view.rows.iter().position(|row| row.id.id == "flat-history")
    );
}

#[test]
fn child_pin_toggles_only_selected_task_and_checks_root_and_own_status() {
    let mut view = tree();
    let root = view.rows.iter().position(|s| s.id.id == "root").unwrap();
    let child = view
        .rows
        .iter()
        .position(|s| s.id.id == "grandchild")
        .unwrap();
    view.rows[root].status = SessionStatus::Inactive;
    view.set_selected(child);
    view.set_active_panel(Panel::TaskList);
    for (pins, pinned) in [
        (vec![], true),
        (vec![session_key("root")], true),
        (vec![session_key("root"), session_key("grandchild")], false),
    ] {
        view.set_pinned(&crate::state::TaskPins {
            active: pins.clone(),
            inactive: vec![],
        });
        view.set_selected(child);
        view.set_active_panel(Panel::TaskList);
        assert_eq!(view.selected_id(), Some(&session_key("grandchild")));
        assert_eq!(view.rows[view.selected].status, SessionStatus::Working);
        assert_eq!(
            key(&mut view, KeyCode::Char('p')),
            Some(Action::TogglePin {
                tab: 0,
                id: session_key("grandchild"),
                pinned,
            })
        );
    }
    view.rows[child].status = SessionStatus::Inactive;
    for (root_status, expected) in [
        (
            SessionStatus::Ready,
            Action::TogglePin {
                tab: 0,
                id: session_key("grandchild"),
                pinned: true,
            },
        ),
        (SessionStatus::Inactive, Action::Unsupported("Pin")),
    ] {
        view.rows[root].status = root_status;
        // The active immediate parent must not replace the actual root check.
        view.rows
            .iter_mut()
            .find(|s| s.id.id == "child")
            .unwrap()
            .status = SessionStatus::Ready;
        view.set_pinned(&crate::state::TaskPins {
            active: vec![],
            inactive: vec![],
        });
        view.set_selected(child);
        view.set_active_panel(Panel::TaskList);
        assert_eq!(key(&mut view, KeyCode::Char('p')), Some(expected));
    }
}

#[test]
fn child_pin_subtrees_keep_pin_order_and_grouping() {
    let mut view = tree();
    view.rows
        .iter_mut()
        .find(|s| s.id.id == "child")
        .unwrap()
        .status = SessionStatus::Ready;
    for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
        view.grouping = grouping;
        for (pins, expected) in [
            (
                vec!["grandchild"],
                vec!["grandchild", "deep", "root", "child"],
            ),
            (
                vec!["grandchild", "root"],
                vec!["grandchild", "deep", "root", "child"],
            ),
            (
                vec!["root", "grandchild"],
                vec!["root", "child", "grandchild", "deep"],
            ),
            (vec!["deep"], vec!["deep", "root", "child", "grandchild"]),
        ] {
            view.set_pinned(&crate::state::TaskPins {
                active: pins.iter().map(|id| session_key(id)).collect(),
                inactive: vec![],
            });
            assert_eq!(ids(&view), expected);
            let indices = view.visible_indices();
            let entries = view.center_rows(&indices, grouping);
            assert!(
                !entries
                    .iter()
                    .any(|row| matches!(row, super::super::navigation::CenterRow::Parent(_)))
            );
            let headings: Vec<_> = entries
                .iter()
                .filter_map(|row| {
                    if let super::super::navigation::CenterRow::Group(index) = row {
                        Some(view.group_heading(*index, 80))
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(headings[0], "Pinned");
            assert_eq!(headings.len(), if pins.contains(&"root") { 1 } else { 2 });
        }
    }
}

#[test]
fn child_pin_stays_visible_under_inactive_parent_in_active_tab() {
    let mut view = tree();
    view.rows
        .iter_mut()
        .find(|s| s.id.id == "root")
        .unwrap()
        .status = SessionStatus::Inactive;
    view.set_pinned(&crate::state::TaskPins {
        active: vec![session_key("grandchild")],
        inactive: vec![],
    });
    assert_eq!(ids(&view), ["grandchild", "deep"]);
    let rendered = screen(&view, 160, 35);
    assert!(rendered.contains("Pinned"));
    assert!(rendered.contains("Provider adapters"));
    assert!(!rendered.contains("Root task"));
}

#[test]
fn inactive_child_pin_stays_visible_with_active_root_in_active_tab() {
    let mut view = tree();
    for row in &mut view.rows {
        if row.id.id != "root" {
            row.status = SessionStatus::Inactive;
        }
    }
    view.set_pinned(&crate::state::TaskPins {
        active: vec![session_key("grandchild")],
        inactive: vec![],
    });
    assert_eq!(ids(&view), ["grandchild", "root"]);
    let rendered = screen(&view, 160, 35);
    assert!(rendered.contains("Pinned"));
    assert!(rendered.contains("Provider adapters"));
    view.rows
        .iter_mut()
        .find(|s| s.id.id == "deep")
        .unwrap()
        .status = SessionStatus::Working;
    assert_eq!(ids(&view), ["grandchild", "deep", "root"]);
    let rendered = screen(&view, 160, 35);
    assert!(rendered.contains("Deep worker"));
}

#[test]
fn active_pins_do_not_change_inactive_rows_or_groups() {
    let mut view = tree();
    view.set_pinned(&crate::state::TaskPins {
        active: vec![session_key("child")],
        inactive: vec![],
    });
    view.status_filter = 1;
    assert_eq!(ids(&view), ["root", "child"]);
    let child = view
        .rows
        .iter()
        .position(|row| row.id.id == "child")
        .unwrap();
    assert!(!view.is_pinned(child));
    assert!(!view.in_pinned_group(child));
    assert!(!screen(&view, 160, 35).contains("Pinned"));
}

#[test]
fn inactive_tab_allows_pinning_with_an_inactive_root() {
    let mut view = tree();
    for row in &mut view.rows {
        row.status = SessionStatus::Inactive;
    }
    view.status_filter = 1;
    let child = view
        .rows
        .iter()
        .position(|row| row.id.id == "grandchild")
        .unwrap();
    view.set_selected(child);
    view.set_active_panel(Panel::TaskList);
    assert!(matches!(key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin { id, tab: 1, pinned: true }) if id == session_key("grandchild")));
}

#[test]
fn each_tab_uses_its_own_pin_order_and_subtrees_even_when_not_selected() {
    let mut view = tree();
    let pins = crate::state::TaskPins {
        active: vec![session_key("root"), session_key("grandchild")],
        inactive: vec![session_key("grandchild"), session_key("root")],
    };
    view.set_pinned(&pins);
    for grouping in [Grouping::Project, Grouping::Status, Grouping::Model] {
        view.grouping = grouping;
        for selected_tab in [0, 1] {
            view.status_filter = selected_tab;
            for tab in [0, 1] {
                for row in &mut view.rows {
                    row.status = if tab == 0 {
                        SessionStatus::Ready
                    } else {
                        SessionStatus::Inactive
                    };
                }
                let ids: Vec<_> = view
                    .visible_indices_for_filter(tab)
                    .into_iter()
                    .map(|index| view.rows[index].id.id.as_str())
                    .collect();
                assert_eq!(
                    ids,
                    if tab == 0 {
                        vec!["root", "child", "grandchild", "deep"]
                    } else {
                        vec!["grandchild", "deep", "root", "child"]
                    }
                );
            }
        }
    }
    view.set_pinned(&crate::state::TaskPins {
        active: vec![],
        inactive: vec![session_key("grandchild")],
    });
    view.status_filter = 0;
    let child = view
        .rows
        .iter()
        .position(|row| row.id.id == "grandchild")
        .unwrap();
    let deep = view
        .rows
        .iter()
        .position(|row| row.id.id == "deep")
        .unwrap();
    assert!(!view.is_pinned(child));
    assert!(!view.in_pinned_group(deep));
    view.status_filter = 1;
    assert!(view.is_pinned(child));
    assert!(view.in_pinned_group(deep));
    view.set_selected(child);
    view.set_active_panel(Panel::TaskList);
    assert_eq!(
        key(&mut view, KeyCode::Char('p')),
        Some(Action::TogglePin {
            id: session_key("grandchild"),
            tab: 1,
            pinned: false,
        })
    );
}
