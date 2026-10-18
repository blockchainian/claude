use super::*;

fn events(fixture: &str) -> Vec<Event> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transcript.jsonl");
    std::fs::write(&path, fixture).unwrap();
    crate::providers::claude::log::tail(&path, 200).unwrap()
}

fn layout(event: &Event, width: u16) -> DetailLayout {
    detail_layout(event, width, Path::new("/"), DateTime::UNIX_EPOCH)
}

fn body_parts(event: &Event) -> Vec<super::super::tool_detail::Part> {
    super::super::tool_detail::detail(event, String::new(), Style::new().dim())
        .sections
        .into_iter()
        .flat_map(|section| section.groups)
        .flat_map(|group| group.parts)
        .collect()
}

fn assert_reading_column(layout: &DetailLayout, segment: usize) {
    let rows: Vec<_> = layout
        .rows
        .iter()
        .filter(|(id, cells)| *id == segment && !cells.is_empty())
        .collect();
    assert!(!rows.is_empty());
    let width = layout.columns.text.width;
    let left = 0;
    for (_, cells) in rows {
        assert!(cells.first().unwrap().0.start >= left);
        assert!(
            cells.last().unwrap().0.end <= left + width,
            "body exceeds reading column: {:?}",
            cells.last()
        );
    }
}

#[test]
fn prose_body_kinds_use_the_log_column_width() {
    let mut event = events(include_str!("../../tests/fixtures/transcript.jsonl")).remove(0);
    event.calls.clear();
    event.body = "A readable sentence with enough words to span the full panel width. ".repeat(8);
    for kind in [
        Kind::User,
        Kind::Assistant,
        Kind::QueuedUser,
        Kind::QueuedSystem,
        Kind::Compaction,
        Kind::Agent,
    ] {
        event.kind = kind;
        for width in [200, 120] {
            let drawn = layout(&event, width);
            assert_reading_column(&drawn, 1);
            assert_eq!(drawn.sources[1].text.as_ref(), event.body);
            if width == 120 {
                assert!(
                    drawn
                        .rows
                        .iter()
                        .any(|(_, cells)| cells.last().is_some_and(|(range, _)| range.end > 60))
                );
            }
        }
    }
}

#[test]
fn mcp_inputs_keep_only_declared_prompt_in_body() {
    let parsed = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
    let event = parsed
        .iter()
        .find(|e| e.calls.iter().any(|c| c.name.ends_with("__start")))
        .unwrap();
    let call = event
        .calls
        .iter()
        .find(|c| c.name.ends_with("__start"))
        .unwrap();
    let mut event = event.clone();
    event.calls = vec![call.clone()];
    let parts = body_parts(&event);
    assert_eq!(event.calls[0].detail_title, "tool-calls");
    assert_eq!(event.calls[0].detail_left, "codex-manager start");
    let input = parts
        .iter()
        .find(|part| part.source.starts_with("Goal:"))
        .unwrap();
    assert!(!input.source.contains("\"cwd\":"));
    for width in [200, 120] {
        let drawn = layout(&event, width);
        let segment = drawn
            .sources
            .iter()
            .position(|source| source.text.as_ref() == input.source)
            .unwrap();
        assert_reading_column(&drawn, segment);
    }
    event.calls[0].input = vec![
        crate::log::Section::Json {
            label: "input".into(),
            text: r#"{"threadId":"abc-123"}"#.into(),
        },
        crate::log::Section::Prose {
            text: "Continue reading this message. ".repeat(20),
            markdown: true,
        },
    ];
    event.calls[0].detail_title = "Send".into();
    let drawn = layout(&event, 200);
    assert_reading_column(&drawn, 1);
    assert!(
        drawn
            .sources
            .iter()
            .any(|s| s.text.contains("\"threadId\":"))
    );
}

