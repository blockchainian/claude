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

fn assert_reading_column(layout: &DetailLayout, segment: usize, wide: bool) {
    let rows: Vec<_> = layout
        .rows
        .iter()
        .filter(|(id, cells)| *id == segment && !cells.is_empty())
        .collect();
    assert!(!rows.is_empty());
    let width = layout.content_width;
    let left = if wide { (layout.width - width) / 2 } else { 0 };
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
            assert_reading_column(&drawn, 0, width == 200);
            assert_eq!(drawn.sources[0].text.as_ref(), event.body);
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
fn generic_input_fields_move_name_to_header_and_keep_ids_in_body() {
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
    let name = call
        .input
        .iter()
        .find_map(|s| match s {
            crate::log::Section::Fields(fields) => fields
                .iter()
                .find(|(k, _)| k == "name")
                .map(|(_, v)| v.clone()),
            _ => None,
        })
        .unwrap();
    let parts = open_parts(&event);
    assert!(
        !parts
            .iter()
            .take_while(|p| !p.source.is_empty())
            .any(|p| p.source.starts_with("name: ") || p.source.contains("\nname: "))
    );
    assert_eq!(
        super::super::tool_detail::input_label(&event.calls[0]).as_deref(),
        Some(name.as_str())
    );
    assert!(parts.iter().any(|p| p.source.starts_with("cwd: ")));
    assert!(!parts.iter().any(|p| p.source.starts_with("prompt: ")));
    for width in [200, 120] {
        let drawn = layout(&event, width);
        for (segment, source) in drawn.sources.iter().enumerate().filter(|(_, s)| {
            s.text.starts_with("cwd: ") || s.text.starts_with("Implement workstream")
        }) {
            let _ = source;
            assert_reading_column(&drawn, segment, width == 200);
        }
    }
    event.calls[0].input = vec![crate::log::Section::Fields(vec![
        ("threadId".into(), "abc-123".into()),
        (
            "prompt".into(),
            "Continue reading this message. ".repeat(20),
        ),
    ])];
    event.calls[0].name = "mcp__plugin_codex_codex-manager__send".into();
    assert_eq!(
        super::super::tool_detail::input_label(&event.calls[0]).as_deref(),
        None
    );
    let drawn = layout(&event, 200);
    assert_reading_column(&drawn, 0, true);
    assert!(
        drawn
            .sources
            .iter()
            .any(|s| s.text.starts_with("threadId:"))
    );
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
                        crate::log::Section::Text { text, label } if label == "questions" => {
                            Some(text.as_str())
                        }
                        crate::log::Section::Markdown { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .unwrap();
                let segment = drawn
                    .sources
                    .iter()
                    .position(|s| s.text.as_ref() == input)
                    .unwrap();
                assert_reading_column(&drawn, segment, width == 200);
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
        let rows: Vec<_> = drawn
            .lines
            .iter()
            .filter(|line| line.to_string().contains(&output))
            .collect();
        assert_eq!(
            rows.len(),
            1,
            "tool output retains one logical line at {width}"
        );
    }
}

#[test]
fn generic_roles_depend_on_field_names_and_only_on_inputs() {
    use crate::log::Section;
    let mut event = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
        .into_iter()
        .find(|e| e.calls.iter().any(|c| c.name.ends_with("__start")))
        .unwrap();
    event.calls.truncate(1);
    for key in ["prompt", "message", "question"] {
        event.calls[0].name = "any_provider.any_tool".into();
        event.calls[0].input = vec![Section::Fields(vec![
            ("title".into(), "Readable task".into()),
            ("sessionId".into(), "abc".into()),
            ("cwd".into(), "/workspace".into()),
            (
                key.into(),
                "Some words to read without a field prefix. ".repeat(12),
            ),
        ])];
        event.calls[0].output = Some(event.calls[0].input.clone());
        assert_eq!(
            super::super::tool_detail::input_label(&event.calls[0]).as_deref(),
            None
        );
        let drawn = layout(&event, 200);
        assert!(drawn.sources[0].text.contains("title: Readable task"));
        assert!(drawn.sources[0].text.contains("sessionId: abc"));
        assert!(drawn.sources[0].text.contains("cwd: /workspace"));
        assert_reading_column(&drawn, 0, true);
        assert_reading_column(&drawn, 1, true);
        assert!(
            drawn
                .sources
                .last()
                .unwrap()
                .text
                .contains(&format!("{key}: Some words"))
        );
        assert!(
            drawn
                .sources
                .last()
                .unwrap()
                .text
                .contains("sessionId: abc")
        );
    }
    for value in ["a\nb".to_owned(), "x".repeat(81)] {
        event.calls[0].input = vec![Section::Fields(vec![("name".into(), value.clone())])];
        assert_eq!(
            super::super::tool_detail::input_label(&event.calls[0]),
            Some(value.clone())
        );
        assert!(
            !open_parts(&event)
                .iter()
                .any(|part| part.source == format!("name: {value}"))
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
    event.calls[0].input = vec![Section::Code {
        label: "command".into(),
        lang: "bash".into(),
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
        Section::Fields(vec![("prompt".into(), text.clone())]),
    ]);
    for part in open_parts(&event) {
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
    let actual = open_parts(&event);
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
    let parts = open_parts(&event);
    assert_eq!(parts[0].source, event.calls[0].title);
    let description = super::super::tool_detail::input_label(&event.calls[0]).unwrap();
    assert!(!parts.iter().any(|part| part.source == description));
    assert!(
        parts[0].lines(120, Style::default(), DateTime::UNIX_EPOCH)[0]
            .to_string()
            .starts_with(&description)
    );
    for width in [120, 200] {
        assert_centered_blocks(&event, width);
        let lines = parts[0].lines(width.into(), Style::default(), DateTime::UNIX_EPOCH);
        let maps = parts[0].maps(&lines);
        assert_eq!(
            maps[0]
                .iter()
                .map(|(_, bytes)| &parts[0].source[bytes.clone()])
                .collect::<String>(),
            event.calls[0].title
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

/// Every input/output group has equal outer margins, while preserving its own indentation.
fn assert_centered_blocks(event: &Event, width: u16) {
    let parts = open_parts(event);
    let drawn = layout(event, width);
    let mut widths = std::collections::BTreeMap::<usize, usize>::new();
    for (index, part) in parts.iter().enumerate() {
        let widest = part
            .lines(
                drawn.content_width.into(),
                Style::default(),
                DateTime::UNIX_EPOCH,
            )
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0);
        let width = widths.entry(part.group.unwrap_or(index)).or_default();
        *width = (*width).max(widest);
    }
    let mut groups = std::collections::BTreeMap::<(usize, usize), Vec<usize>>::new();
    for (index, part) in parts.iter().enumerate() {
        let group = part.group.unwrap_or(index);
        let kind = if widths[&group] > usize::from(drawn.content_width) {
            part.content_kind() as usize
        } else {
            0
        };
        groups.entry((group, kind)).or_default().push(index);
    }
    for indices in groups.values() {
        // Merged title rules also carry a right-aligned duration, outside the body blocks.
        if indices.len() == 1
            && event.calls.len() > 1
            && event
                .calls
                .iter()
                .any(|call| call.title == parts[indices[0]].source)
        {
            continue;
        }
        let rows: Vec<_> = drawn
            .rows
            .iter()
            .enumerate()
            .filter(|(_, (id, _))| indices.contains(id))
            .map(|(row, _)| &drawn.lines[row])
            .collect();
        let nonempty: Vec<_> = rows
            .iter()
            .map(|line| line.to_string())
            .filter(|text| !text.trim().is_empty())
            .collect();
        if nonempty.is_empty() {
            continue;
        }
        let left = nonempty
            .iter()
            .map(|text| text.width() - text.trim_start().width())
            .min()
            .unwrap();
        let right = nonempty
            .iter()
            .map(|text| usize::from(width).saturating_sub(text.width()))
            .min()
            .unwrap();
        assert!(
            if width >= super::super::render::WIDE_LAYOUT_COLUMNS {
                left.abs_diff(right) <= 1
            } else {
                left == 0
            },
            "{} {:?} group {:?}: margins {left}/{right} at width {width}: {nonempty:#?}",
            event.body,
            event.kind,
            indices
        );
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
        super::super::tool_detail::input_label(call).as_deref(),
        Some("实测 attach 发提示词")
    );
    let body = open_parts(event);
    assert!(body.iter().any(|part| part.source == "name: attach-probe"));
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
        for expected in message.lines().filter(|line| !line.is_empty()) {
            assert!(
                drawn
                    .lines
                    .iter()
                    .any(|line| line.to_string().contains(expected)),
                "lost JSON logical line at {width}: {expected}"
            );
        }
        let continuation = drawn
            .sources
            .iter()
            .find(|source| source.text.contains("lastMessage"))
            .unwrap();
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
            assert!(
                drawn
                    .lines
                    .iter()
                    .any(|line| line.to_string().trim_start() == command_line),
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
