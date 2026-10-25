use super::*;
use std::io::Write;

fn text(event: &Event) -> String {
    parts(event)
        .into_iter()
        .flat_map(|section| section.groups)
        .flat_map(|group| group.parts)
        .map(|part| part.source)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn notification_detail_reads_appended_output_after_empty_file() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("task output.output");
    let mut file = std::fs::File::create(&output).unwrap();
    let fixture = include_str!("../../tests/fixtures/claude-task-notifications.jsonl").replace(
        "/tmp/agents-notification/tasks/b9wi31ww0.output",
        output.to_str().unwrap(),
    );
    let transcript = directory.path().join("transcript.jsonl");
    std::fs::write(&transcript, fixture).unwrap();
    let events = crate::providers::claude::event_reader::tail(&transcript, usize::MAX).unwrap();
    let event = events.iter().find(|event| !event.calls.is_empty()).unwrap();
    assert!(!text(event).contains("first output"));
    writeln!(file, "first output").unwrap();
    assert!(text(event).contains("first output"));
    writeln!(file, "second output").unwrap();
    assert!(text(event).contains("first output\nsecond output"));
}

#[test]
fn background_start_detail_reads_appended_output() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("task output.output");
    let mut file = std::fs::File::create(&output).unwrap();
    let fixture = include_str!("../../tests/fixtures/background/two-awaits.jsonl");
    let mut rows: Vec<serde_json::Value> = fixture
        .lines()
        .take(2)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let content = rows[1]["message"]["content"][0]["content"]
        .as_str()
        .unwrap();
    let path = content
        .split_once("Output is being written to: ")
        .unwrap()
        .1
        .split_once(". You will be notified")
        .unwrap()
        .0;
    rows[1]["message"]["content"][0]["content"] =
        content.replace(path, output.to_str().unwrap()).into();
    let transcript = directory.path().join("transcript.jsonl");
    std::fs::write(
        &transcript,
        rows.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let events = crate::providers::claude::event_reader::tail(&transcript, usize::MAX).unwrap();
    let event = events.iter().find(|event| !event.calls.is_empty()).unwrap();
    assert!(!text(event).contains("appended command output"));
    writeln!(file, "appended command output").unwrap();
    assert!(text(event).contains("appended command output"));
}
