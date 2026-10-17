use super::*;
use crate::providers::Provider;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

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
            subagent_type: None,
            id: "Bash-fixture".into(),
            title: "Bash".into(),
            name: "Bash".into(),
            input: vec![Section::Code {
                label: "command".into(),
                lang: "bash".into(),
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
            subagent_type: None,
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
