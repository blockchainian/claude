use super::*;
use crate::event::{Reader as _, SourceRecord};

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    [
        "claude-tool-calls.jsonl",
        "claude-tool-details.jsonl",
        "claude-agent-names.jsonl",
        "claude-peer-prompts.jsonl",
        "claude-task-notifications.jsonl",
        "claude-queued-dequeue.jsonl",
        "claude-prompt-images.jsonl",
        "transcript.jsonl",
    ]
    .into_iter()
    .map(|name| dir.join(name))
    .collect()
}

fn without_context(mut event: Event) -> Event {
    event.turn = None;
    event.previous_end = None;
    event
}

#[test]
fn records_read_back_as_the_sequentially_decoded_event() {
    let mut compared = 0;
    let mut multi_record = 0;
    for path in fixtures() {
        let mut reader = Reader::new(path.clone(), false);
        let events = reader.refresh(usize::MAX).unwrap().events;
        let groups: Vec<_> = events
            .iter()
            .map(|event| event.source_records.clone())
            .collect();
        let read = reader.read_events(&groups).unwrap();
        assert_eq!(read.len(), events.len());
        for (event, read) in events.into_iter().zip(read) {
            compared += 1;
            multi_record += usize::from(event.source_records.len() > 1);
            assert_eq!(read, Some(without_context(event)), "{}", path.display());
        }
    }
    assert!(compared > 20, "{compared}");
    assert!(multi_record > 0, "no multi-record event was compared");
}

#[test]
fn an_unreadable_group_is_none_and_the_others_are_read() {
    let path = fixtures().remove(0);
    let mut reader = Reader::new(path.clone(), false);
    let events = reader.refresh(usize::MAX).unwrap().events;
    let record = |offset| SourceRecord {
        path: path.clone(),
        offset,
    };
    let first = events[0].source_records.clone();
    let end = std::fs::metadata(&path).unwrap().len();
    let groups = vec![
        first.clone(),
        vec![record(first[0].offset + 1)],
        vec![record(end + 100)],
        vec![],
        vec![SourceRecord {
            path: path.with_extension("gone"),
            offset: 0,
        }],
        first,
    ];
    let read = reader.read_events(&groups).unwrap();
    assert!(read[0].is_some());
    assert_eq!(read[0], read[5]);
    assert!(read[1..5].iter().all(Option::is_none), "{read:?}");
}

#[test]
fn a_tool_block_without_an_id_has_no_source() {
    let mut line: serde_json::Value = serde_json::from_str(
        include_str!("../../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .find(|line| line.contains("\"tool_use\""))
            .unwrap(),
    )
    .unwrap();
    for block in line["message"]["content"].as_array_mut().unwrap() {
        if block["type"] == "tool_use" {
            block.as_object_mut().unwrap().remove("id");
        }
    }
    let mut file = tempfile::NamedTempFile::new().unwrap();
    std::io::Write::write_all(&mut file, format!("{line}\n").as_bytes()).unwrap();
    let events = Reader::new(file.path().to_path_buf(), false)
        .refresh(usize::MAX)
        .unwrap()
        .events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, None);
}
