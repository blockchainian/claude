use super::super::task_ui::EventDetailMode;
use super::*;

fn transcript(view: &mut View) {
    view.set_active_panel(Panel::EventDetail);
    view.handle_key(KeyCode::Char(' ').into());
}

#[test]
fn transcript_space_switches_only_in_details_outside_search_and_terminal() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 160, 45, now);
    view.set_active_panel(Panel::EventList);
    view.handle_key(KeyCode::Char(' ').into());
    assert_eq!(view.event_detail.mode, EventDetailMode::Event);
    transcript(&mut view);
    assert_eq!(view.event_detail.mode, EventDetailMode::Transcript);
    for key in [KeyCode::Left, KeyCode::Right] {
        view.handle_key(key.into());
        assert_eq!(view.event_detail.mode, EventDetailMode::Transcript);
    }
    view.handle_key(KeyCode::Char(' ').into());
    assert_eq!(view.event_detail.mode, EventDetailMode::Event);
    transcript(&mut view);
    view.handle_key(KeyCode::Char('/').into());
    view.handle_key(KeyCode::Char(' ').into());
    assert_eq!(view.event_detail.mode, EventDetailMode::Transcript);
    assert_eq!(view.event_detail.search.input.text(), " ");
    view.handle_key(KeyCode::Esc.into());
    let screen = std::sync::Arc::new(std::sync::Mutex::new(
        crate::session_terminal::TerminalScreen::new(10, 40),
    ));
    view.show_terminal(session_key("log"), screen);
    for (key, bytes) in [
        (KeyCode::Char(' '), b" ".as_slice()),
        (KeyCode::Left, b"\x1b[D".as_slice()),
        (KeyCode::Right, b"\x1b[C".as_slice()),
    ] {
        assert_eq!(
            view.handle_key(key.into()),
            Some(Action::TerminalInput(bytes.to_vec()))
        );
    }

    assert_eq!(view.event_detail.mode, EventDetailMode::Terminal);
    assert_eq!(view.event_detail.return_mode, EventDetailMode::Transcript);
}

#[test]
fn transcript_multiline_json_matches_event_document_lines() {
    let raw = include_str!("../../tests/fixtures/claude-write/python.jsonl")
        .lines()
        .next()
        .unwrap();
    assert!(raw.contains(r"\n"));
    let content = super::super::tool_detail::transcript_content(
        &Ok(raw.into()),
        crate::providers::ProviderId::Claude,
        &crate::event::Kind::Assistant,
    );
    let (view, now) = event_fixture();
    let mut event = view.event_panel[0].clone();
    event.kind = crate::event::Kind::Assistant;
    event.body = raw.into();
    event.detail_body = None;
    let expected =
        super::super::tool_detail::event_detail_content(&event, String::new(), Style::default());
    let draw = |content: &super::super::event_detail::EventDetailContent, width| {
        content.sections[0].groups[0]
            .parts
            .iter()
            .flat_map(|part| part.lines(width, Style::default(), now))
            .collect::<Vec<_>>()
    };
    assert!(
        draw(&expected, 80)
            .iter()
            .any(|line| line.to_string().contains("import json, sys"))
    );
    for width in [20, 80, 120, 200] {
        assert_eq!(
            draw(&content, width),
            draw(&expected, width),
            "width {width}"
        );
    }
}

#[test]
fn transcript_right_click_terminal_returns_to_previous_submode() {
    for toggle in [false, true] {
        let (mut view, now) = event_fixture();
        event_screen(&view, 160, 45, now);
        view.set_active_panel(Panel::EventDetail);
        if toggle {
            view.handle_key(KeyCode::Char(' ').into());
        }
        let expected = if !toggle {
            EventDetailMode::Event
        } else {
            EventDetailMode::Transcript
        };
        let detail = view.viewport.borrow().event_detail;
        let click = mouse(
            MouseEventKind::Down(MouseButton::Right),
            detail.x + 2,
            detail.y + 2,
        );
        assert_eq!(view.handle_mouse(click), None);
        assert_eq!(
            view.handle_mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Right),
                ..click
            }),
            Some(Action::OpenTerminal(session_key("log")))
        );
        view.request_terminal();
        let screen = std::sync::Arc::new(std::sync::Mutex::new(
            crate::session_terminal::TerminalScreen::new(10, 40),
        ));
        view.show_terminal(session_key("log"), screen);
        event_screen(&view, 160, 45, now);
        assert_eq!(view.handle_mouse(click), None);
        assert_eq!(
            view.handle_mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Right),
                ..click
            }),
            Some(Action::CloseTerminal)
        );
        view.hide_terminal();
        assert_eq!(view.event_detail.mode, expected);
    }
}

