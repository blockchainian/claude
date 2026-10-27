use super::*;
use crate::event::{Reader as _, SourceRecord};

#[test]
fn pinned_records_restore_real_events_before_refresh() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for fixture in [
        "claude-tool-calls.jsonl",
        "claude-tool-details.jsonl",
        "claude-prompt-images.jsonl",
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
fn pinned_invalid_records_do_not_discard_other_groups() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-tool-calls.jsonl");
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

#[test]
fn task_notifications_have_a_source_and_restore_from_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for fixture in [
        "claude-background-notifications.jsonl",
        "claude-task-notifications.jsonl",
    ] {
        let path = root.join(fixture);
        let notifications: Vec<_> = tail(&path, usize::MAX)
            .unwrap()
            .into_iter()
            .filter(|event| {
                event
                    .calls
                    .iter()
                    .any(|call| call.name == "task-notification")
            })
            .collect();
        assert!(!notifications.is_empty(), "{fixture}");
        let groups: Vec<_> = notifications
            .iter()
            .map(|event| event.source_records.clone())
            .collect();
        let restored = Reader::new(path, false).read_events(&groups).unwrap();
        for (event, restored) in notifications.iter().zip(restored) {
            assert!(
                event.source.as_deref().is_some_and(|source| !source.is_empty()),
                "{fixture}: {}",
                event.body
            );
            assert_eq!(restored.unwrap().source, event.source, "{fixture}");
        }
    }
}
