//! Recursive notifications for the registry and transcript trees.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

pub fn watch(
    config_dir: &Path,
    mut changed: impl FnMut(notify::Result<notify::Event>) + Send + 'static,
) -> notify::Result<RecommendedWatcher> {
    let sessions = config_dir.join("sessions");
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&sessions)?;
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
            Ok(event) if relevant(&event) => changed(Ok(event)),
            Ok(_) => {}
            Err(error) => changed(Err(error)),
        })?;
    watcher.watch(&sessions, RecursiveMode::Recursive)?;
    watcher.watch(&config_dir.join("projects"), RecursiveMode::Recursive)?;
    Ok(watcher)
}

fn relevant(event: &notify::Event) -> bool {
    !matches!(event.kind, EventKind::Access(_))
        && (event.paths.is_empty()
            || event.paths.iter().any(|path| {
                path.file_name().is_none_or(|name| name != "agents.json")
                    && path
                        .extension()
                        .is_none_or(|extension| extension != "key" && extension != "tmp")
            }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
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
        let fixture = include_str!("../../tests/fixtures/transcript.jsonl");
        fs::write(&path, fixture).unwrap();
        let (tx, rx) = mpsc::channel();
        let _watcher = match watch(config.path(), move |event| {
            let _ = tx.send(event);
        }) {
            Ok(watcher) => watcher,
            Err(error) => {
                eprintln!("Skipping filesystem watcher test: {error}");
                return;
            }
        };
        assert_eq!(
            fs::metadata(config.path().join("sessions"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
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
}
