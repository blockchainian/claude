use super::*;
use crate::providers::Provider;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Modifier;

fn open_tool_fixture() -> (View, DateTime<Utc>) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&path, include_str!("../../tests/fixtures/transcript.jsonl")).unwrap();
    let mut provider = crate::providers::claude::ClaudeProvider::new(dir.path().to_owned());
    let mut view = View::new(Grouping::Project, false);
    view.set_sessions(provider.refresh(1).sessions, false);
    view.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.log_session = view.selected_id().cloned();
    let mut event = crate::providers::claude::log::tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    event.kind = Kind::Tool;
    event.calls = tool_calls();
    let now = event.started_at;
    view.set_log(vec![event.clone()]);
    view.select_fixture_event(Some(event));
    (view, now)
}

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn tool_calls() -> Vec<crate::log::ToolCall> {
    use crate::log::{Section, ToolCall};
    vec![
        ToolCall {
            detail_left: "Bash".into(),
            detail_title: "Bash".into(),
            show_details: false,
            id: "Bash-fixture".into(),
            title: "Bash".into(),
            name: "Bash".into(),
            input: vec![Section::Shell {
                prefix: 0,
                text: "cargo test".into(),
            }],
            output: Some(vec![Section::Text {
                label: "stdout".into(),
                text: "running 3 tests\n...".into(),
            }]),
            exit_code: None,
            is_error: false,
            duration: Some(chrono::Duration::seconds(12)),
        },
        ToolCall {
            detail_left: "Read".into(),
            detail_title: "Read".into(),
            show_details: false,
            id: "Read-fixture".into(),
            title: "Read".into(),
            name: "Read".into(),
            input: vec![Section::Text {
                label: "path".into(),
                text: "agents/src/log.rs:1-40".into(),
            }],
            output: None,
            exit_code: None,
            is_error: false,
            duration: None,
        },
    ]
}

fn text_cell<'a>(buffer: &'a Buffer, text: &str) -> &'a ratatui::buffer::Cell {
    for y in buffer.area.y..buffer.area.bottom() {
        let row: String = (buffer.area.x..buffer.area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(byte) = row.find(text) {
            return &buffer[(buffer.area.x + row[..byte].chars().count() as u16, y)];
        }
    }
    panic!("missing rendered text: {text}");
}

