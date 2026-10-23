use super::*;
use crate::event::Section;

fn recorded_handback() -> (Vec<crate::event::Event>, String) {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-subagent-handback.jsonl");
    let record: serde_json::Value = serde_json::from_str(
        include_str!("../../tests/fixtures/claude-subagent-handback.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let message = record["message"]["content"][0]["input"]["message"]
        .as_str()
        .unwrap()
        .to_owned();
    (
        crate::providers::claude::event_reader::tail_session(&path, 10, true).unwrap(),
        message,
    )
}

#[test]
fn subagent_handback_recorded_input_output_and_titles() {
    let (events, message) = recorded_handback();
    assert_eq!(events.len(), 1);
    let call = &events[0].calls[0];
    assert_eq!(
        call.input,
        vec![Section::Markdown {
            label: "message".into(),
            text: message.clone()
        }]
    );
    assert_eq!(
        call.output.as_ref().unwrap(),
        &vec![Section::Text {
            label: "output".into(),
            text: "Report delivered to your caller.".into(),
        }]
    );
    assert_eq!(events[0].body, message);
    assert_eq!(call.detail_left(), "");
    assert_eq!(super::tool_detail::call_title(call).1, "SubagentHandback");
}

#[test]
fn subagent_handback_recorded_ui_snapshot() {
    let (events, _) = recorded_handback();
    let now = events[0].ended_at.unwrap();
    let (mut view, _) = event_fixture();
    view.set_event(events.clone());
    view.select_fixture_event(Some(events[0].clone()));
    for width in [120, 180] {
        let rendered = event_screen(&view, width, 150, now);
        assert!(
            rendered.contains("Report delivered to your caller."),
            "{rendered}"
        );
        assert!(!rendered.contains("\"message\":"), "{rendered}");
        assert!(!rendered.contains("**Cause:**"), "{rendered}");
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/ui_snapshots/subagent_handback_{width}.txt"));
        if std::env::var_os("UPDATE_HANDBACK_SNAPSHOTS").is_some() {
            std::fs::write(&path, &rendered).unwrap();
        }
        assert_eq!(rendered, std::fs::read_to_string(path).unwrap());
    }
}