#[test]
fn transcript_records_have_their_own_titles_and_pretty_json_in_file_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.jsonl");
    let claude = include_str!("../../tests/fixtures/claude-write/python.jsonl");
    let claude = claude.lines().next().unwrap();
    let codex = r#"{"timestamp":"2026-10-12T02:12:55.062Z","type":"response_item","payload":{"type":"custom_tool_call","call_id":"call_1","name":"apply_patch","input":"*** Begin Patch\n*** End Patch"}}"#;
    std::fs::write(&path, format!("{claude}\n{codex}\n")).unwrap();
    let (mut view, now) = event_fixture();
    view.rows[0].id.provider = crate::providers::ProviderId::Codex;
    view.event_session = Some(view.rows[0].id.clone());
    let mut event = view.event_panel[0].clone();
    let first = crate::event::SourceRecord {
        path: path.clone(),
        offset: 0,
    };
    let second = crate::event::SourceRecord {
        path: path.clone(),
        offset: claude.len() as u64 + 1,
    };
    event.source_records = vec![second.clone(), first, second];
    view.set_event(vec![event]);
    event_screen(&view, 240, 90, now);
    transcript(&mut view);
    event_screen(&view, 240, 90, now);
    let mut buf = Buffer::empty(Rect::new(0, 0, 240, 90));
    view.render_at(buf.area, &mut buf, now);
    let sources = view.viewport.borrow().event_text.sources.clone();
    assert_eq!(sources.len(), 4);
    for (index, (source, title)) in [(claude, "Assistant"), (codex, "Custom Tool Call")]
        .into_iter()
        .enumerate()
    {
        let value: serde_json::Value = serde_json::from_str(source).unwrap();
        let at = chrono::DateTime::parse_from_rfc3339(value["timestamp"].as_str().unwrap())
            .unwrap()
            .with_timezone(&Utc);
        let header = sources[index * 2].text.to_string();
        assert!(header.contains(title), "{header}");
        if index > 0 {
            view.handle_key(KeyCode::End.into());
        }
        let rendered = event_screen(&view, 240, 90, now);
        let drawn_header = rendered
            .lines()
            .find(|line| {
                line.contains(title)
                    && line
                        .trim_end_matches(['▕', ' '])
                        .ends_with(&super::super::formatting::clock_text(at))
            })
            .unwrap_or_else(|| panic!("missing record clock: {rendered}"));
        assert!(drawn_header.contains(title));
        let pretty = &sources[index * 2 + 1].text;
        assert!(pretty.contains('\n'));
        assert_eq!(
            pretty.trim_end(),
            super::super::tool_detail::Part::text(source.into(), false).source
        );
    }
    assert!(
        buf.content
            .iter()
            .any(|cell| cell.fg != ratatui::style::Color::Reset)
    );
    // Rendering and geometry changes must use the cached records, even if the file disappears.
    std::fs::remove_file(path).unwrap();
    assert!(event_screen(&view, 220, 80, now).contains("custom_tool_call"));
}

#[test]
fn transcript_without_source_records_has_an_empty_message() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 160, 45, now);
    transcript(&mut view);
    assert!(event_screen(&view, 160, 45, now).contains("No transcript records"));
}

