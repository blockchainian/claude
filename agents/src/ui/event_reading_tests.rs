use super::*;

fn events(fixture: &str) -> Vec<Event> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transcript.jsonl");
    std::fs::write(&path, fixture).unwrap();
    crate::providers::claude::event_reader::tail(&path, 200).unwrap()
}

fn layout(event: &Event, width: u16) -> EventDetailLayout {
    event_detail_layout(event, width, Path::new("/workspace"), DateTime::UNIX_EPOCH)
}

fn body_parts(event: &Event) -> Vec<super::super::tool_detail::Part> {
    super::super::tool_detail::event_detail_content(event, String::new(), Style::new().dim())
        .sections
        .into_iter()
        .flat_map(|section| section.groups)
        .flat_map(|group| group.parts)
        .collect()
}

#[test]
fn advisor_list_row_uses_catalog_model_and_hides_encrypted_payload() {
    use crate::providers::Provider;
    let fixture = include_str!("../../tests/fixtures/claude-advisor.jsonl");
    let transcript = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(transcript.path(), fixture.lines().next().unwrap()).unwrap();
    let pending = crate::providers::claude::event_reader::tail_session(transcript.path(), 20, true)
        .unwrap()
        .remove(0);
    std::fs::write(transcript.path(), fixture).unwrap();
    let mut parsed =
        crate::providers::claude::event_reader::tail_session(transcript.path(), 20, true).unwrap();
    let event = parsed.remove(0);
    let config = tempfile::tempdir().unwrap();
    crate::providers::claude::write_fixture_cache(config.path());
    let project = config.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_str!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(config.path().into());
    let mut session = provider.refresh(1).sessions.remove(0);
    let mut following: serde_json::Value = serde_json::from_str(
        include_str!("../../tests/fixtures/claude-tool-details.jsonl")
            .lines()
            .nth(6)
            .unwrap(),
    )
    .unwrap();
    let end = event.ended_at.unwrap();
    let next = end + chrono::Duration::seconds(2);
    following["timestamp"] = next.to_rfc3339().into();
    std::fs::write(transcript.path(), format!("{fixture}{following}\n")).unwrap();
    let parsed =
        crate::providers::claude::event_reader::tail_session(transcript.path(), 20, true).unwrap();
    session.last_record_at = Some(next);
    assert_eq!(
        row_time(&pending, &session, true),
        RowTime::Running(pending.started_at)
    );
    assert_eq!(
        sort_duration(&pending, &session, next, true),
        Some(next - pending.started_at)
    );
    let time = event_time(&event, &session, false, next, EventSort::Time);
    assert_eq!(time, "4m 45s");
    assert_eq!(
        event_time(&parsed[1], &session, true, next, EventSort::Time),
        "2s"
    );
    let mut view = View::new(Grouping::Project, false);
    view.set_input_choices(HashMap::from([(
        ProviderId::Claude,
        provider.input_choices(),
    )]));
    let display = view.display_event(&event);
    assert_eq!(display.body, "advisor · Fable 5.1");
    assert_eq!(display.calls[0].summary, "advisor · Fable 5.1");
    assert_eq!(display.calls[0].title, "advisor · Fable 5.1");
    let area = Rect::new(0, 0, 100, 1);
    let mut buffer = Buffer::empty(area);
    render_event(
        &display,
        &time,
        true,
        area,
        &mut buffer,
        None,
        Path::new("/workspace"),
    );
    let row = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(row.contains("advisor · Fable 5.1"), "{row}");
    assert!(row.contains("4m 45s"), "{row}");
    assert!(!view.event_detail_text(&event).contains("encrypted_content"));
    assert!(view.event_detail_text(&event).contains("(encrypted)"));
    assert_eq!(event.body, "advisor");
    let advisor_x = row.find("advisor").unwrap() as u16;
    assert_eq!(
        buffer[(advisor_x, 0)].fg,
        style::event_styles(&Kind::Assistant).1.fg.unwrap()
    );
    assert_eq!(event.kind, Kind::Assistant);
    assert_eq!(
        sort_duration(&event, &session, next, false),
        Some(end - event.started_at)
    );
    assert_eq!(
        event_time(&event, &session, true, next, EventSort::Time),
        "4m 45s"
    );
    let tool = &parsed[1];
    let mut buffer = Buffer::empty(area);
    render_event(
        tool,
        "2s",
        true,
        area,
        &mut buffer,
        None,
        Path::new("/workspace"),
    );
    let first = buffer
        .content
        .iter()
        .find(|cell| !cell.symbol().trim().is_empty())
        .unwrap();
    assert_eq!(first.fg, style::event_styles(&Kind::Tool).1.fg.unwrap());
    view.set_sessions(vec![session], false);
    view.set_event(parsed);
    for (filter, expected) in [
        (crate::event::EventFilter::Message, Kind::Assistant),
        (crate::event::EventFilter::Tool, Kind::Tool),
    ] {
        view.event_list.filter = filter;
        let shown: Vec<_> = view
            .shown_events()
            .map(|(_, event)| event.kind.clone())
            .collect();
        assert_eq!(shown, vec![expected]);
    }
}

