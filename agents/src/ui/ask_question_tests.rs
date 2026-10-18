use super::*;

const REJECTED: &str = include_str!("../../tests/fixtures/claude-questions-rejected.jsonl");
const ANSWERED: &str = include_str!("../../tests/fixtures/claude-questions-answered.jsonl");

fn question_event(fixture: &str) -> crate::log::Event {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(fixture.as_bytes()).unwrap();
    crate::providers::claude::log::tail(file.path(), 10)
        .unwrap()
        .remove(0)
}

fn output(event: &crate::log::Event) -> &str {
    let crate::log::Section::Text { text, .. } = &event.calls[0].output.as_ref().unwrap()[0] else {
        panic!("question output is text")
    };
    text
}

#[test]
fn ask_question_titles_use_tool_name_without_subject() {
    for fixture in [REJECTED, ANSWERED] {
        let title = "AskUserQuestion";
        let event = question_event(fixture);
        assert_eq!(event.calls[0].title, title);
        assert_eq!(event.body, title);
        assert!(log::shows_calls(&event));
        let (mut view, now) = log_fixture();
        view.set_log(vec![event.clone()]);
        view.select_fixture_event(Some(event.clone()));
        let screen = log_screen(&view, 180, 60, now);
        let compact: String = screen.chars().filter(|ch| !ch.is_whitespace()).collect();
        let title: String = title.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(compact.matches(&title).count() >= 2, "{screen}");
        let detail = tool_detail::detail(&event, String::new(), Style::new().dim());
        assert!(detail.sections[0].title.left.is_none());
        assert_eq!(detail.sections[0].title.title, event.calls[0].title);
    }
}

#[test]
fn ask_question_input_is_copyable_json() {
    let mut records: Vec<serde_json::Value> = REJECTED
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for multi in [false, true] {
        records[0]["message"]["content"][0]["input"]["questions"][0]["multiSelect"] = multi.into();
        let fixture = records
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let event = question_event(&fixture);
        let parts = detail_parts(&event);
        let expected =
            serde_json::to_string_pretty(&records[0]["message"]["content"][0]["input"]).unwrap();
        assert_eq!(parts[0].source, expected);
        for width in [24, 180] {
            let (mut view, now) = log_fixture();
            view.set_log(vec![event.clone()]);
            view.select_fixture_event(Some(event.clone()));
            let area = Rect::new(0, 0, width, 150);
            view.viewport.borrow_mut().log_detail = area;
            let mut buf = Buffer::empty(area);
            view.render_log_detail(area, &mut buf, now);
            let viewport = view.viewport.borrow();
            let region = &viewport.log_text;
            let source = region
                .sources
                .iter()
                .find(|source| source.text.as_ref() == expected)
                .unwrap();
            let rows: Vec<_> = region
                .rows
                .iter()
                .filter(|row| row.segment == source.id)
                .collect();
            assert!(!rows.is_empty());
            for row in rows {
                for (columns, bytes) in &row.cells {
                    let source = &parts[0].source[bytes.clone()];
                    let cell = buf[(region.area.x + columns.start, row.y)].symbol();
                    if cell == "…" {
                        assert!(!source.is_empty());
                    } else {
                        assert_eq!(source, cell);
                    }
                }
            }
        }
    }
}

#[test]
fn ask_question_rejection_preserves_output_order() {
    let event = question_event(REJECTED);
    let record: serde_json::Value = serde_json::from_str(REJECTED.lines().nth(1).unwrap()).unwrap();
    let original = record["message"]["content"][0]["content"].as_str().unwrap();
    assert_eq!(output(&event), original);
    assert!(log::open_text(&event).contains(output(&event)));
}

#[test]
fn ask_question_outputs_without_marker_are_unchanged() {
    let answered = question_event(ANSWERED);
    let record: serde_json::Value = serde_json::from_str(ANSWERED.lines().nth(1).unwrap()).unwrap();
    assert_eq!(
        output(&answered),
        record["message"]["content"][0]["content"].as_str().unwrap()
    );
    let mut records: Vec<serde_json::Value> = REJECTED
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    records[1]["message"]["content"][0]["content"] = "The tool use was rejected.".into();
    let fixture = records
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        output(&question_event(&fixture)),
        "The tool use was rejected."
    );
}

#[test]
fn ask_question_detail_snapshots_keep_sections_centered_and_copyable() {
    for (fixture, name) in [
        (REJECTED, "task_log_questions_rejected.txt"),
        (ANSWERED, "task_log_questions_answered.txt"),
    ] {
        let event = question_event(fixture);
        let (mut view, now) = log_fixture();
        view.set_log(vec![event.clone()]);
        view.select_fixture_event(Some(event.clone()));
        let area = Rect::new(0, 0, 140, 35);
        view.viewport.borrow_mut().log_detail = area;
        let mut buf = Buffer::empty(area);
        view.render_log_detail(area, &mut buf, now);
        let screen = buf
            .content
            .chunks(usize::from(area.width))
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/ui_snapshots")
            .join(name);
        if std::env::var_os("UPDATE_DETAIL_SNAPSHOT").is_some() {
            std::fs::write(&path, format!("{}\n", screen.trim_end())).unwrap();
        }
        assert_eq!(
            screen.trim_end(),
            std::fs::read_to_string(path).unwrap().trim_end()
        );

        let parts = detail_parts(&event);
        let viewport = view.viewport.borrow();
        let region = &viewport.log_text;
        let columns =
            super::super::detail_model::detail_columns(area, view.center_layout(area).log);
        for row in &region.rows {
            assert_eq!(row.x, columns.text.x);
            assert!(
                row.cells
                    .iter()
                    .all(|(cells, _)| cells.end <= columns.text.width)
            );
        }
        assert_eq!(region.sources[1].text.as_ref(), parts[0].source);
        drop(viewport);
        let screen = log_screen(&view, 180, 70, now);
        assert_eq!(
            double_click_text(&mut view, &screen, "(Recommended)"),
            Some(Action::Copy {
                text: log::open_text(&event),
                label: log::copied_label(&event)
            })
        );
    }
}
