use super::*;
use crate::log::Kind;

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
fn codex_nonzero_exit_title_keeps_status_out_of_body() {
    let event = codex_exit_event(1);
    let parts = detail_parts(&event);
    let lines: Vec<_> = parts
        .iter()
        .flat_map(|part| part.lines(80, Style::default(), event.started_at))
        .map(|line| line.to_string())
        .collect();
    assert_eq!(lines, ["false", "↓", "[scrubbed]"]);
    let detail = tool_detail::detail(&event, String::new(), Style::new().dim());
    assert_eq!(detail.sections[0].groups.len(), 3);
    assert_eq!(
        detail.sections[0].title.left.as_deref(),
        Some("Exec command · 1")
    );
    assert_eq!(detail.sections[0].title.title, "false");
    assert!(
        detail.sections[0]
            .groups
            .iter()
            .all(|group| group.parts.len() == 1)
    );
    assert_eq!(parts[2].source, "[scrubbed]");
    let arrow_lines = parts[1].lines(80, Style::default(), event.started_at);
    assert!(parts[1].source.is_empty());
    assert!(parts[1].maps(&arrow_lines)[0].is_empty());
    let lines = parts[2].lines(80, Style::default(), event.started_at);
    assert_eq!(
        parts[2].maps(&lines)[0].last().unwrap().1.end,
        parts[2].source.len()
    );
    assert_eq!(super::super::log::open_text(&event), "false\n\n[scrubbed]");
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
fn codex_exit_list_keeps_summary_and_nonzero_status() {
    assert_eq!(codex_exit_event(0).body, "false");
    assert_eq!(codex_exit_event(1).body, "false (1)");
}

#[test]
fn codex_exit_list_colors_only_status_digits() {
    for code in [0, 1, 101, -1] {
        let event = codex_exit_event(code);
        let line = super::super::log::body_line(&event, 80, Path::new("/"));
        let text = line.to_string();
        assert!(!text.contains("exit"));
        let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 1));
        line.render(buffer.area, &mut buffer);
        let base = super::super::style::log_styles(&Kind::Tool).1.fg.unwrap();
        assert_eq!(buffer[(0, 0)].fg, base);
        if code != 0 {
            assert_eq!(text, format!("false ({code})"));
            let start = text[..text.rfind(&code.to_string()).unwrap()]
                .chars()
                .count() as u16;
            assert_eq!(buffer[(start - 2, 0)].fg, base);
            for x in start..start + code.to_string().len() as u16 {
                assert_eq!(
                    buffer[(x, 0)].fg,
                    super::super::style::log_styles(&Kind::Error).1.fg.unwrap()
                );
            }
        }
    }
}

#[test]
fn codex_exit_detail_snapshot() {
    let (mut view, now) = log_fixture();
    let mut event = codex_exit_event(1);
    event.started_at = now;
    event.ended_at = Some(now);
    event.previous_end = None;
    event.duration = None;
    view.set_log(vec![event]);
    let rendered = log_screen(&view, 100, 40, now);
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/codex_exit_detail.txt");
    update_log_snapshot_body(&path, &rendered);
    assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
}

#[test]
fn command_exit_titles_use_error_colored_digits_for_both_providers() {
    use crate::providers::Provider;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("claude.jsonl");
    let mut records: Vec<serde_json::Value> =
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    records[1]["message"]["content"][0]["is_error"] = true.into();
    records[1]["message"]["content"][0]["content"] = "Exit code 101\nreason".into();
    records[1]["toolUseResult"] = "Error: Exit code 101\nreason".into();
    std::fs::write(
        &path,
        records[..2]
            .iter()
            .map(|v| format!("{v}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let provider = crate::providers::claude::ClaudeProvider::new(dir.path().into());
    let (mut view, _) = log_fixture();
    view.rows[0].transcript = Some(path);
    let claude = provider
        .log(&view.rows[0], usize::MAX, false)
        .unwrap()
        .pop()
        .unwrap();
    for mut event in [claude, codex_exit_event(101)] {
        assert_eq!(event.calls[0].exit_code, Some(101));
        for code in [Some(101), Some(0), None] {
            event.calls[0].exit_code = code;
            for count in [1, 2] {
                let mut event = event.clone();
                event.calls = vec![event.calls[0].clone(); count];
                let detail = tool_detail::detail(&event, String::new(), Style::new().dim());
                let columns = super::super::detail_model::detail_columns(
                    Rect::new(0, 0, 120, 30),
                    Rect::new(0, 0, 40, 30),
                );
                for section in &detail.sections {
                    let (line, _) = section.title.line(columns);
                    let text = line.to_string();
                    assert!(!text.contains("exit"), "{text}");
                    assert_eq!(text.contains("· 101"), code == Some(101), "{text}");
                    let mut buffer = Buffer::empty(columns.text);
                    line.render(columns.text, &mut buffer);
                    if code == Some(101) {
                        let start = text[..text.find("101").unwrap()].chars().count() as u16;
                        for x in start..start + 3 {
                            assert_eq!(
                                buffer[(x, 0)].fg,
                                super::super::style::log_styles(&Kind::Error).1.fg.unwrap()
                            );
                        }
                        assert_eq!(buffer[(0, 0)].fg, Color::Reset);
                        assert_eq!(buffer[(start - 2, 0)].fg, Color::Reset);
                    }
                }
            }
        }
    }
}

#[test]
fn command_exit_activity_colors_only_status_digits() {
    let (mut view, now) = log_fixture();
    view.rows[0].activity = Some(crate::model::Activity {
        exit_code: Some(101),
        kind: Kind::Tool,
        label: "false (101)".into(),
        at: now,
    });
    let area = Rect::new(0, 0, 160, 45);
    let mut buffer = Buffer::empty(area);
    view.render_at(area, &mut buffer, now);
    let mut found = false;
    for y in 0..area.height {
        let text = (0..area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(start) = text.find("false (101)") {
            let x = text[..start].chars().count() as u16;
            assert_eq!(
                buffer[(x + 6, y)].fg,
                super::super::style::log_styles(&Kind::Tool).1.fg.unwrap()
            );
            for x in x + 7..x + 10 {
                assert_eq!(
                    buffer[(x, y)].fg,
                    super::super::style::log_styles(&Kind::Error).1.fg.unwrap()
                );
            }
            found = true;
        }
    }
    assert!(found);
}