#[test]
fn codex_agent_messages_show_routes_and_payload() {
    use crate::providers::Provider;
    let dir = tempfile::tempdir().unwrap();
    let provider =
        crate::providers::codex::CodexProvider::new(dir.path().into(), dir.path().join("manager"));
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_str!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut claude = crate::providers::claude::ClaudeProvider::new(dir.path().into());
    let mut session = claude.refresh(1).sessions.remove(0);
    let path = dir.path().join("messages.jsonl");
    session.transcript = Some(path.clone());
    let plaintext = include_str!("../../tests/fixtures/codex-agent-messages/final-answer.jsonl");
    let encrypted = include_str!("../../tests/fixtures/codex-agent-messages/new-task.jsonl");
    let mut message: serde_json::Value = serde_json::from_str(plaintext).unwrap();
    message["payload"]["content"][0]["text"] = serde_json::json!(
        "Message Type: MESSAGE\nTask name: /root\nSender: /root/demo_sub\nPayload:\n**完成** [report](https://example.com/report)\n\nPayload:\n保留正文"
    );
    let message = serde_json::to_string(&message).unwrap() + "\n";
    for (record, title, body) in [
        (plaintext, "demo_sub → 演示完成", "演示完成"),
        (encrypted, "demo_sub ← new task", "(encrypted)"),
        (
            message.as_str(),
            "demo_sub → message",
            "**完成** [report](https://example.com/report)\n\nPayload:\n保留正文",
        ),
    ] {
        std::fs::write(&path, record).unwrap();
        let event = provider.event_panel(&session, 10, false).unwrap().remove(0);
        assert_eq!(event.body, body);
        assert_eq!(event.summary(), title);
        assert_eq!(super::body_line(&event, 100, dir.path()).to_string(), title);
        let parts = body_parts(&event);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].source, body);
        assert!(parts[0].markdown());
        assert_eq!(super::open_text(&event), body);
        assert!(
            layout(&event, 120)
                .sources
                .iter()
                .any(|source| source.text.as_ref() == body)
        );
        if record == message {
            let drawn = layout(&event, 120);
            assert!(
                drawn
                    .lines
                    .iter()
                    .any(|line| line.to_string().contains("完成 report"))
            );
            assert!(drawn.lines.iter().flat_map(|line| &line.spans).any(|span| {
                span.content == "完成"
                    && span
                        .style
                        .add_modifier
                        .contains(ratatui::style::Modifier::BOLD)
            }));
            assert!(
                drawn
                    .links
                    .iter()
                    .flatten()
                    .any(|link| link.target == Path::new("https://example.com/report"))
            );
        }
    }
}

