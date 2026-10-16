use super::*;
use crate::providers::{ProviderId, SessionKey, claude::ClaudeProvider};
use std::fs;
use std::time::{Duration, SystemTime};

fn config(count: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/project");
    fs::create_dir_all(&project).unwrap();
    for index in 0..count {
        let path = project.join(format!("00000000-0000-0000-0000-{index:012}.jsonl"));
        fs::write(
            &path,
            include_str!("../../../tests/fixtures/transcript.jsonl"),
        )
        .unwrap();
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(index as u64))
            .unwrap();
    }
    dir
}

#[test]
fn empty_hub_has_no_sessions_or_more_pages() {
    let snapshot = Hub::new(Vec::new()).refresh();
    assert!(snapshot.sessions.is_empty());
    assert!(!snapshot.has_more);
}

#[test]
fn hub_pages_each_provider_and_merges_by_time() {
    let first = config(21);
    let second = config(1);
    let mut hub = Hub::new(vec![
        Box::new(ClaudeProvider::new(second.path().into())),
        Box::new(ClaudeProvider::new(first.path().into())),
    ]);
    let snapshot = hub.refresh();
    assert_eq!(snapshot.sessions.len(), 11);
    assert!(snapshot.has_more);
    assert_eq!(snapshot.sessions[0].updated_at.timestamp(), 20);
    assert!(
        snapshot
            .sessions
            .windows(2)
            .all(|pair| pair[0].updated_at >= pair[1].updated_at)
    );
    hub.show_more();
    assert_eq!(hub.refresh().sessions.len(), 21);
    hub.show_more();
    let snapshot = hub.refresh();
    assert_eq!(snapshot.sessions.len(), 22);
    assert!(!snapshot.has_more);
}

#[test]
fn merge_orders_live_then_inactive_then_children_and_drops_orphans() {
    let dir = config(7);
    let mut snapshot = ClaudeProvider::new(dir.path().into()).refresh(1);
    let key = |id: &str| SessionKey {
        provider: ProviderId::Claude,
        id: id.into(),
    };
    // Start with sessions parsed from the real transcript fixture.
    let mut live = snapshot.sessions.remove(0);
    live.status = SessionStatus::Ready;
    live.updated_at = SystemTime::UNIX_EPOCH.into();
    let mut later_live = snapshot.sessions.remove(0);
    later_live.status = SessionStatus::Working;
    later_live.updated_at = live.updated_at;
    let mut child = snapshot.sessions.remove(0);
    child.parent = Some(live.id.clone());
    let mut earlier_child = snapshot.sessions.remove(0);
    earlier_child.parent = Some(later_live.id.clone());
    let mut orphan = snapshot.sessions.remove(0);
    orphan.parent = Some(key("missing"));
    let mut descendant = snapshot.sessions.remove(0);
    descendant.parent = Some(orphan.id.clone());
    let inactive = snapshot.sessions.remove(0);
    let expected = vec![
        later_live.id.clone(),
        live.id.clone(),
        inactive.id.clone(),
        earlier_child.id.clone(),
        child.id.clone(),
    ];
    let result = merge(
        [
            Snapshot {
                sessions: vec![child, orphan, live],
                has_more: false,
                inactive_total: 0,
            },
            Snapshot {
                sessions: vec![descendant, inactive, earlier_child, later_live],
                has_more: true,
                inactive_total: 1,
            },
        ]
        .into_iter(),
    );
    assert_eq!(
        result
            .sessions
            .iter()
            .map(|session| session.id.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(result.has_more);
}

#[test]
fn single_provider_hub_preserves_snapshot() {
    let dir = config(12);
    let expected = ClaudeProvider::new(dir.path().into()).refresh(1);
    let actual = Hub::new(vec![Box::new(ClaudeProvider::new(dir.path().into()))]).refresh();
    assert_eq!(actual.sessions, expected.sessions);
    assert_eq!(actual.has_more, expected.has_more);
}