#[test]
fn transcript_json_uses_shared_path_rendering() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.jsonl");
    let absolute = dir.path().join("source.rs").display().to_string();
    std::fs::write(
        &path,
        serde_json::json!({"type":"user", "path":absolute}).to_string(),
    )
    .unwrap();
    let (mut view, now) = event_fixture();
    view.rows[0].cwd = dir.path().to_owned();
    let mut event = view.event_panel[0].clone();
    event.source_records = vec![crate::event::SourceRecord { path, offset: 0 }];
    view.set_event(vec![event]);
    event_screen(&view, 240, 90, now);
    transcript(&mut view);
    let rendered = event_screen(&view, 240, 90, now);
    let content = super::super::tool_detail::transcript_content(
        &Ok(serde_json::json!({"type":"user", "path":absolute}).to_string()),
        crate::providers::ProviderId::Claude,
        &crate::event::Kind::Assistant,
    );
    let part = &content.sections[0].groups[0].parts[0];
    let expected = super::super::tool_detail::Part::text(
        serde_json::json!({"type":"user", "path":absolute}).to_string(),
        false,
    );
    assert_eq!(
        part.lines_in(100, Style::default(), now, dir.path()),
        expected.lines_in(100, Style::default(), now, dir.path())
    );
    assert!(rendered.contains("source.rs"), "{rendered}");
}

#[test]
fn transcript_refresh_reloads_rewritten_records_at_the_same_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.jsonl");
    std::fs::write(
        &path,
        r#"{"type":"user","message":{"role":"user","content":"before"}}"#,
    )
    .unwrap();
    let (mut view, now) = event_fixture();
    let mut event = view.event_panel[0].clone();
    event.source_records = vec![crate::event::SourceRecord {
        path: path.clone(),
        offset: 0,
    }];
    view.set_event(vec![event.clone()]);
    event_screen(&view, 180, 60, now);
    transcript(&mut view);
    assert!(event_screen(&view, 180, 60, now).contains("before"));
    std::fs::write(
        path,
        r#"{"type":"user","message":{"role":"user","content":"after"}}"#,
    )
    .unwrap();
    event.body = "after".into();
    view.set_event(vec![event]);
    assert!(event_screen(&view, 180, 60, now).contains("\"after\""));
}

