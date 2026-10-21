use super::*;
use crate::event::{Reader as _, SourceRecord};

#[test]
fn pinned_missing_transcript_discards_each_group() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-rollout.jsonl");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    std::fs::copy(fixture, &path).unwrap();
    let groups: Vec<_> = tail(&path, usize::MAX)
        .unwrap()
        .into_iter()
        .map(|event| event.source_records)
        .collect();
    assert!(!groups.is_empty());
    let mut reader = Reader::new(path.clone(), false);
    std::fs::remove_file(path).unwrap();
    let events = reader.read_events(&groups).unwrap();
    assert_eq!(events.len(), groups.len());
    assert!(events.iter().all(Option::is_none));
}

#[test]
fn pinned_records_restore_real_events_before_refresh() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for fixture in [
        "codex-rollout.jsonl",
        "codex-raw-detail.jsonl",
        "codex-agent-exchange.jsonl",
        "codex-perspective/alpha.jsonl",
    ] {
        let path = root.join(fixture);
        let mut expected = tail(&path, usize::MAX).unwrap();
        assert!(!expected.is_empty());
        let groups: Vec<_> = expected
            .iter()
            .map(|event| {
                let mut records = event.source_records.clone();
                records.reverse();
                records
            })
            .collect();
        let mut reader = Reader::new(path, false);
        let actual = reader.read_events(&groups).unwrap();
        for event in &mut expected {
            event.turn = None;
            event.previous_end = None;
        }
        assert_eq!(
            actual,
            expected.into_iter().map(Some).collect::<Vec<_>>(),
            "{fixture}"
        );
    }
}

#[test]
fn pinned_malformed_completion_only_discards_its_group() {
    use std::io::Write;
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-tool-calls.jsonl");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut record: Value = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
        .unwrap();
    record["payload"]["item"]
        .as_object_mut()
        .unwrap()
        .remove("command");
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(text.as_bytes()).unwrap();
    let good = tail(file.path(), usize::MAX).unwrap().remove(0);
    let offset = text.len() as u64;
    serde_json::to_writer(&mut file, &record).unwrap();
    file.write_all(b"\n").unwrap();
    let mut reader = Reader::new(file.path().to_owned(), false);
    let actual = reader
        .read_events(&[
            vec![SourceRecord {
                path: file.path().to_owned(),
                offset,
            }],
            good.source_records,
        ])
        .unwrap();
    assert!(actual[0].is_none());
    assert!(actual[1].is_some());
}

#[test]
fn pinned_invalid_records_do_not_discard_other_groups() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex-rollout.jsonl");
    let event = tail(&path, usize::MAX).unwrap().remove(0);
    let mut reader = Reader::new(path.clone(), false);
    let groups = vec![
        vec![],
        vec![SourceRecord {
            path: path.clone(),
            offset: u64::MAX,
        }],
        vec![SourceRecord {
            path: path.with_extension("missing"),
            offset: 0,
        }],
        vec![SourceRecord { path, offset: 1 }],
        event.source_records,
    ];
    let actual = reader.read_events(&groups).unwrap();
    assert_eq!(actual.len(), groups.len());
    assert!(actual[..4].iter().all(Option::is_none));
    assert!(actual[4].is_some());
}
