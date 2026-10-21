use super::*;
use crate::event::{Reader as _, SourceRecord};
use std::io::Write;

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    [
        "codex-rollout.jsonl",
        "codex-tool-calls.jsonl",
        "codex-agent-exchange.jsonl",
        "codex-agent-messages/sequence.jsonl",
        "codex-perspective/root.jsonl",
        "codex-perspective/alpha.jsonl",
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
    let mut merged = 0;
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
            merged += usize::from(event.calls.len() > 1 && event.kind == Kind::Agent);
            compared += 1;
            assert_eq!(read, Some(without_context(event)), "{}", path.display());
        }
    }
    assert!(compared > 10, "{compared}");
    assert!(merged > 0, "no merged message was compared");
}

#[test]
fn an_unreadable_group_is_none_and_the_others_are_read() {
    let path = fixtures().remove(1);
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
fn a_group_whose_record_fails_to_decode_is_none() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    let mut offsets = Vec::new();
    for (index, line) in include_str!("../../../tests/fixtures/codex-tool-calls.jsonl")
        .lines()
        .enumerate()
    {
        offsets.push(file.as_file().metadata().unwrap().len());
        let mut record: Value = serde_json::from_str(line).unwrap();
        if index == 5 {
            record["payload"]["item"]["id"] = Value::Null;
        }
        writeln!(file, "{record}").unwrap();
    }
    let mut reader = Reader::new(file.path().to_path_buf(), false);
    let group = |index: usize| {
        vec![SourceRecord {
            path: file.path().to_path_buf(),
            offset: offsets[index],
        }]
    };
    let read = reader.read_events(&[group(4), group(5), group(6)]).unwrap();
    assert!(read[0].is_some() && read[1].is_none() && read[2].is_some());
}

#[test]
fn a_missing_transcript_reads_every_group_as_none() {
    let mut reader = Reader::new(PathBuf::from("/nonexistent/rollout.jsonl"), false);
    let group = vec![SourceRecord {
        path: PathBuf::from("/nonexistent/rollout.jsonl"),
        offset: 0,
    }];
    let read = reader.read_events(&[group.clone(), group]).unwrap();
    assert_eq!(read, vec![None, None]);
}