fn assert_reading_column(layout: &EventDetailLayout, segment: usize) {
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
fn prose_body_kinds_use_the_event_column_width() {
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
    assert_eq!(event.calls[0].title, "tool-calls");
    assert_eq!(event.calls[0].detail_left(), "codex-manager start");
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
        crate::event::Section::Structured {
            label: "input".into(),
            text: r#"{"threadId":"abc-123"}"#.into(),
        },
        crate::event::Section::Prose {
            text: "Continue reading this message. ".repeat(20),
            markdown: true,
        },
    ];
    event.calls[0].title = "Send".into();
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
    event.calls[0].input = vec![crate::event::Section::Prose {
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
                        crate::event::Section::Prose { text, .. } => Some(text.as_str()),
                        crate::event::Section::Markdown { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .unwrap();
                let segment = drawn
                    .sources
                    .iter()
                    // A section break appends a newline to the section's last source.
                    .position(|s| s.text.trim_end() == input)
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
    event.calls[0].output = Some(vec![crate::event::Section::Text {
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
    use crate::event::Section;
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
        let fields = Section::Structured {
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
    use crate::event::Section;
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
        Section::Structured {
            label: "output".into(),
            text: serde_json::json!({"prompt": text}).to_string(),
        },
    ]);
    for part in body_parts(&event) {
        for width in [200, 120] {
            let lines = part.lines(width, Style::default(), DateTime::UNIX_EPOCH);
            let json = super::super::structured_text::parse(&part.source)
                .is_some_and(|document| document.syntax() == "json");
            if json {
                assert!(
                    lines.len() > part.source.lines().count(),
                    "long JSON values wrap at {width}"
                );
                assert!(lines.iter().all(|line| line.width() <= width));
            } else if part.markdown() {
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
    let expected = [event.body.as_str()];
    let actual = body_parts(&event);
    assert_eq!(
        actual.iter().map(|p| &p.source).collect::<Vec<_>>(),
        expected
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
    let detail =
        super::super::tool_detail::event_detail_content(&event, String::new(), Style::new().dim());
    let title = &detail.sections[0].title;
    assert_eq!(title.source(), event.calls[0].title.clone());
    let description = event.calls[0].title.clone();
    assert_eq!(title.left, Some(event.calls[0].detail_left()));
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

fn codex_fixture() -> (
    tempfile::TempDir,
    crate::providers::codex::CodexProvider,
    crate::model::Session,
) {
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
    let session = provider.refresh(1).sessions.remove(0);
    (dir, provider, session)
}

fn codex_events(fixture: &str) -> Vec<Event> {
    let (_dir, provider, session) = codex_fixture();
    codex_fixture_events(&provider, &session, fixture)
}

fn codex_fixture_events(
    provider: &crate::providers::codex::CodexProvider,
    session: &crate::model::Session,
    fixture: &str,
) -> Vec<Event> {
    use crate::providers::Provider;
    let mut session = session.clone();
    session.transcript = Some(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture),
    );
    provider.event_panel(&session, usize::MAX, false).unwrap()
}

/// Every input/output group follows the panel midline and preserves its own indentation.
fn assert_centered_blocks(event: &Event, width: u16) {
    let detail =
        super::super::tool_detail::event_detail_content(event, String::new(), Style::new().dim());
    assert_centered_event_detail(event, width, &detail);
}

fn assert_centered_event_detail(
    event: &Event,
    width: u16,
    detail: &super::super::event_detail::EventDetailContent,
) {
    let drawn = layout(event, width);
    let cap = usize::from(drawn.columns.text.width);
    let mut id = 0;
    for section in &detail.sections {
        id += 1;
        for group in &section.groups {
            let start = id;
            id += group.parts.len();
            let widest = group
                .parts
                .iter()
                .flat_map(|part| {
                    part.lines_in(
                        usize::from(if part.reading_column() {
                            drawn.columns.reading_width.min(drawn.columns.text.width)
                        } else {
                            drawn.columns.text.width
                        }),
                        Style::default(),
                        DateTime::UNIX_EPOCH,
                        Path::new("/workspace"),
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
fn diff_block_centers_like_code() {
    let event = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"))
        .into_iter()
        .find(|event| event.calls.iter().any(|call| call.name == "Edit"))
        .unwrap();
    let source = event.calls[0]
        .input
        .iter()
        .chain(event.calls[0].output.iter().flatten())
        .find_map(|section| match section {
            crate::event::Section::Diff { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap();
    assert!(source.contains('装'));
    let mut code = event.clone();
    let call = &mut code.calls[0];
    for section in call
        .input
        .iter_mut()
        .chain(call.output.iter_mut().flatten())
    {
        if let crate::event::Section::Diff { label, lang, text } = section {
            *section = crate::event::Section::Code {
                label: label.clone(),
                lang: lang.clone(),
                text: text.clone(),
            };
        }
    }
    for light in [false, true] {
        super::super::highlight::use_theme(light, None).unwrap();
        for width in [41, 120, 200] {
            let diff = layout(&event, width);
            let code_layout = layout(&code, width);
            let marker = "+    fn wrapped_path_ends";
            let diff_line = diff
                .lines
                .iter()
                .find(|line| line.to_string().contains(marker))
                .unwrap();
            let code_line = code_layout
                .lines
                .iter()
                .find(|line| line.to_string().contains(marker))
                .unwrap();
            let start = diff_line.to_string().find(marker).unwrap();
            assert_eq!(
                start,
                code_line.to_string().find(marker).unwrap(),
                "width={width}"
            );
            if width == 200 {
                assert!(start > 0);
            }
            assert_eq!(
                diff_line.to_string().trim_end(),
                code_line.to_string().trim_end()
            );
        }
    }
}

#[test]
fn subagent_activity_blocks_are_centered() {
    let events = codex_events("codex-collaboration/spawn_agent.jsonl");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].calls[0].name, "spawn_agent");
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
    let (_dir, provider, session) = codex_fixture();
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
        entries.extend(codex_fixture_events(&provider, &session, fixture));
    }
    assert!(entries.len() > 30);
    for event in entries {
        let detail = super::super::tool_detail::event_detail_content(
            &event,
            String::new(),
            Style::new().dim(),
        );
        for width in [200, 151, 120] {
            assert_centered_event_detail(&event, width, &detail);
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
            snapshot.push_str(&format!("{} — {width} columns\n", event.summary()));
            for line in layout(&event, width).lines {
                snapshot.push_str(line.to_string().trim_end());
                snapshot.push('\n');
            }
        }
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/event_detail_blocks_centered.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
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
    assert_eq!(Some(call.title.as_str()), Some("attach-probe"));
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
    let event = super::event_markdown::notification_events(dir.path(), Some(output)).remove(0);
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
    assert_eq!(
        event.calls[0]
            .output
            .as_ref()
            .unwrap()
            .iter()
            .map(|section| match section {
                crate::event::Section::OutputFile(path) =>
                    crate::event::Section::read_output_file(path),
                section => section.clone(),
            })
            .collect::<Vec<_>>(),
        vec![crate::event::Section::Markdown {
            label: String::new(),
            text: message.into(),
        }]
    );
    for width in [200, 90] {
        let drawn = layout(&event, width);
        assert!(
            drawn
                .sources
                .iter()
                .any(|source| source.text.contains(command_line))
        );
        assert!(
            !drawn
                .sources
                .iter()
                .any(|source| source.text.contains("lastMessage"))
        );
        let expected = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
            Line::from(command_line.replace("/workspace/", "")),
            drawn.columns.text.width.into(),
        )
        .to_string();
        assert!(
            drawn
                .lines
                .iter()
                .any(|line| line.to_string().trim_start() == expected),
            "Markdown code block stays preformatted at {width}"
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
    let event = super::event_markdown::notification_events(dir.path(), Some(output)).remove(0);
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
        crate::event::Section::Text {
            label: "stdout".into(),
            text: command_line.into(),
        },
        crate::event::Section::Code {
            label: "output".into(),
            lang: "text".into(),
            text: command_line.into(),
        },
        crate::event::Section::Markdown {
            label: "message".into(),
            text: format!("Result\n\n```text\n{command_line}\n```"),
        },
    ] {
        if matches!(section, crate::event::Section::Text { .. }) == code {
            continue;
        }
        let mut event = event.clone();
        event.calls[0].output = Some(vec![section]);
        for width in [200, 90] {
            let drawn = layout(&event, width);
            let expected = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
                Line::from(command_line.replace("/workspace/", "")),
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
    let event = super::event_markdown::notification_events(
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
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
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
            .filter_map(|call| call.duration.map(format_event_duration))
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
    for tool in ["spawn_agent", "send_message", "followup_task"] {
        entries.extend(codex_events(&format!("codex-collaboration/{tool}.jsonl")));
    }
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
                    | "followup_task"
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
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/restored_tool_event_detail.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

fn list_row_records() -> Vec<serde_json::Value> {
    include_str!("../../tests/fixtures/tool-list/claude.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn list_row_fixture(records: &[serde_json::Value]) -> String {
    records.iter().map(|record| format!("{record}\n")).collect()
}

#[test]
fn tool_list_rows_keep_names_subjects_and_detail_titles() {
    for event in events(&list_row_fixture(&list_row_records()[..3])) {
        let call = &event.calls[0];
        assert_eq!(
            event.body,
            format!("{} {}", call.detail_left(), call.title).trim()
        );
        assert_eq!(
            super::body_line(&event, 200, Path::new("/workspace")).to_string(),
            format!(
                "{} {}",
                call.detail_left(),
                call.title.trim_start_matches("/workspace/")
            )
        );
        let title = layout(&event, 200).lines[0].to_string();
        assert!(title.starts_with(&call.detail_left()));
        assert!(title.contains(call.title.trim_start_matches("/workspace/")));
    }
}

#[test]
fn tool_list_rows_group_members_and_preserve_relative_paths() {
    let mut records = list_row_records();
    let edits = &mut records[3..];
    for (record, path) in edits
        .iter_mut()
        .zip(["/workspace/src/a.rs", "/workspace/src/b.rs"])
    {
        record["message"]["content"][0]["input"]["file_path"] = path.into();
    }
    let merged = events(&list_row_fixture(edits)).remove(0);
    assert_eq!(
        merged.body,
        "Edit {/workspace/src/a.rs, /workspace/src/b.rs}"
    );
    assert_eq!(
        super::body_line(&merged, 200, Path::new("/workspace")).to_string(),
        "Edit {src/a.rs, src/b.rs}"
    );
    assert_eq!(
        merged
            .paths
            .iter()
            .map(|range| &merged.body[range.clone()])
            .collect::<Vec<_>>(),
        ["/workspace/src/a.rs", "/workspace/src/b.rs"]
    );
    let batch = edits[0]["message"]["id"].clone();
    let mut read = records[0].clone();
    read["message"]["id"] = batch;
    read["message"]["content"][0]["input"]["file_path"] = "/workspace/src/c.rs".into();
    records.push(read);
    let mixed = events(&list_row_fixture(&records[3..])).remove(0);
    assert_eq!(
        super::body_line(&mixed, 200, Path::new("/workspace")).to_string(),
        "Edit {src/a.rs, src/b.rs}, Read src/c.rs"
    );
}

#[test]
fn tool_list_rows_keep_shell_display_names_and_grouped_subjects() {
    let event = codex_events("codex-inner/git-chain.jsonl").remove(0);
    assert_eq!(event.body, "git {status, diff}, sed");
    assert_eq!(event.calls[0].detail_left(), "Exec command");
    assert_eq!(event.calls[0].title, "git {status, diff}, sed");
    let mut record = list_row_records().remove(0);
    let block = &mut record["message"]["content"][0];
    block["name"] = "Bash".into();
    block["input"] = serde_json::json!({"command":"git status --short && git diff --name-only; sed -n '1,3p' file"});
    let event = events(&list_row_fixture(&[record.clone()])).remove(0);
    assert_eq!(event.body, "git {status, diff}, sed");
    record["message"]["content"][0]["input"]["description"] = "Inspect changes".into();
    assert_eq!(
        events(&list_row_fixture(&[record.clone()]))[0].body,
        "Inspect changes"
    );
    record["message"]["content"][0]["name"] = "Read".into();
    assert_eq!(
        events(&list_row_fixture(&[record.clone()]))[0].body,
        "Inspect changes"
    );
    record["message"]["content"][0]["input"] = serde_json::json!({});
    assert_eq!(events(&list_row_fixture(&[record]))[0].body, "Read");
}

#[test]
fn tool_list_rows_choose_descriptions_by_data() {
    let records: Vec<serde_json::Value> =
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    for event in events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl")) {
        for call in event.calls {
            let input = records
                .iter()
                .flat_map(|record| record["message"]["content"].as_array().unwrap())
                .find(|block| block["id"] == call.id)
                .unwrap()["input"]
                .clone();
            let expected = input["description"]
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if call.title.is_empty() {
                        crate::providers::tool::display_tool_name(&call.name)
                    } else {
                        format!("{} {}", call.detail_left(), call.title)
                    }
                });
            let mut expected = call
                .agent_message
                .as_ref()
                .map(|message| message.summary(1))
                .unwrap_or(expected);
            if let Some(code) = call.exit_code.filter(|code| *code != 0) {
                expected.push_str(&format!(" · {code}"));
            }
            assert_eq!(call.summary, expected, "{}", call.name);
        }
    }
}

#[test]
fn tool_list_rows_keep_codex_native_file_paths() {
    let entries = codex_events("tool-list/codex.jsonl");
    assert_eq!(entries.len(), 3);
    for (event, expected) in entries.into_iter().zip([
        "sed",
        "Apply patch /workspace/agents/src/log.rs · /workspace/agents/src/providers/codex/rollout.rs",
        "Apply patch /private/tmp/agents-http-settings/report.md",
    ]) {
        assert_eq!(event.body, expected);
        for range in &event.paths {
            assert!(event.body[range.clone()].starts_with('/'));
        }
    }
}

#[test]
fn tool_list_rows_group_home_paths_through_existing_shortening() {
    let home = std::env::var("HOME").unwrap();
    let mut records = list_row_records();
    for (record, path) in records[3..].iter_mut().zip(["src/a.rs", "src/b.rs"]) {
        record["message"]["content"][0]["input"]["file_path"] = format!("{home}/{path}").into();
    }
    let event = events(&list_row_fixture(&records[3..])).remove(0);
    assert_eq!(
        super::body_line(&event, 200, Path::new("/workspace")).to_string(),
        "Edit {~/src/a.rs, ~/src/b.rs}"
    );
    assert_eq!(
        super::body_line(&event, 200, Path::new(&home)).to_string(),
        "Edit {src/a.rs, src/b.rs}"
    );
}

#[test]
fn codex_collaboration_message_sections_snapshot() {
    let mut snapshot = String::new();
    for fixture in [
        "codex-agent-messages/new-task.jsonl",
        "codex-agent-messages/final-answer.jsonl",
        "codex-agent-messages/sequence.jsonl",
        "codex-collaboration/spawn_agent.jsonl",
        "codex-collaboration/send_message.jsonl",
        "codex-collaboration/followup_task.jsonl",
    ] {
        for event in codex_events(fixture) {
            let panel = Rect::new(0, 0, 120, 35);
            let mut buffer = Buffer::empty(panel);
            let view = View::new(Grouping::Project, false);
            view.render_event_detail_content(
                &event,
                "09:54:58",
                panel,
                &mut buffer,
                Path::new("/workspace"),
                DateTime::UNIX_EPOCH,
            );
            snapshot.push_str(&format!("{fixture}\n"));
            let rows: Vec<_> = buffer
                .content
                .chunks(120)
                .map(|row| {
                    row.iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_owned()
                })
                .collect();
            snapshot.push_str(rows.join("\n").trim_end());
            snapshot.push_str("\n\n");
            let columns = event_detail_columns(panel, panel);
            let viewport = view.viewport.borrow();
            let layout = viewport.event_detail_layout.as_ref().unwrap();
            for (row, time) in &layout.times {
                let y = EVENT_TOP + *row as u16;
                let start = columns.time.right() - time.width() as u16;
                assert_eq!(
                    (start..columns.time.right())
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>(),
                    time.content.as_ref()
                );
            }
        }
    }
    let snapshot = format!("{}\n", snapshot.trim_end());
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ui_snapshots/codex_collaboration_messages.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn codex_perspective_list_snapshot() {
    let mut snapshot = String::new();
    for session in ["root", "alpha", "beta", "alpha_kid"] {
        snapshot.push_str(&format!("{session}\n"));
        for event in codex_events(&format!("codex-perspective/{session}.jsonl")) {
            snapshot.push_str(&super::body_line(&event, 160, Path::new("/workspace")).to_string());
            snapshot.push('\n');
        }
        snapshot.push('\n');
    }
    let snapshot = format!("{}\n", snapshot.trim_end());
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/codex_perspective_list.txt");
    if std::env::var_os("UPDATE_EVENT_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn background_notification_titles_show_only_exit_numbers_on_the_left() {
    let fixture = include_str!("../../tests/fixtures/claude-background-notifications.jsonl");
    let events = events(fixture);
    for (event, code) in events
        .iter()
        .zip([None, Some(1), Some(-1), Some(144), None, None])
    {
        let detail =
            super::super::tool_detail::event_detail_content(event, String::new(), Style::new());
        assert_eq!(
            detail.sections[0].title.left,
            code.map(|code| code.to_string())
        );
        assert_eq!(detail.sections[0].title.exit_code, code);
        assert_eq!(detail.sections[0].title.title, event.body);
        let line = super::body_line(event, 300, Path::new("/workspace"));
        assert_eq!(line.to_string(), event.body);
        if let Some(code) = code {
            let columns = super::super::event_detail::event_detail_columns(
                Rect::new(0, 0, 300, 20),
                Rect::new(0, 0, 300, 20),
            );
            let (header, _) = detail.sections[0].title.line(columns);
            assert!(
                header
                    .spans
                    .iter()
                    .any(|span| span.content == code.to_string()
                        && span.style
                            == super::super::style::event_styles(&Kind::Error).1.not_dim())
            );
            assert!(
                line.spans
                    .iter()
                    .any(|span| span.content == code.to_string()
                        && span.style.fg == super::super::style::event_styles(&Kind::Error).0.fg)
            );
        }
    }
}

#[test]
fn diff_changed_rows_fill_every_panel_column_in_both_themes() {
    let mut cases = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
    cases.extend(codex_events("codex-file-changes.jsonl"));
    let cases: Vec<_> = cases
        .into_iter()
        .filter(|event| {
            event.calls.iter().any(|call| {
                call.input
                    .iter()
                    .chain(call.output.iter().flatten())
                    .any(|section| matches!(section, crate::event::Section::Diff { .. }))
            })
        })
        .collect();
    let mut markers = std::collections::HashSet::new();
    for light in [false, true] {
        super::super::highlight::use_theme(light, None).unwrap();
        for event in &cases {
            let sources: Vec<_> = event
                .calls
                .iter()
                .flat_map(|call| call.input.iter().chain(call.output.iter().flatten()))
                .filter_map(|section| match section {
                    crate::event::Section::Diff { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            for width in [16, 41, 120, 200] {
                let panel = Rect::new(3, 2, width, 250);
                let mut buffer = Buffer::empty(Rect::new(0, 0, width + 6, 254));
                let view = View::new(Grouping::Project, false);
                view.render_event_detail_content(
                    event,
                    "12:34:56",
                    super::super::event_detail::EventDetailArea {
                        panel,
                        content: panel
                            .inner(Margin::new(super::super::render::HORIZONTAL_PADDING, 0)),
                    },
                    &mut buffer,
                    Path::new("/workspace"),
                    DateTime::UNIX_EPOCH,
                );
                let viewport = view.viewport.borrow();
                let layout = viewport.event_detail_layout.as_ref().unwrap();
                for (row, source) in layout.visible.iter().enumerate() {
                    let segment = layout.rows[*source].0;
                    if !sources.contains(&layout.sources[segment].text.as_ref()) {
                        continue;
                    }
                    let text = layout.lines[*source].to_string();
                    let marker = text.trim_start().chars().next();
                    let changed = matches!(marker, Some('+' | '-'));
                    markers.extend(marker);
                    let y = panel.y + EVENT_TOP + row as u16;
                    let expected = if changed {
                        let x = layout.columns.text.x
                            + text.chars().position(|c| c != ' ').unwrap() as u16;
                        let background = buffer[(x, y)].bg;
                        assert_ne!(background, ratatui::style::Color::Reset);
                        background
                    } else {
                        ratatui::style::Color::Reset
                    };
                    for x in panel.x..panel.right() {
                        assert_eq!(
                            buffer[(x, y)].bg,
                            expected,
                            "light={light}, width={width}, x={x}, text={text}"
                        );
                    }
                    assert_eq!(buffer[(panel.x - 1, y)].bg, ratatui::style::Color::Reset);
                    assert_eq!(buffer[(panel.right(), y)].bg, ratatui::style::Color::Reset);
                }
            }
        }
    }
    assert!(markers.contains(&'+') && markers.contains(&'-') && markers.contains(&'@'));
}

#[test]
fn diff_backgrounds_reach_terminal_edges_without_moving_text() {
    use crate::providers::Provider;
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_str!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().into());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.event_session = view.selected_id().cloned();
    let now = DateTime::UNIX_EPOCH;
    let mut cases = events(include_str!("../../tests/fixtures/claude-tool-calls.jsonl"));
    cases.extend(codex_events("codex-file-changes.jsonl"));
    let mut markers = std::collections::HashSet::new();
    for light in [false, true] {
        super::super::highlight::use_theme(light, None).unwrap();
        for event in &cases {
            view.set_event(vec![event.clone()]);
            view.select_fixture_event(Some(event.clone()));
            for width in [41, 120, 200] {
                let area = Rect::new(0, 0, width, 250);
                let mut buffer = Buffer::empty(area);
                view.render_at(area, &mut buffer, now);
                let viewport = view.viewport.borrow();
                let layout = viewport.event_detail_layout.as_ref().unwrap();
                let panel = view.center_layout(area).event_detail;
                // Compare both renderers at the same inset content rectangle.
                let reference = View::new(Grouping::Project, false);
                let mut before = Buffer::empty(area);
                reference.render_event_detail_content(
                    event,
                    "12:34:56",
                    viewport.event_detail,
                    &mut before,
                    &view.selected_row().unwrap().cwd,
                    now,
                );
                for (row, &source) in layout.visible.iter().enumerate() {
                    let line = &layout.lines[source];
                    let marker = line.to_string().trim_start().chars().next();
                    let changed = line.style.bg.is_some();
                    if changed {
                        markers.extend(marker);
                    }
                    let y = panel.y + EVENT_TOP + row as u16;
                    let expected = line.style.bg.unwrap_or(ratatui::style::Color::Reset);
                    for x in panel.x..panel.right() {
                        if changed
                            || x < viewport.event_detail.x
                            || x >= viewport.event_detail.right()
                        {
                            assert_eq!(
                                buffer[(x, y)].bg,
                                expected,
                                "light={light}, width={width}, x={x}, row={row}, marker={marker:?}"
                            );
                        }
                        // Padding changes only the background, preserving the centered text.
                        if !layout.times.iter().any(|(time_row, _)| *time_row == source) {
                            assert_eq!(
                                cell_text(buffer[(x, y)].symbol()),
                                cell_text(before[(x, y)].symbol()),
                                "text moved at ({x}, {y}), width={width}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(markers.contains(&'+') && markers.contains(&'-'));
}