#[test]
fn transcript_titles_use_provider_fields_and_leave_missing_metadata_empty() {
    use crate::event::Kind;
    use crate::providers::ProviderId;
    let raw = Ok(r#"{"type":"assistant","payload":{"type":"nested"}}"#.to_owned());
    let claude =
        super::super::tool_detail::transcript_content(&raw, ProviderId::Claude, &Kind::Assistant);
    assert_eq!(claude.sections[0].title.title, "Assistant");
    let mode = Ok(r#"{"type":"permission-mode"}"#.to_owned());
    let mode =
        super::super::tool_detail::transcript_content(&mode, ProviderId::Claude, &Kind::Assistant);
    assert_eq!(mode.sections[0].title.title, "Permission Mode");
    assert_eq!(claude.sections[0].title.left, None);
    for (raw, title) in [
        (
            r#"{"type":"session_meta","payload":{"cwd":"/tmp"}}"#,
            "Session Meta",
        ),
        ("{}", ""),
    ] {
        let content = super::super::tool_detail::transcript_content(
            &Ok(raw.into()),
            ProviderId::Codex,
            &Kind::Assistant,
        );
        assert_eq!(content.sections[0].title.title, title);
        assert_eq!(content.sections[0].title.left, None);
    }
}

#[test]
fn transcript_titles_match_the_event_title_with_the_record_time_in_the_time_column() {
    use crate::event::Kind;
    let raw = Ok(
        r#"{"timestamp":"2026-10-22T08:15:30Z","type":"response_item","payload":{"type":"message"}}"#
            .to_owned(),
    );
    let content = super::super::tool_detail::transcript_content(
        &raw,
        crate::providers::ProviderId::Codex,
        &Kind::Assistant,
    );
    let title = &content.sections[0].title;
    assert_eq!(title.left, None);
    assert_eq!(
        title.time,
        Some(super::super::formatting::clock_text(
            "2026-10-22T08:15:30Z".parse().unwrap()
        ))
    );
    assert_eq!(
        title.title_style,
        super::super::style::event_styles(&Kind::Assistant).0
    );
}

#[test]
fn transcript_long_json_lines_wrap_without_losing_text_or_syntax_styles() {
    let raw = include_str!("../../tests/fixtures/codex-raw-detail.jsonl")
        .lines()
        .nth(2)
        .unwrap();
    let content = super::super::tool_detail::transcript_content(
        &Ok(raw.to_owned()),
        crate::providers::ProviderId::Codex,
        &crate::event::Kind::Assistant,
    );
    let part = &content.sections[0].groups[0].parts[0];
    let lines = part.lines(33, Style::default(), Utc::now());
    assert!(lines.len() > 10);
    assert!(lines.iter().all(|line| line.width() <= 33));
    assert_eq!(
        lines.iter().map(ToString::to_string).collect::<String>(),
        part.source.replace('\n', "")
    );
    let styled_bytes = |lines: Vec<Line<'static>>| {
        lines
            .into_iter()
            .flat_map(|line| line.spans)
            .flat_map(|span| {
                span.content
                    .as_bytes()
                    .iter()
                    .copied()
                    .map(|byte| (byte, span.style))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        styled_bytes(lines),
        styled_bytes(
            super::super::tool_detail::Part::text(raw.into(), false).lines(
                usize::MAX,
                Style::default(),
                Utc::now()
            )
        )
    );
}

#[test]
fn transcript_records_scroll_as_one_complete_document_without_fold_markers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.jsonl");
    let records = include_str!("../../tests/fixtures/codex-raw-detail.jsonl");
    std::fs::write(&path, records).unwrap();
    let offsets = records
        .split_inclusive('\n')
        .scan(0u64, |offset, line| {
            let at = *offset;
            *offset += line.len() as u64;
            Some(at)
        })
        .skip(1);
    let (mut view, now) = event_fixture();
    view.rows[0].id.provider = crate::providers::ProviderId::Codex;
    view.event_session = Some(view.rows[0].id.clone());
    let mut event = view.event_panel[0].clone();
    event.source_records = offsets
        .map(|offset| crate::event::SourceRecord {
            path: path.clone(),
            offset,
        })
        .collect();
    view.set_event(vec![event]);
    event_screen(&view, 200, 50, now);
    transcript(&mut view);
    let first = event_screen(&view, 200, 50, now);
    let detail = view.viewport.borrow().event_text.area;
    let body = |screen: &str| {
        screen
            .lines()
            .skip(usize::from(detail.y))
            .take(usize::from(detail.height))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(!body(&first).contains('…'), "{first}");
    assert!(view.event_detail.scroll.borrow().state.hidden.is_empty());
    let expected = view
        .event_detail
        .transcript
        .as_ref()
        .unwrap()
        .text
        .as_ref()
        .unwrap()
        .clone();
    let mut seen = std::collections::BTreeSet::new();
    let mut rendered = first;
    loop {
        assert!(!body(&rendered).contains('…'), "{rendered}");
        let viewport = view.viewport.borrow();
        for row in &viewport.event_text.rows {
            for (_, bytes) in &row.cells {
                seen.extend((bytes.start..bytes.end).map(|byte| (row.segment, byte)));
            }
        }
        drop(viewport);
        let before = view.event_detail.scroll.borrow().offset;
        view.handle_key(KeyCode::PageDown.into());
        rendered = event_screen(&view, 200, 50, now);
        if view.event_detail.scroll.borrow().offset == before {
            break;
        }
    }
    let viewport = view.viewport.borrow();
    let sources = &viewport.event_text.sources;
    let json = sources
        .iter()
        .filter(|source| source.text.trim_start().starts_with('{'))
        .collect::<Vec<_>>();
    assert_eq!(json.len(), 2);
    for source in json {
        for (byte, value) in source.text.bytes().enumerate() {
            if value != b'\n' {
                assert!(
                    seen.contains(&(source.id, byte)),
                    "unreachable byte {byte} in {}",
                    source.text
                );
            }
        }
    }
    assert_eq!(
        sources[1].text.trim_end().to_owned() + "\n\n" + sources[3].text.trim_end(),
        expected
            .lines()
            .map(|line| super::super::tool_detail::Part::text(line.into(), false).source)
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}

#[test]
fn transcript_claude_titles_show_content_types_in_order_else_the_role() {
    use crate::providers::ProviderId;
    for (fixture, line, expected) in [
        (
            include_str!("../../tests/fixtures/claude-todos.jsonl"),
            0,
            "Tool Use",
        ),
        (
            include_str!("../../tests/fixtures/claude-subagent-model-history.jsonl"),
            1,
            "Thinking",
        ),
        (
            include_str!("../../tests/fixtures/claude-bg-metadata.jsonl"),
            3,
            "Text",
        ),
        (
            include_str!("../../tests/fixtures/claude-advisor.jsonl"),
            0,
            "Server Tool Use",
        ),
        (
            include_str!("../../tests/fixtures/claude-todos.jsonl"),
            1,
            "Tool Result",
        ),
        (
            include_str!("../../tests/fixtures/claude-equal-timestamps.jsonl"),
            0,
            "Text",
        ),
        (
            include_str!("../../tests/fixtures/claude-prompt-images.jsonl"),
            0,
            "Text, Image",
        ),
        (
            include_str!("../../tests/fixtures/claude-prompt-images.jsonl"),
            4,
            "Text, Image, Image",
        ),
        (
            include_str!("../../tests/fixtures/claude-prompt-images.jsonl"),
            7,
            "Text, Text",
        ),
        (
            include_str!("../../tests/fixtures/claude-bg-metadata.jsonl"),
            0,
            "User",
        ),
        (
            include_str!("../../tests/fixtures/claude-subagent-model-history.jsonl"),
            0,
            "Attachment",
        ),
        (
            include_str!("../../tests/fixtures/claude-queued-dequeue.jsonl"),
            3,
            "System",
        ),
        (
            include_str!("../../tests/fixtures/claude-model-command.jsonl"),
            0,
            "Assistant",
        ),
    ] {
        let raw = fixture.lines().nth(line).unwrap();
        assert_eq!(transcript_title(ProviderId::Claude, raw), expected);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.jsonl");
        std::fs::write(&path, raw).unwrap();
        let (mut view, now) = event_fixture();
        let mut event = view.event_panel[0].clone();
        event.source_records = vec![crate::event::SourceRecord { path, offset: 0 }];
        view.set_event(vec![event]);
        event_screen(&view, 120, 30, now);
        transcript(&mut view);
        let rendered = event_screen(&view, 120, 30, now);
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
}

#[test]
fn transcript_codex_titles_prefer_item_then_content_then_payload_type() {
    use crate::providers::ProviderId;
    for (fixture, line, expected) in [
        (
            include_str!("../../tests/fixtures/codex-shell-truncated.jsonl"),
            0,
            "Command Execution",
        ),
        (
            include_str!("../../tests/fixtures/codex-rollout.jsonl"),
            5,
            "Agent Message",
        ),
        (
            include_str!("../../tests/fixtures/codex-rollout.jsonl"),
            1,
            "Input Text, Input Text",
        ),
        (
            include_str!("../../tests/fixtures/codex-rollout.jsonl"),
            6,
            "Output Text",
        ),
        (
            include_str!("../../tests/fixtures/codex-rollout.jsonl"),
            3,
            "Custom Tool Call",
        ),
        (
            include_str!("../../tests/fixtures/codex-rollout.jsonl"),
            0,
            "Task Started",
        ),
    ] {
        assert_eq!(
            transcript_title(ProviderId::Codex, fixture.lines().nth(line).unwrap()),
            expected
        );
    }
}

/// The title the transcript gives one raw record.
fn transcript_title(provider: crate::providers::ProviderId, raw: &str) -> String {
    super::super::tool_detail::transcript_content(
        &Ok(raw.into()),
        provider,
        &crate::event::Kind::Assistant,
    )
    .sections[0]
        .title
        .title
        .clone()
}
