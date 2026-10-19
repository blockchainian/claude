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
fn ask_question_list_uses_questions_and_detail_uses_headers() {
    for (fixture, title) in [
        (REJECTED, ["压缩摘要"].as_slice()),
        (ANSWERED, ["子命令", "cd"].as_slice()),
    ] {
        let event = question_event(fixture);
        let record: serde_json::Value =
            serde_json::from_str(fixture.lines().next().unwrap()).unwrap();
        let questions = record["message"]["content"][0]["input"]["questions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|question| question["header"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join(" · ");
        assert_eq!(event.calls[0].title, format!("AskUserQuestion {questions}"));
        assert_eq!(event.body, format!("AskUserQuestion {questions}"));
        assert!(log::shows_calls(&event));
        let (mut view, now) = log_fixture();
        view.set_log(vec![event.clone()]);
        view.select_fixture_event(Some(event.clone()));
        let screen = log_screen(&view, 180, 60, now);
        let compact: String = screen.chars().filter(|ch| !ch.is_whitespace()).collect();
        assert!(
            compact.contains(&format!("AskUserQuestion{}", questions.replace(' ', ""))),
            "{screen}"
        );
        let detail = tool_detail::detail(&event, String::new(), Style::new().dim());
        assert_eq!(
            detail.sections[0].title.left.as_deref(),
            Some("AskUserQuestion")
        );
        assert_eq!(
            detail
                .sections
                .iter()
                .map(|section| section.title.title.as_str())
                .collect::<Vec<_>>(),
            title
        );
        assert_eq!(event.calls[0].detail_title, title.join(" · "));
    }
}

#[test]
fn ask_question_input_is_a_copyable_numbered_list() {
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
        let suffix = if multi { " (multi-select)" } else { "" };
        let expected = format!(
            "压缩摘要那条在 agents 里怎么处理？{suffix}\n1. 显示成压缩标记 (Recommended): 不当用户消息，日志里出一行“Conversation compacted”之类的系统行\n2. 直接隐藏: 日志里完全不显示这条"
        );
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
                    assert_eq!(
                        &parts[0].source[bytes.clone()],
                        buf[(region.area.x + columns.start, row.y)].symbol()
                    );
                }
            }
        }
    }
}

fn with_result(fixture: &str, content: &str) -> crate::log::Event {
    let mut records: Vec<serde_json::Value> = fixture
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    records[1]["message"]["content"][0]["content"] = content.into();
    question_event(
        &records
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[test]
fn ask_question_answers_list_headers_like_the_submit_tab() {
    let event = question_event(ANSWERED);
    let record: serde_json::Value = serde_json::from_str(ANSWERED.lines().nth(1).unwrap()).unwrap();
    let result = &record["toolUseResult"];
    let expected = result["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| {
            format!(
                "● {} → {}",
                question["header"].as_str().unwrap(),
                result["answers"][question["question"].as_str().unwrap()]
                    .as_str()
                    .unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(output(&event), expected);
    assert!(log::open_text(&event).contains(output(&event)));
}

#[test]
fn ask_question_rejections_list_what_was_answered() {
    assert_eq!(
        output(&question_event(REJECTED)),
        "Chat about this\n\nYou have not answered all questions"
    );
    let record: serde_json::Value = serde_json::from_str(REJECTED.lines().nth(1).unwrap()).unwrap();
    let original = record["message"]["content"][0]["content"].as_str().unwrap();
    let answered = original.replace("(No answer provided)", "Answer: 直接隐藏");
    assert_eq!(
        output(&with_result(REJECTED, &answered)),
        "Chat about this\n\n● 压缩摘要 → 直接隐藏"
    );
    let rejected = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed.";
    assert_eq!(output(&with_result(REJECTED, rejected)), "Rejected");
    let noted = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nask me later";
    assert_eq!(
        output(&with_result(REJECTED, noted)),
        "Rejected\n\nask me later"
    );
    assert_eq!(
        output(&with_result(REJECTED, "The tool use was rejected.")),
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
        assert_eq!(region.sources[1].text.trim_end(), parts[0].source);
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
