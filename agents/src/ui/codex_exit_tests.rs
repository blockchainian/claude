use super::*;

fn codex_exit_event(code: i32) -> crate::log::Event {
    use crate::providers::Provider;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("failed.jsonl");
    let mut record: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/codex-command-failed.jsonl"
    ))
    .unwrap();
    record["payload"]["item"]["exit_code"] = code.into();
    if code == 0 {
        record["payload"]["item"]["status"] = "completed".into();
    }
    std::fs::write(&path, format!("{record}\n")).unwrap();
    let provider =
        crate::providers::codex::CodexProvider::new(dir.path().into(), dir.path().join("manager"));
    let (mut view, _) = log_fixture();
    view.rows[0].transcript = Some(path);
    provider
        .log(&view.rows[0], usize::MAX, false)
        .unwrap()
        .pop()
        .unwrap()
}

#[test]
fn codex_nonzero_exit_parts_have_separate_centering_and_copy_sources() {
    let event = codex_exit_event(1);
    let parts = detail_parts(&event);
    let lines: Vec<_> = parts
        .iter()
        .flat_map(|part| part.lines(80, Style::default(), event.started_at))
        .map(|line| line.to_string())
        .collect();
    assert_eq!(lines, ["false", "↓", "exit code: 1", "↓", "[scrubbed]"]);
    let detail = tool_detail::detail(&event, String::new(), Style::new().dim());
    assert_eq!(detail.sections[0].groups.len(), 5);
    assert!(
        detail.sections[0]
            .groups
            .iter()
            .all(|group| group.parts.len() == 1)
    );
    assert_eq!(parts[2].source, "exit code: 1");
    for index in [1, 3] {
        let lines = parts[index].lines(80, Style::default(), event.started_at);
        assert!(parts[index].source.is_empty());
        assert!(parts[index].maps(&lines)[0].is_empty());
    }
    let lines = parts[2].lines(80, Style::default(), event.started_at);
    assert_eq!(
        parts[2].maps(&lines)[0].last().unwrap().1.end,
        parts[2].source.len()
    );
    assert_eq!(
        super::super::log::open_text(&event),
        "false\n\nexit code: 1\n\n[scrubbed]"
    );
    assert!(event.calls[0].is_error);
}

#[test]
fn codex_zero_exit_parts_keep_input_arrow_output() {
    let event = codex_exit_event(0);
    let lines: Vec<_> = detail_parts(&event)
        .iter()
        .flat_map(|part| part.lines(80, Style::default(), event.started_at))
        .map(|line| line.to_string())
        .collect();
    assert_eq!(lines, ["false", "↓", "[scrubbed]"]);
    assert!(!event.calls[0].is_error);
}

#[test]
fn codex_nonzero_exit_list_ends_with_arrow_and_number() {
    assert!(codex_exit_event(1).body.ends_with(" → 1"));
    assert!(!codex_exit_event(0).body.contains("→"));
}

#[test]
fn codex_exit_detail_snapshot() {
    let (mut view, now) = log_fixture();
    let mut event = codex_exit_event(1);
    event.started_at = now;
    event.ended_at = Some(now);
    event.previous_end = None;
    event.tool_time = crate::log::ToolTime::Turn;
    view.set_log(vec![event]);
    let rendered = log_screen(&view, 100, 40, now);
    assert_eq!(
        rendered,
        include_str!("../../tests/ui_snapshots/codex_exit_detail.txt")
    );
}
