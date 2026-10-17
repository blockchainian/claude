use super::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn filters_preferences_temporary_keys_and_read_access() {
    use notify::event::AccessKind;
    let event = |path: &str| notify::Event::new(EventKind::Any).add_path(path.into());
    for path in ["sessions/a.key", "sessions/a.tmp", "projects/p/agents.json"] {
        assert!(!relevant(&event(path)));
    }
    assert!(relevant(&event("projects/p/session.jsonl")));
    assert!(relevant(&notify::Event::new(EventKind::Other)));
    assert!(!relevant(
        &notify::Event::new(EventKind::Access(AccessKind::Any))
            .add_path("projects/p/session.jsonl".into())
    ));
    assert!(relevant(
        &event("sessions/a.tmp").add_path("sessions/a.json".into())
    ));
}

#[test]
fn appended_transcript_notifies_within_two_seconds() {
    let config = tempfile::tempdir().unwrap();
    let project = config.path().join("projects/probe");
    fs::create_dir_all(&project).unwrap();
    let path = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    let fixture = include_str!("../../../tests/fixtures/transcript.jsonl");
    fs::write(&path, fixture).unwrap();
    let (tx, rx) = mpsc::channel();
    let _watcher = match watch(std::slice::from_ref(&project), move |event| {
        let _ = tx.send(event);
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("Skipping filesystem watcher test: {error}");
            return;
        }
    };
    // Drain any directory-creation notifications before the measured append.
    while rx.try_recv().is_ok() {}
    let start = Instant::now();
    let line = fixture.lines().nth(2).unwrap();
    writeln!(
        OpenOptions::new().append(true).open(&path).unwrap(),
        "{line}"
    )
    .unwrap();
    let expected = path.canonicalize().unwrap();
    loop {
        let event = rx
            .recv_timeout(Duration::from_secs(2).saturating_sub(start.elapsed()))
            .expect("no append notification within 2 s")
            .unwrap();
        if event
            .paths
            .iter()
            .any(|path| path.canonicalize().is_ok_and(|path| path == expected))
        {
            break;
        }
    }
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn manager_lock_churn_does_not_refresh_sessions() {
    // Paths and event kinds captured from the running manager's FSEvents stream.
    for path in [
        "manager/session/thread.jsonl.lock",
        "manager/session/thread.jsonl.lock/owner",
        "manager/session/state.json.lock",
    ] {
        for kind in [
            EventKind::Create(notify::event::CreateKind::Folder),
            EventKind::Remove(notify::event::RemoveKind::Folder),
            EventKind::Modify(notify::event::ModifyKind::Metadata(
                notify::event::MetadataKind::Extended,
            )),
        ] {
            assert!(
                !relevant(&notify::Event::new(kind).add_path(path.into())),
                "{path}"
            );
        }
    }
    for path in [
        "manager/session/state.json",
        "manager/session/thread.ask.request.json",
        "codex/thread-writer-locks/thread.lock",
        "codex/sessions/rollout.jsonl",
    ] {
        assert!(
            relevant(&notify::Event::new(EventKind::Any).add_path(path.into())),
            "{path}"
        );
    }
    assert!(relevant(
        &notify::Event::new(EventKind::Any)
            .add_path("manager/session/state.json.lock".into())
            .add_path("manager/session/state.json".into())
    ));
}
