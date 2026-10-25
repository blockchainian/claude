use super::*;

#[test]
fn background_collects_commands_from_all_descendant_statuses_and_depths() {
    let now = Utc::now();
    let parent = session("root", "Parent", SessionStatus::Ready, 0);
    let mut rows = vec![parent];
    let mut parent = session_key("root");
    for (index, status) in [
        SessionStatus::Ready,
        SessionStatus::Inactive,
        SessionStatus::NeedsInput,
        SessionStatus::Working,
    ]
    .into_iter()
    .enumerate()
    {
        let mut child = session(&format!("child-{index}"), "Child", status, 0);
        child.id.provider = ProviderId::Codex;
        child.parent = Some(parent);
        child.background.push(crate::model::BackgroundTask {
            description: format!("sleep {}", 100 + index),
            output: None,
            started_at: now,
        });
        parent = child.id.clone();
        rows.push(child);
    }
    let mut unrelated = session("unrelated", "Other", SessionStatus::Ready, 0);
    unrelated.background.push(crate::model::BackgroundTask {
        description: "wrong command".into(),
        output: None,
        started_at: now,
    });
    rows.push(unrelated);
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(rows, false);
    view.selected = view
        .rows
        .iter()
        .position(|s| s.id == session_key("root"))
        .unwrap();
    assert!(view.has_background());
    let area = Rect::new(0, 0, 60, 30);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, now);
    let rendered = buf
        .content
        .chunks(60)
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    for command in ["sleep 100", "sleep 101", "sleep 102", "sleep 103"] {
        assert!(rendered.contains(command), "{command}: {rendered}");
    }
    assert!(!rendered.contains("wrong command"), "{rendered}");
    // With no working descendant, the commands still keep the section present.
    for row in &mut view.rows {
        if row.status == SessionStatus::Working {
            row.status = SessionStatus::Ready;
        }
    }
    assert!(view.has_background());
}

#[test]
fn background_without_output_shows_title_time_and_does_not_open_event() {
    let terminals: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/codex-background/terminals.json"
    ))
    .unwrap();
    let record: serde_json::Value = serde_json::from_str(
        include_str!("../../tests/fixtures/codex-background/rollout.jsonl")
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    let started: chrono::DateTime<Utc> = record["timestamp"].as_str().unwrap().parse().unwrap();
    let command = terminals["data"][0]["command"].as_str().unwrap();
    let mut row = session("background", "Background task", SessionStatus::Ready, 0);
    row.background.push(crate::model::BackgroundTask {
        description: command.into(),
        output: None,
        started_at: started,
    });
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 60, 20);
    let mut buf = Buffer::empty(area);
    view.render_task_details(area, &mut buf, started + Duration::seconds(65));
    let rendered = buf
        .content
        .chunks(60)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned();
    assert!(rendered.contains(command), "{rendered}");
    assert!(rendered.contains("1m 5s"), "{rendered}");
    assert!(view.viewport.borrow().task_details_background.is_empty());
    let before = view.visible_event().cloned();
    assert_eq!(double_click_text(&mut view, &rendered, command), None);
    assert_eq!(view.visible_event().cloned(), before);
    assert!(view.feedback.is_none());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/task_details_codex_background.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &rendered).unwrap();
    }
    assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
}