#[test]
fn detail_body_uses_default_foreground_and_preserves_title_and_markdown_styles() {
    let (view, now) = open_tool_fixture();
    let mut event = view.visible_log_event().unwrap().clone();
    event.calls.clear();
    event.body =
        "plain **bold** [link](https://example.com)\n\n# Heading\n\n```rust\nlet value = 42;\n```"
            .into();
    let mut snapshot = String::new();
    for kind in [
        Kind::Assistant,
        Kind::Agent,
        Kind::Error,
        Kind::Denied,
        Kind::QueuedSystem,
    ] {
        event.kind = kind;
        let area = Rect::new(0, 0, 120, 30);
        let mut buffer = Buffer::empty(area);
        let view = View::new(Grouping::Project, false);
        view.render_event_detail(&event, "12:34:56", area, &mut buffer, Path::new("/"), now);
        let title = text_cell(&buffer, kind_label(&event.kind));
        assert_eq!(
            title.fg,
            super::super::style::log_styles(&event.kind).0.fg.unwrap()
        );
        assert_eq!(
            text_cell(&buffer, "plain").fg,
            Color::Reset,
            "{:?}",
            event.kind
        );
        if matches!(event.kind, Kind::Assistant | Kind::Agent) {
            assert!(text_cell(&buffer, "bold").modifier.contains(Modifier::BOLD));
            assert_eq!(text_cell(&buffer, "bold").fg, Color::Reset);
            assert!(
                text_cell(&buffer, "Heading")
                    .modifier
                    .contains(Modifier::BOLD)
            );
            assert_eq!(text_cell(&buffer, "Heading").fg, Color::Reset);
            assert_eq!(text_cell(&buffer, "link").fg, Color::Reset);
            assert_eq!(text_cell(&buffer, "let value").fg, Color::Cyan);
            assert!(
                view.viewport
                    .borrow()
                    .log_links
                    .iter()
                    .any(|(_, target)| target.to_string_lossy().contains("example.com"))
            );
        }
        let mut list = Buffer::empty(Rect::new(0, 0, 120, 1));
        render_event(
            &event,
            "12:34:56",
            false,
            list.area,
            &mut list,
            None,
            Path::new("/"),
        );
        assert_eq!(
            text_cell(&list, "plain").fg,
            super::super::style::log_styles(&event.kind).1.fg.unwrap()
        );
        snapshot.push_str(&format!("{:?}\n", event.kind));
        for text in [
            kind_label(&event.kind),
            "plain",
            "bold",
            "link",
            "Heading",
            "let value",
        ] {
            let cell = text_cell(&buffer, text);
            snapshot.push_str(&format!("  {text}: {:?} {:?}\n", cell.fg, cell.modifier));
        }
        snapshot.push_str(&format!(
            "  list plain: {:?}\n",
            text_cell(&list, "plain").fg
        ));
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/log_detail_colors.txt");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn tool_detail_body_uses_default_foreground_and_preserves_syntax_highlighting() {
    let (mut view, now) = open_tool_fixture();
    let mut event = view.visible_log_event().unwrap().clone();
    event.calls.truncate(1);
    event.calls[0].detail_title = "Run checks".into();
    event.calls[0].output = Some(vec![
        crate::log::Section::Text {
            label: String::new(),
            text: "plain output".into(),
        },
        crate::log::Section::Code {
            label: String::new(),
            lang: "rust".into(),
            text: "let value = 42;".into(),
        },
        crate::log::Section::Json {
            label: String::new(),
            text: "{\"answer\":42}".into(),
        },
    ]);
    for failed in [false, true] {
        event.calls[0].is_error = failed;
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, 200, 35);
        let mut buffer = Buffer::empty(area);
        view.render_at(area, &mut buffer, now);
        let detail = view.viewport.borrow().log_detail;
        let mut body = Buffer::empty(detail);
        for y in detail.y..detail.bottom() {
            for x in detail.x..detail.right() {
                body[(x, y)] = buffer[(x, y)].clone();
            }
        }
        assert_eq!(
            text_cell(&body, "Run checks").fg,
            super::super::style::log_styles(&Kind::Tool).0.fg.unwrap()
        );
        assert_eq!(text_cell(&body, "plain output").fg, Color::Reset);
        for token in ["cargo", "let", "answer"] {
            assert_ne!(text_cell(&body, token).fg, Color::Reset, "{token}");
        }
        let expected = super::super::highlight::code_lines("let value = 42;", "rust");
        assert_eq!(
            text_cell(&body, "let").fg,
            expected[0].spans[0].style.fg.unwrap()
        );
        assert_eq!(
            text_cell(&body, "answer").fg,
            super::super::highlight::json_key_foreground()
        );
    }
}

#[test]
fn cross_section_drag_preserves_blank_line() {
    let (mut view, now) = open_tool_fixture();
    let event = view.visible_log_event().unwrap().clone();
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 60));
    view.render_at(buf.area, &mut buf, now);
    let (first, last, expected) = {
        let viewport = view.viewport.borrow();
        let region = &viewport.log_text;
        let first = region
            .rows
            .iter()
            .find(|row| !row.cells.is_empty())
            .unwrap()
            .clone();
        let last = region
            .rows
            .iter()
            .rev()
            .find(|row| !row.cells.is_empty())
            .unwrap()
            .clone();
        let expected = open_text(&event).trim_end_matches('\n').to_owned();
        (first, last, expected)
    };
    let x1 = first.x + first.cells.first().unwrap().0.start;
    let x2 = last.x + last.cells.last().unwrap().0.end;
    view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x1, first.y));
    view.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x2, last.y));
    let action = view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x2, last.y));
    assert_eq!(
        action,
        Some(Action::Copy {
            text: expected,
            label: "Copied selection".into()
        })
    );
}

#[test]
fn title_to_body_drag_does_not_copy_the_display_gap() {
    let (mut view, now) = open_tool_fixture();
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 60));
    view.render_at(buf.area, &mut buf, now);
    let (title, body, expected, gap_y) = {
        let viewport = view.viewport.borrow();
        let region = &viewport.log_text;
        let title = region
            .rows
            .iter()
            .find(|row| row.segment == 0 && !row.cells.is_empty())
            .unwrap()
            .clone();
        let body = region
            .rows
            .iter()
            .find(|row| row.segment == 1 && !row.cells.is_empty())
            .unwrap()
            .clone();
        let expected = format!("{}\n{}", region.sources[0].text, region.sources[1].text);
        let gap_y = title.y + 1;
        (title, body, expected, gap_y)
    };
    assert_eq!(body.y, title.y + 2);
    view.begin_text_selection(title.x, gap_y);
    assert!(view.text_selection.borrow().is_none());
    let start = title.x + title.cells.first().unwrap().0.start;
    let end = body.x + body.cells.last().unwrap().0.end;
    view.begin_text_selection(start, title.y);
    view.extend_text_selection(end, body.y);
    assert_eq!(
        view.finish_text_selection(end, body.y),
        Some(Action::Copy {
            text: expected,
            label: "Copied selection".into(),
        })
    );
}