#[test]
fn pending_tool_copy_preserves_prompt_without_display_only_suffix() {
    let fixture = include_str!("../../tests/fixtures/reading-column-send.jsonl");
    let record: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let prompt = record["message"]["content"][0]["input"]["prompt"]
        .as_str()
        .unwrap();
    let mut event = events(fixture).remove(0);
    assert_eq!(open_text(&event), prompt);
    let text = format!("\n{prompt}\n\n");
    event.calls[0].input = vec![crate::log::Section::Prose {
        text: text.clone(),
        markdown: false,
    }];
    assert_eq!(open_text(&event), text);
}

#[test]
fn question_agent_inputs_limit_prose_and_preserve_tool_output() {
    for fixture in [
        include_str!("../../tests/fixtures/claude-questions-answered.jsonl"),
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl"),
    ] {
        for event in events(fixture) {
            if !event
                .calls
                .iter()
                .any(|c| c.name == "AskUserQuestion" || c.name == "Agent")
            {
                continue;
            }
            for width in [200, 120] {
                let drawn = layout(&event, width);
                let input = event.calls[0]
                    .input
                    .iter()
                    .find_map(|s| match s {
                        crate::log::Section::Prose { text, .. } => Some(text.as_str()),
                        crate::log::Section::Markdown { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .unwrap();
                let segment = drawn
                    .sources
                    .iter()
                    .position(|s| s.text.as_ref() == input)
                    .unwrap();
                assert_reading_column(&drawn, segment);
            }
        }
    }
    let mut event = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Bash"))
        .unwrap();
    event.calls.truncate(1);
    let output = "tool output with many words ".repeat(20);
    event.calls[0].output = Some(vec![crate::log::Section::Text {
        label: "stdout".into(),
        text: output.clone(),
    }]);
    for width in [200, 120] {
        let drawn = layout(&event, width);
        let id = drawn
            .sources
            .iter()
            .position(|source| source.text.as_ref() == output)
            .unwrap();
        let rows: Vec<_> = drawn
            .rows
            .iter()
            .enumerate()
            .filter(|(_, (segment, _))| *segment == id)
            .map(|(row, _)| &drawn.lines[row])
            .collect();
        assert!(rows[0].to_string().ends_with('…'));
        assert_eq!(rows[0].width(), usize::from(drawn.columns.text.width));
        assert_eq!(
            rows.len(),
            1,
            "tool output retains one logical line at {width}"
        );
    }
}

#[test]
fn generic_roles_are_explicit_and_field_names_are_literal() {
    use crate::log::Section;
    let mut event = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
        .into_iter()
        .find(|e| !e.calls.is_empty())
        .unwrap();
    event.calls.truncate(1);
    for key in [
        "prompt",
        "message",
        "question",
        "command",
        "name",
        "description",
    ] {
        let fields = Section::Json {
            label: "input".into(),
            text: serde_json::json!({key: "Some words to read. ".repeat(12)}).to_string(),
        };
        event.calls[0].input = vec![
            fields.clone(),
            Section::Prose {
                text: "Explicit prose. ".repeat(12),
                markdown: true,
            },
        ];
        event.calls[0].output = Some(vec![fields]);
        let drawn = layout(&event, 200);
        assert!(drawn.sources[1].text.contains(&format!("\"{key}\": ")));
        assert!(drawn.sources[2].text.starts_with("Explicit prose."));
        assert_reading_column(&drawn, 2);
        assert!(
            drawn
                .sources
                .last()
                .unwrap()
                .text
                .contains(&format!("\"{key}\": "))
        );
    }
}

#[test]
fn command_and_all_output_body_formats_keep_their_width() {
    use crate::log::Section;
    let mut event = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
        .into_iter()
        .find(|e| e.calls.iter().any(|c| c.name == "Bash"))
        .unwrap();
    event.calls.truncate(1);
    let text = "long command or output stays at the panel width ".repeat(10);
    event.calls[0].input = vec![Section::Shell {
        prefix: 0,
        text: format!("echo {text}\necho second"),
    }];
    event.calls[0].output = Some(vec![
        Section::Text {
            label: "stdout".into(),
            text: text.clone(),
        },
        Section::Markdown {
            label: "result".into(),
            text: text.clone(),
        },
        Section::Code {
            label: "output".into(),
            lang: "text".into(),
            text: text.clone(),
        },
        Section::Json {
            label: "output".into(),
            text: serde_json::json!({"prompt": text}).to_string(),
        },
    ]);
    for part in body_parts(&event) {
        for width in [200, 120] {
            let lines = part.lines(width, Style::default(), DateTime::UNIX_EPOCH);
            if part.markdown() {
                assert!(lines.iter().all(|line| line.width() <= width));
            } else if part.source.contains(&text) {
                assert!(lines.iter().any(|line| line.width() > width));
            }
        }
    }
}

#[test]
fn queued_fragments_keep_json_formatting_and_markdown_detection() {
    let mut event = events(include_str!("../../tests/fixtures/transcript.jsonl")).remove(0);
    event.calls.clear();
    event.kind = Kind::QueuedUser;
    event.body = "Read **this** before {\"answer\":[1,2]} and continue".into();
    let expected = super::super::tool_detail::Part::text_parts(event.body.clone(), false);
    let actual = body_parts(&event);
    assert_eq!(
        actual.iter().map(|p| &p.source).collect::<Vec<_>>(),
        expected.iter().map(|p| &p.source).collect::<Vec<_>>()
    );
    for part in actual {
        assert!(
            part.lines(200, Style::default(), DateTime::UNIX_EPOCH)
                .iter()
                .all(|line| line.width() <= 200)
        );
    }
}

#[test]
fn merged_calls_keep_descriptions_in_title_bars() {
    let mut event = events(include_str!(
        "../../tests/fixtures/claude-manager-start.jsonl"
    ))
    .into_iter()
    .find(|e| e.calls.iter().any(|c| c.name.ends_with("__start")))
    .unwrap();
    let call = event
        .calls
        .iter()
        .find(|c| c.name.ends_with("__start"))
        .unwrap()
        .clone();
    event.calls = vec![call.clone(), call];
    let detail = super::super::tool_detail::detail(&event, String::new(), Style::new().dim());
    let title = &detail.sections[0].title;
    assert_eq!(title.source(), event.calls[0].detail_title.clone());
    let description = event.calls[0].detail_title.clone();
    assert_eq!(title.left, Some(event.calls[0].detail_left.clone()));
    assert!(
        !body_parts(&event)
            .iter()
            .any(|part| part.source == description)
    );
    for width in [120, 200] {
        assert_centered_blocks(&event, width);
        let columns = layout(&event, width).columns;
        let (line, maps) = title.line(columns);
        assert!(line.to_string().contains(&description));
        let source = title.source();
        assert_eq!(
            maps.iter()
                .map(|(_, bytes)| &source[bytes.clone()])
                .collect::<String>(),
            description
        );
    }
}

fn codex_events(fixture: &str) -> Vec<Event> {
    use crate::providers::Provider;
    let dir = tempfile::tempdir().unwrap();
    rusqlite::Connection::open(dir.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let mut provider = crate::providers::codex::CodexProvider::new(
        dir.path().to_owned(),
        dir.path().join("manager"),
    );
    let mut session = provider.refresh(1).sessions.remove(0);
    session.transcript = Some(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture),
    );
    provider.log(&session, usize::MAX, false).unwrap()
}

/// Every input/output group follows the panel midline and preserves its own indentation.
fn assert_centered_blocks(event: &Event, width: u16) {
    let detail = super::super::tool_detail::detail(event, String::new(), Style::new().dim());
    let drawn = layout(event, width);
    let cap = usize::from(drawn.columns.text.width);
    let mut id = 0;
    for section in detail.sections {
        id += 1;
        for group in section.groups {
            let start = id;
            id += group.parts.len();
            let widest = group
                .parts
                .iter()
                .flat_map(|part| {
                    part.lines(
                        usize::from(drawn.columns.reading_width.min(drawn.columns.text.width)),
                        Style::default(),
                        DateTime::UNIX_EPOCH,
                    )
                })
                .map(|line| line.width())
                .max()
                .unwrap_or(0)
                .min(cap);
            let padding = usize::from(
                super::super::centering::centered_x(
                    drawn.columns.panel,
                    drawn.columns.text,
                    widest as u16,
                ) - drawn.columns.text.x,
            );
            for (row, (segment, cells)) in drawn
                .rows
                .iter()
                .enumerate()
                .filter(|(_, (segment, _))| (start..id).contains(segment))
            {
                assert!(drawn.lines[row].width() <= cap);
                assert!(
                    cells
                        .iter()
                        .all(|(cells, _)| cells.start as usize >= padding
                            && cells.end as usize <= cap),
                    "{} segment {segment}",
                    event.body
                );
                if !drawn.lines[row].to_string().trim().is_empty() {
                    assert!(
                        drawn.lines[row]
                            .to_string()
                            .starts_with(&" ".repeat(padding))
                    );
                }
            }
            let actual = drawn
                .rows
                .iter()
                .enumerate()
                .filter(|(_, (segment, _))| (start..id).contains(segment))
                .map(|(row, _)| drawn.lines[row].width())
                .max()
                .unwrap_or(0);
            assert_eq!(actual, padding + widest, "{} at {width}", event.body);
        }
    }
}

#[test]
fn subagent_activity_blocks_are_centered() {
    let events = codex_events("codex-inner/subagent.jsonl");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].calls[0].name, "SubAgentActivity");
    for width in [200, 151, 120, 41] {
        assert_centered_blocks(&events[0], width);
    }
}

#[test]
fn provider_detail_blocks_are_centered() {
    let mut entries = Vec::new();
    for fixture in [
        include_str!("../../tests/fixtures/transcript.jsonl"),
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl"),
        include_str!("../../tests/fixtures/claude-tool-details.jsonl"),
        include_str!("../../tests/fixtures/claude-questions-answered.jsonl"),
        include_str!("../../tests/fixtures/claude-peer-prompts.jsonl"),
        include_str!("../../tests/fixtures/claude-compaction.jsonl"),
        include_str!("../../tests/fixtures/claude-questions-pending.jsonl"),
        include_str!("../../tests/fixtures/claude-questions-rejected.jsonl"),
        include_str!("../../tests/fixtures/claude-task-notifications.jsonl"),
        include_str!("../../tests/fixtures/claude-task-notification-delivery.jsonl"),
    ] {
        entries.extend(events(fixture));
    }
    for fixture in [
        "codex-rollout.jsonl",
        "codex-tool-calls.jsonl",
        "codex-command-failed.jsonl",
        "codex-inner/subagent.jsonl",
        "codex-inner/mcp.jsonl",
        "codex-inner/extension.jsonl",
        "codex-inner/command.jsonl",
        "codex-inner/file-change.jsonl",
        "codex-inner/image-view.jsonl",
        "codex-inner/image-generation.jsonl",
        "codex-inner/collaboration.jsonl",
        "codex-input/default-pending.jsonl",
        "codex-input/default-answered.jsonl",
        "codex-input/plan-pending.jsonl",
        "codex-input/plan-answered.jsonl",
    ] {
        entries.extend(codex_events(fixture));
    }
    assert!(entries.len() > 30);
    for event in entries {
        for width in [200, 151, 120] {
            assert_centered_blocks(&event, width);
        }
    }
}

#[test]
fn centered_detail_blocks_snapshot() {
    let mut entries = codex_events("codex-inner/subagent.jsonl");
    entries.extend(
        codex_events("codex-tool-calls.jsonl")
            .into_iter()
            .filter(|event| event.calls.iter().any(|call| call.name == "spawn_agent")),
    );
    entries.extend(
        events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
            .into_iter()
            .filter(|event| event.calls.iter().any(|call| call.name == "WebFetch")),
    );
    let mut snapshot = String::new();
    for event in entries {
        for width in [120, 200] {
            snapshot.push_str(&format!("{} — {width} columns\n", event.body));
            for line in layout(&event, width).lines {
                snapshot.push_str(line.to_string().trim_end());
                snapshot.push('\n');
            }
        }
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/detail_blocks_centered.txt");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn description_takes_priority_over_name_in_real_agent_inputs() {
    let parsed = events(include_str!(
        "../../tests/fixtures/claude-agent-display.jsonl"
    ));
    let event = parsed
        .iter()
        .find(|event| {
            event
                .calls
                .iter()
                .any(|call| call.id == "toolu_015h3Ngbytx63sMddkJwjDX7")
        })
        .unwrap();
    let call = &event.calls[0];
    assert_eq!(
        Some(call.detail_title.as_str()),
        Some("实测 attach 发提示词")
    );
    let body = body_parts(event);
    assert!(
        !body
            .iter()
            .any(|part| part.source.contains("name: attach-probe"))
    );
    assert!(
        !body
            .iter()
            .any(|part| part.source == "实测 attach 发提示词")
    );
}

#[test]
fn notification_preformatted_blocks_preserve_logical_lines() {
    let output = include_str!("../../tests/fixtures/claude-task-notification-output.txt");
    let dir = tempfile::tempdir().unwrap();
    let event = super::log_markdown::notification_events(dir.path(), Some(output)).remove(0);
    let value: serde_json::Value = serde_json::Deserializer::from_str(output)
        .into_iter()
        .next()
        .unwrap()
        .unwrap();
    let message = value["lastMessage"].as_str().unwrap();
    let command_line = message
        .lines()
        .find(|line| line.ends_with("Claude provider:"))
        .unwrap();
    for width in [200, 90] {
        let drawn = layout(&event, width);
        let continuation = drawn
            .sources
            .iter()
            .find(|source| source.text.contains("lastMessage"))
            .unwrap();
        for expected in message.lines().filter(|line| !line.is_empty()) {
            assert!(continuation.text.contains(expected));
        }
        let rows: Vec<_> = drawn
            .rows
            .iter()
            .enumerate()
            .filter(|(_, (segment, _))| *segment == continuation.id)
            .collect();
        assert_eq!(rows.len(), continuation.text.lines().count());
        for ((row, _), source) in rows.into_iter().zip(continuation.text.lines()) {
            let expected = truncate_detail_line(
                Line::from(source.to_owned()),
                &mut Vec::new(),
                drawn.columns.text.width.into(),
            );
            assert_eq!(
                drawn.lines[row].to_string().trim_start(),
                expected.to_string().trim_start()
            );
        }
        assert!(
            continuation.text.contains(&format!("\n  {command_line}")),
            "JSON continuations use structural indent"
        );
    }
}

#[test]
fn command_preformatted_blocks_preserve_logical_lines() {
    recorded_preformatted_block(false);
}

#[test]
fn code_preformatted_blocks_preserve_logical_lines() {
    recorded_preformatted_block(true);
}

fn recorded_preformatted_block(code: bool) {
    let output = include_str!("../../tests/fixtures/claude-task-notification-output.txt");
    let dir = tempfile::tempdir().unwrap();
    let event = super::log_markdown::notification_events(dir.path(), Some(output)).remove(0);
    let value: serde_json::Value = serde_json::Deserializer::from_str(output)
        .into_iter()
        .next()
        .unwrap()
        .unwrap();
    let message = value["lastMessage"].as_str().unwrap();
    let command_line = message
        .lines()
        .find(|line| line.ends_with("Claude provider:"))
        .unwrap();
    // Both examples come verbatim from the completion's recorded terminal capture.
    for section in [
        crate::log::Section::Text {
            label: "stdout".into(),
            text: command_line.into(),
        },
        crate::log::Section::Code {
            label: "output".into(),
            lang: "text".into(),
            text: command_line.into(),
        },
        crate::log::Section::Markdown {
            label: "message".into(),
            text: format!("Result\n\n```text\n{command_line}\n```"),
        },
    ] {
        if matches!(section, crate::log::Section::Text { .. }) == code {
            continue;
        }
        let mut event = event.clone();
        event.calls[0].output = Some(vec![section]);
        for width in [200, 90] {
            let drawn = layout(&event, width);
            let expected = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
                Line::from(command_line.to_owned()),
                drawn.columns.text.width.into(),
            )
            .to_string();
            assert!(
                drawn
                    .lines
                    .iter()
                    .any(|line| line.to_string().trim_start() == expected),
                "wrapped command/code at {width}"
            );
        }
    }
}

#[test]
fn notification_preformatted_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let event = super::log_markdown::notification_events(
        dir.path(),
        Some(include_str!(
            "../../tests/fixtures/claude-task-notification-output.txt"
        )),
    )
    .remove(0);
    let mut snapshot = String::new();
    for width in [200, 90] {
        snapshot.push_str(&format!("{width} columns\n"));
        for line in layout(&event, width).lines {
            snapshot.push_str(line.to_string().trim_end());
            snapshot.push('\n');
        }
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/task_notification_json.txt");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn equal_reading_width_wraps_prose_identically_in_wide_and_narrow_panels() {
    let event = events(include_str!("../../tests/fixtures/transcript.jsonl"))
        .into_iter()
        .find(|event| event.kind == Kind::Assistant)
        .unwrap();
    let narrow = layout(&event, 98);
    let wide = layout(&event, 200);
    assert_eq!(narrow.columns.reading_width, wide.columns.reading_width);
    assert_ne!(narrow.columns.text.width, wide.columns.text.width);
    assert_eq!(
        narrow
            .lines
            .iter()
            .skip(1)
            .map(|line| line.to_string().trim_start().to_owned())
            .collect::<Vec<_>>(),
        wide.lines
            .iter()
            .skip(1)
            .map(|line| line.to_string().trim_start().to_owned())
            .collect::<Vec<_>>(),
    );
    assert!(
        narrow
            .lines
            .iter()
            .all(|line| line.width() <= narrow.columns.text.width.into())
    );
    assert_centered_blocks(&event, 98);
    assert_centered_blocks(&event, 200);
}

#[test]
fn merged_tool_durations_are_separate_from_content_lines() {
    let event = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
        .into_iter()
        .find(|event| event.calls.len() > 1)
        .unwrap();
    for width in [120, 200] {
        let drawn = layout(&event, width);
        let expected: Vec<_> = event
            .calls
            .iter()
            .filter_map(|call| call.duration.map(format_log_duration))
            .collect();
        assert!(!expected.is_empty());
        assert_eq!(
            drawn
                .times
                .iter()
                .map(|(_, time)| time.content.to_string())
                .collect::<Vec<_>>(),
            expected
        );
        for (row, time) in &drawn.times {
            assert_eq!(
                drawn.lines[*row].width(),
                usize::from(drawn.columns.text.width)
            );
            assert!(
                !drawn.lines[*row]
                    .to_string()
                    .contains(time.content.as_ref())
            );
        }
    }
}

#[test]
fn restored_tool_details_snapshot() {
    let mut entries = Vec::new();
    for fixture in [
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl"),
        include_str!("../../tests/fixtures/claude-tool-details.jsonl"),
        include_str!("../../tests/fixtures/claude-skill-call.jsonl"),
        include_str!("../../tests/fixtures/claude-agent-names.jsonl"),
    ] {
        entries.extend(events(fixture));
    }
    entries.extend(codex_events("codex-agent-names.jsonl"));
    let mut snapshot = String::new();
    for event in entries {
        for call in &event.calls {
            if !matches!(
                call.name.as_str(),
                "Agent"
                    | "Task"
                    | "SendMessage"
                    | "WebFetch"
                    | "Skill"
                    | "spawn_agent"
                    | "send_message"
            ) {
                continue;
            }
            let mut selected = event.clone();
            selected.calls = vec![call.clone()];
            for width in [120, 200] {
                snapshot.push_str(&format!("{} {} — {width} columns\n", call.name, call.id));
                for line in layout(&selected, width).lines {
                    snapshot.push_str(line.to_string().trim_end());
                    snapshot.push('\n');
                }
            }
        }
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/restored_tool_details.txt");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}
