use super::rollout::{Reader, summary, tail};
use crate::event::Kind;
use serde_json::Value;
use std::io::Write;

fn records() -> Vec<Value> {
    include_str!("../../../tests/fixtures/codex-memory-citation/reply.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn expected() -> String {
    records()[0]["payload"]["item"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn memory_reply_uses_processed_body_in_both_record_orders() {
    for reverse in [false, true] {
        let mut records = records();
        if reverse {
            records.reverse();
        }
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for record in records {
            writeln!(file, "{record}").unwrap();
        }
        assert_eq!(summary(file.path()).unwrap().last_message, Some(expected()));
        let events = tail(file.path(), 1).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Assistant);
        assert_eq!(events[0].body, expected());
        assert_memory_note(&events[0]);
        assert!(!events[0].body.contains("<oai-mem-citation>"));
        assert_eq!(tail(file.path(), 100).unwrap().len(), 1);
    }
}

#[test]
fn memory_reply_raw_body_is_replaced_when_completion_arrives() {
    let records = records();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{}", records[1]).unwrap();
    let mut reader = Reader::new(file.path().to_owned(), false);
    let raw = records[1]["payload"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert_eq!(reader.refresh(1).unwrap().events[0].body, raw);
    assert_eq!(
        summary(file.path()).unwrap().last_message.as_deref(),
        Some(raw)
    );
    writeln!(file, "{}", records[0]).unwrap();
    let update = reader.refresh(1).unwrap();
    assert_eq!(update.events.len(), 1);
    assert_eq!(update.appended, 0);
    assert_eq!(update.replaced, 1);
    assert_eq!(update.events[0].body, expected());
    assert_memory_note(&update.events[0]);
    assert_eq!(summary(file.path()).unwrap().last_message, Some(expected()));
    assert_eq!(reader.refresh(1).unwrap().parsed_bytes, 0);
}

#[test]
fn memory_reply_completion_is_not_replaced_by_later_raw_record() {
    let records = records();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{}", records[0]).unwrap();
    let mut reader = Reader::new(file.path().to_owned(), false);
    assert_eq!(reader.refresh(1).unwrap().events[0].body, expected());
    writeln!(file, "{}", records[1]).unwrap();
    let update = reader.refresh(1).unwrap();
    assert_eq!(update.events.len(), 1);
    assert_eq!(update.appended, 0);
    assert_eq!(update.events[0].body, expected());
    assert_memory_note(&update.events[0]);
}

#[test]
fn memory_reply_different_message_ids_in_one_turn_remain_separate() {
    let mut records = records();
    records[1]["payload"]["id"] = "different-message".into();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for record in &records {
        writeln!(file, "{record}").unwrap();
    }
    let events = tail(file.path(), 100).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].body, expected());
    assert_eq!(
        events[1].body,
        records[1]["payload"]["content"][0]["text"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        summary(file.path()).unwrap().last_message.as_deref(),
        Some(events[1].body.as_str())
    );
}

#[test]
fn memory_reply_completion_before_paging_boundary_still_wins() {
    let mut records = records();
    records[1]["payload"]["content"][0]["text"] = records[1]["payload"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .repeat(100)
        .into();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    for record in records {
        writeln!(file, "{record}").unwrap();
    }
    let mut reader = Reader::new(file.path().to_owned(), false);
    assert_eq!(reader.refresh(1).unwrap().events[0].body, expected());
}

fn assert_memory_note(event: &crate::event::Event) {
    let detail = event.detail_body.as_ref().unwrap();
    assert!(detail.text.is_none());
    assert_eq!(detail.notes.len(), 1);
    let note = &detail.notes[0];
    assert_eq!(
        note.text,
        "Memory: MEMORY.md:136 · source conditions validation context"
    );
    assert_eq!(&note.text[note.links[0].range.clone()], "MEMORY.md:136");
    assert_eq!(note.links[0].line, 136);
    let home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".codex")
        });
    assert_eq!(note.links[0].path, home.join("memories/MEMORY.md"));
}

#[test]
fn memory_reply_without_entries_has_no_detail_notes() {
    for citation in [
        Value::Null,
        serde_json::json!({"entries": [], "rolloutIds": []}),
    ] {
        let mut record = records().remove(0);
        record["payload"]["item"]["memory_citation"] = citation;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        assert!(tail(file.path(), 1).unwrap()[0].detail_body.is_none());
    }
}

#[test]
fn memory_reply_formats_ranges_and_aligns_following_entries() {
    let mut record = records().remove(0);
    let entry = record["payload"]["item"]["memory_citation"]["entries"][0].clone();
    record["payload"]["item"]["memory_citation"]["entries"]
        .as_array_mut()
        .unwrap()
        .push(entry);
    record["payload"]["item"]["memory_citation"]["entries"][1]["lineEnd"] = 140.into();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{record}").unwrap();
    let events = tail(file.path(), 1).unwrap();
    let notes = &events[0].detail_body.as_ref().unwrap().notes;
    assert_eq!(
        notes[0].text,
        "Memory: MEMORY.md:136 · source conditions validation context"
    );
    assert_eq!(
        notes[1].text,
        "        MEMORY.md:136-140 · source conditions validation context"
    );
    assert_eq!(
        &notes[1].text[notes[1].links[0].range.clone()],
        "MEMORY.md:136-140"
    );
}

#[test]
fn memory_reply_rejects_invalid_locations() {
    for (path, start, end) in [
        ("../MEMORY.md", 136, 136),
        ("/MEMORY.md", 136, 136),
        ("MEMORY.md", 0, 1),
        ("MEMORY.md", 140, 136),
    ] {
        let mut record = records().remove(0);
        let entry = &mut record["payload"]["item"]["memory_citation"]["entries"][0];
        entry["path"] = path.into();
        entry["lineStart"] = start.into();
        entry["lineEnd"] = end.into();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{record}").unwrap();
        assert_eq!(
            tail(file.path(), 1).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