fn assert_hidden_suffix_search(width: u16) {
    let (mut view, now) = open_tool_fixture();
    let mut event = view.visible_log_event().unwrap().clone();
    event.calls.truncate(1);
    let output = (0..50)
        .map(|n| format!("row {n}"))
        .chain(std::iter::once(format!("{}HIDDEN_NEEDLE", "x".repeat(240))))
        .collect::<Vec<_>>()
        .join("\n");
    event.calls[0].output = Some(vec![crate::log::Section::Text {
        label: "stdout".into(),
        text: output,
    }]);
    view.select_fixture_event(Some(event));
    let area = Rect::new(0, 0, width, 30);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    let target = {
        let viewport = view.viewport.borrow();
        let layout = viewport.log_detail_layout.as_ref().unwrap();
        let target = layout
            .rows
            .iter()
            .position(|(id, cells)| {
                cells.iter().any(|(_, bytes)| {
                    layout.sources[*id].text[bytes.clone()].contains("HIDDEN_NEEDLE")
                })
            })
            .unwrap();
        let drawn = layout.lines[target].to_string();
        assert!(drawn.ends_with('…'));
        assert!(!drawn.contains("HIDDEN_NEEDLE"));
        assert_eq!(viewport.log_detail_scroll, 0);
        target
    };
    view.log_detail_search.query.insert("HIDDEN_NEEDLE");
    view.jump_detail_match();
    assert_eq!(view.viewport.borrow().log_detail_scroll, target);
    view.render_at(area, &mut buf, now);
    let viewport = view.viewport.borrow();
    assert!(viewport.log_detail_scroll > 0);
    assert!(viewport.log_text.rows.iter().any(|row| {
        row.cells.iter().any(|(_, bytes)| {
            viewport.log_text.sources[row.segment].text[bytes.clone()].contains("HIDDEN_NEEDLE")
        })
    }));
}

#[test]
fn search_finds_hidden_suffix_at_100_columns() {
    assert_hidden_suffix_search(100);
}

#[test]
fn search_finds_hidden_suffix_at_200_columns() {
    assert_hidden_suffix_search(200);
}

#[test]
fn claude_write_content_uses_path_syntax_and_preserves_unknown_text() {
    let fixture = include_str!("../../tests/fixtures/claude-write/python.jsonl");
    let mut record: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let mut snapshot = String::new();
    for (path, content, token, highlighted) in [
        ("/tmp/tail.py", None, "\"user\"", true),
        (
            "/tmp/Dockerfile",
            Some("FROM alpine\nRUN echo hello\n"),
            "FROM",
            true,
        ),
        (
            "/tmp/tail.unknown-syntax",
            Some("import json, sys\n"),
            "import",
            false,
        ),
        (
            "/tmp/no-extension",
            Some("import json, sys\n"),
            "import",
            false,
        ),
    ] {
        record["message"]["content"][0]["input"]["file_path"] = path.into();
        if let Some(content) = content {
            record["message"]["content"][0]["input"]["content"] = content.into();
        }
        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(transcript.path(), format!("{record}\n")).unwrap();
        let event = crate::providers::claude::log::tail(transcript.path(), 1)
            .unwrap()
            .remove(0);
        assert_eq!(event.calls[0].detail_title, path);
        let view = View::new(Grouping::Project, false);
        let area = Rect::new(0, 0, 120, 24);
        let mut buffer = Buffer::empty(area);
        view.render_event_detail(
            &event,
            "12:34:56",
            area,
            &mut buffer,
            Path::new("/"),
            event.started_at,
        );
        let color = text_cell(&buffer, token).fg;
        assert_eq!(color != Color::Reset, highlighted, "{path}: {color:?}");
        snapshot.push_str(&format!("{path}\n"));
        for y in area.y..area.bottom() {
            let row: String = (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if row.trim().is_empty() {
                continue;
            }
            snapshot.push_str(row.trim_end());
            snapshot.push('\n');
        }
        snapshot.push_str(&format!("{token}: {color:?}\n\n"));
    }
    snapshot.pop();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/claude_write.txt");
    if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}
