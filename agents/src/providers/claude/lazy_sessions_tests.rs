use super::*;

fn children(parent: &Path, count: usize) -> Vec<PathBuf> {
    let directory = parent.with_extension("").join("subagents");
    fs::create_dir_all(&directory).unwrap();
    (0..count)
        .map(|index| {
            let path = directory.join(format!("agent-{index:03}.jsonl"));
            fs::write(
                &path,
                include_str!("../../../tests/fixtures/subagent.jsonl"),
            )
            .unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(
                    SystemTime::UNIX_EPOCH
                        + std::time::Duration::from_secs(1_700_000_100 + index as u64),
                )
                .unwrap();
            path
        })
        .collect()
}

#[test]
fn inactive_subagents_share_the_page_and_only_selected_summaries_are_read() {
    let directory = tempfile::tempdir().unwrap();
    let id = "00000000-0000-0000-0000-000000000001";
    let parent = super::tests::transcript(directory.path(), id, 0);
    let paths = children(&parent, 30);
    let mut provider = ClaudeProvider::new(directory.path().into());
    let snapshot = provider.refresh(1);
    assert_eq!(snapshot.sessions.len(), PAGE_SIZE);
    assert!(snapshot.has_more);
    assert_eq!(provider.summaries.len(), PAGE_SIZE);
    for (session, expected) in snapshot.sessions.iter().zip(paths.iter().rev()) {
        assert_eq!(session.transcript.as_ref(), Some(expected));
        assert_eq!(session.parent.as_ref().map(|key| key.id.as_str()), Some(id));
        assert_eq!(session.status, SessionStatus::Inactive);
    }
    assert!(!provider.summaries.contains_key(&parent));
    let snapshot = provider.refresh(2);
    assert_eq!(snapshot.sessions.len(), PAGE_SIZE * 2);
    assert_eq!(provider.summaries.len(), PAGE_SIZE * 2);
}

#[test]
fn live_parent_descendants_are_retained_beyond_the_inactive_page() {
    let directory = tempfile::tempdir().unwrap();
    let entry = registry::tests::fixture(directory.path());
    let id = entry["sessionId"].as_str().unwrap();
    let parent = super::tests::transcript(directory.path(), id, 0);
    fs::write(
        directory.path().join("sessions/live.json"),
        entry.to_string(),
    )
    .unwrap();
    for path in children(&parent, 30) {
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::UNIX_EPOCH)
            .unwrap();
    }
    for index in 0..15 {
        super::tests::transcript(
            directory.path(),
            &format!("10000000-0000-0000-0000-{index:012}"),
            index,
        );
    }
    let mut provider = ClaudeProvider::new(directory.path().into());
    let snapshot = provider.refresh(1);
    assert_eq!(snapshot.sessions.len(), 1 + 30 + PAGE_SIZE);
    assert!(snapshot.has_more);
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .filter(|session| session.parent.is_some())
            .count(),
        30
    );
    assert_eq!(provider.summaries.len(), 1 + 30 + PAGE_SIZE);
}

#[test]
fn recent_live_parent_finished_children_consume_history_slots_once() {
    let directory = tempfile::tempdir().unwrap();
    let entry = registry::tests::fixture(directory.path());
    let id = entry["sessionId"].as_str().unwrap();
    let parent = super::tests::transcript(directory.path(), id, 0);
    fs::write(
        directory.path().join("sessions/live.json"),
        entry.to_string(),
    )
    .unwrap();
    children(&parent, 30);
    for index in 0..15 {
        super::tests::transcript(
            directory.path(),
            &format!("10000000-0000-0000-0000-{index:012}"),
            index,
        );
    }
    let mut provider = ClaudeProvider::new(directory.path().into());
    let snapshot = provider.refresh(1);
    assert_eq!(snapshot.sessions.len(), 31);
    assert_eq!(
        snapshot
            .sessions
            .iter()
            .filter(|session| session.status == SessionStatus::Inactive)
            .count(),
        30
    );
    assert_eq!(provider.summaries.len(), 31);
    assert!(snapshot.has_more);
    let selected: HashSet<_> = snapshot
        .sessions
        .iter()
        .map(|session| &session.id)
        .collect();
    assert_eq!(selected.len(), snapshot.sessions.len());
}
