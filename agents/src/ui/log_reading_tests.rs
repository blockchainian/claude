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
    let width = if wide { layout.width / 2 } else { layout.width };
    let left = if wide { (layout.width - width) / 2 } else { 0 };
    for (_, cells) in rows {
        assert!(cells.first().unwrap().0.start >= left);
        assert!(
            cells.last().unwrap().0.end <= left + width,
            "body exceeds reading column: {:?}",
            cells.last()
        );
    }
    if wide {
        assert_eq!(
            layout
                .rows
                .iter()
                .find(|(id, cells)| *id == segment && !cells.is_empty())
                .unwrap()
                .1[0]
                .0
                .start,
            left
        );
    }
}

#[test]
fn prose_body_kinds_use_reading_column_only_in_wide_mode() {
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
fn generic_input_fields_promote_titles_and_prose_without_changing_outputs() {
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
        super::super::tool_detail::input_title(&event.calls[0]).as_deref(),
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
        super::super::tool_detail::input_title(&event.calls[0]).as_deref(),
        Some("thread abc-123")
    );
    let drawn = layout(&event, 200);
    assert_reading_column(&drawn, 0, true);
    assert!(
        !drawn
            .sources
            .iter()
            .any(|s| s.text.starts_with("threadId:"))
    );
}

#[test]
fn question_and_agent_inputs_are_prose_but_tool_outputs_keep_full_width() {
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
        .find(|e| e.calls.iter().any(|c| c.name == "Bash"))
        .unwrap();
    event.calls.truncate(1);
    event.calls[0].output = Some(vec![crate::log::Section::Text {
        label: "stdout".into(),
        text: "tool output with many words ".repeat(20),
    }]);
    for width in [200, 120] {
        let drawn = layout(&event, width);
        let output = drawn
            .sources
            .iter()
            .position(|s| s.text.starts_with("tool output"))
            .unwrap();
        assert!(
            drawn
                .rows
                .iter()
                .filter(|(id, _)| *id == output)
                .any(|(_, cells)| cells
                    .last()
                    .is_some_and(|(range, _)| range.end > width * 3 / 4))
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
            super::super::tool_detail::input_title(&event.calls[0]).as_deref(),
            Some("Readable task · session abc")
        );
        let drawn = layout(&event, 200);
        assert_eq!(drawn.sources[0].text.as_ref(), "cwd: /workspace");
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
            super::super::tool_detail::input_title(&event.calls[0]),
            None
        );
        assert_eq!(open_parts(&event)[0].source, format!("name: {value}"));
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
        assert_eq!(part.column(200, true), None);
        assert_eq!(part.column(120, false), None);
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
        assert_eq!(part.column(200, true), Some((100, 50)));
    }
}
