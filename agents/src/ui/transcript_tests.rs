use super::super::task_ui::EventDetailMode;
use super::*;

fn transcript(view: &mut View) {
    view.set_active_panel(Panel::EventDetail);
    view.handle_key(KeyCode::Right.into());
}

#[test]
fn transcript_left_and_right_switch_only_outside_search_and_terminal() {
    let (mut view, now) = event_fixture();
    event_screen(&view, 160, 45, now);
    transcript(&mut view);
    assert_eq!(view.event_detail.mode, EventDetailMode::Transcript);
    view.handle_key(KeyCode::Left.into());
    assert_eq!(view.event_detail.mode, EventDetailMode::Event);
    transcript(&mut view);
    view.handle_key(KeyCode::Char('/').into());
    view.handle_key(KeyCode::Left.into());
    assert_eq!(view.event_detail.mode, EventDetailMode::Transcript);
    view.handle_key(KeyCode::Esc.into());
    let screen = std::sync::Arc::new(std::sync::Mutex::new(
        crate::session_terminal::TerminalScreen::new(10, 40),
    ));
    view.show_terminal(session_key("log"), screen);
    for (key, bytes) in [(KeyCode::Left, b"\x1b[D"), (KeyCode::Right, b"\x1b[C")] {
        assert_eq!(
            view.handle_key(key.into()),
            Some(Action::TerminalInput(bytes.to_vec()))
        );
    }
}

#[test]
fn transcript_right_click_terminal_returns_to_previous_submode() {
    for key in [KeyCode::Left, KeyCode::Right] {
        let (mut view, now) = event_fixture();
        event_screen(&view, 160, 45, now);
        view.set_active_panel(Panel::EventDetail);
        view.handle_key(key.into());
        let expected = if key == KeyCode::Left {
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
    let rendered = event_screen(&view, 240, 90, now);
    let mut buf = Buffer::empty(Rect::new(0, 0, 240, 90));
    view.render_at(buf.area, &mut buf, now);
    let viewport = view.viewport.borrow();
    let sources = &viewport.event_text.sources;
    assert_eq!(sources.len(), 4);
    for (index, (source, title)) in [
        (claude, "assistant"),
        (codex, "response_item / custom_tool_call"),
    ]
    .into_iter()
    .enumerate()
    {
        let value: serde_json::Value = serde_json::from_str(source).unwrap();
        let at = chrono::DateTime::parse_from_rfc3339(value["timestamp"].as_str().unwrap())
            .unwrap()
            .with_timezone(&Utc);
        let header = sources[index * 2].text.to_string();
        assert!(header.contains(title), "{header}");
        let drawn_header = rendered
            .lines()
            .find(|line| {
                line.contains(title)
                    && line
                        .trim_start()
                        .starts_with(&super::super::formatting::clock_text(at))
            })
            .unwrap_or_else(|| panic!("missing record clock: {rendered}"));
        assert!(drawn_header.contains(title));
        let pretty = &sources[index * 2 + 1].text;
        assert!(pretty.contains('\n'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(pretty).unwrap(),
            value
        );
    }
    assert!(
        buf.content
            .iter()
            .any(|cell| cell.fg != ratatui::style::Color::Reset)
    );
    drop(viewport);
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
fn transcript_json_keeps_absolute_paths_verbatim() {
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
    assert!(rendered.contains(&absolute), "{rendered}");
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
    use crate::providers::ProviderId;
    let raw = Ok(r#"{"type":"assistant","payload":{"type":"nested"}}"#.to_owned());
    let claude = super::super::tool_detail::transcript_content(&raw, ProviderId::Claude);
    assert_eq!(claude.sections[0].title.title, "assistant");
    assert_eq!(claude.sections[0].title.left, None);
    for (raw, title) in [
        (
            r#"{"type":"session_meta","payload":{"cwd":"/tmp"}}"#,
            "session_meta",
        ),
        ("{}", ""),
    ] {
        let content =
            super::super::tool_detail::transcript_content(&Ok(raw.into()), ProviderId::Codex);
        assert_eq!(content.sections[0].title.title, title);
        assert_eq!(content.sections[0].title.left, None);
    }
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
    );
    let part = &content.sections[0].groups[0].parts[0];
    let lines = part.lines(33, Style::default(), Utc::now());
    assert!(lines.len() > 10);
    assert!(lines.iter().all(|line| line.width() <= 33));
    assert_eq!(
        lines.iter().map(ToString::to_string).collect::<String>(),
        raw
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
        styled_bytes(super::super::highlight::code_lines(raw, "json"))
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
    loop {
        let rendered = event_screen(&view, 200, 50, now);
        assert!(!body(&rendered).contains('…'), "{rendered}");
        let viewport = view.viewport.borrow();
        for row in &viewport.event_text.rows {
            for (_, bytes) in &row.cells {
                seen.extend((bytes.start..bytes.end).map(|byte| (row.segment, byte)));
            }
        }
        drop(viewport);
        let before = view.event_detail.scroll.borrow().offset;
        view.handle_key(KeyCode::Down.into());
        event_screen(&view, 200, 50, now);
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
    );
}
