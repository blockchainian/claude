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

#[test]
fn background_mixes_tasks_and_descendants_by_height_start_and_identity() {
    use std::io::Write;

    let now = Utc::now();
    let directory = tempfile::tempdir().unwrap();
    let mut parent = session("root", "Parent", SessionStatus::Ready, 0);
    // Reverse path order deliberately, with equal starts across both kinds.
    for (path, title, minutes, output) in [
        ("z-output", "Shell later tie", 2, "test output"),
        ("a-output", "Shell earlier tie", 2, "test output"),
        ("cargo", "cargo test", 10, "test output"),
        ("quiet", "General refactor", 15, ""),
    ] {
        let path = directory.path().join(path);
        let mut file = std::fs::File::create(&path).unwrap();
        write!(file, "{output}").unwrap();
        parent.background.push(crate::model::BackgroundTask {
            description: title.into(),
            output: path,
            started_at: now - Duration::minutes(minutes),
        });
    }
    let mut rows = vec![parent];
    for (id, title, minutes, activity, parent) in [
        ("plato", "Plato", Some(12), true, "bern"),
        ("bern", "Bernoulli", Some(4), false, "root"),
        ("z-tie", "Session later tie", Some(2), true, "root"),
        ("a-tie", "Session earlier tie", Some(2), true, "root"),
        ("z-unknown", "Unknown later", None, true, "root"),
        ("a-unknown", "Unknown earlier", None, true, "root"),
        ("quiet-unknown", "Unknown quiet", None, false, "root"),
    ] {
        let mut child = session(id, title, SessionStatus::Working, 0);
        child.parent = Some(session_key(parent));
        child.started_at = minutes.map(|minutes| now - Duration::minutes(minutes));
        child.last_message = activity.then(|| "Working".into());
        rows.push(child);
    }
    let expected = [
        "Plato",
        "cargo test",
        "Shell earlier tie",
        "Shell later tie",
        "Session earlier tie",
        "Session later tie",
        "Unknown earlier",
        "Unknown later",
        "General refactor",
        "Bernoulli",
        "Unknown quiet",
    ];
    for reverse in [false, true] {
        if reverse {
            rows.reverse();
        }
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(rows.clone(), false);
        view.selected = view.rows.iter().position(|s| s.id.id == "root").unwrap();
        let area = Rect::new(0, 0, 80, 45);
        let mut buffer = Buffer::empty(area);
        view.render_details(area, &mut buffer, now);
        let rendered = buffer
            .content
            .chunks(80)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let positions: Vec<_> = expected
            .iter()
            .map(|title| {
                rendered
                    .find(title)
                    .unwrap_or_else(|| panic!("missing {title}:\n{rendered}"))
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{rendered}"
        );
    }
}
