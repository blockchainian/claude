use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::net::UnixListener,
    time::{Duration, Instant},
};

fn fixture() -> (tempfile::TempDir, CodexProvider, Session) {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(
        &path,
        include_str!("../../../tests/fixtures/codex-thread-clock.jsonl"),
    )
    .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=? WHERE id='thread-0'",
        [path.to_str().unwrap()],
    )
    .unwrap();
    let mut provider = CodexProvider::new(dir.path().into(), dir.path().join("manager"));
    let session = provider
        .refresh(2)
        .sessions
        .into_iter()
        .find(|s| s.id.id == "thread-0")
        .unwrap();
    (dir, provider, session)
}

#[test]
fn terminal_cleanup_only_owns_threads_opened_inactive() {
    let (_dir, provider, mut session) = fixture();
    for archived in [false, true] {
        session.archived = archived;
        for status in [
            SessionStatus::Inactive,
            SessionStatus::Ready,
            SessionStatus::Working,
        ] {
            session.status = status;
            assert_eq!(
                provider.terminal_cleanup(&session).unwrap().is_some(),
                status == SessionStatus::Inactive,
                "{status:?}, archived={archived}"
            );
        }
    }
}

#[test]
fn terminal_cleanup_archives_without_hiding_the_thread() {
    for (archived, running) in [(false, false), (true, false), (false, true)] {
        let (dir, mut provider, mut session) = fixture();
        session.archived = archived;
        let cleanup = provider
            .terminal_cleanup(&session)
            .unwrap()
            .expect("inactive terminal needs cleanup");
        // Unarchive can move the rollout; the open file must still identify its contents.
        let moved = dir.path().join("resumed.jsonl");
        fs::rename(session.transcript.as_ref().unwrap(), &moved).unwrap();
        let control = dir.path().join("app-server-control");
        fs::create_dir(&control).unwrap();
        let listener = UnixListener::bind(control.join("app-server-control.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let home = dir.path().to_owned();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(Instant::now() < deadline, "cleanup did not connect");
                std::thread::sleep(Duration::from_millis(10));
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            loop {
                let message = socket.read().unwrap();
                let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                let result = match request["method"].as_str().unwrap() {
                    "initialize" => json!({"userAgent":"test-daemon"}),
                    "initialized" => continue,
                    "thread/turns/list" => {
                        if running {
                            json!({"data":[{"id":"turn","status":"inProgress"}]})
                        } else {
                            json!({"data":[]})
                        }
                    }
                    "thread/archive" => {
                        assert_eq!(request["params"], json!({"threadId":"thread-0"}));
                        Connection::open(home.join("state_5.sqlite"))
                            .unwrap()
                            .execute("UPDATE threads SET archived=1 WHERE id='thread-0'", [])
                            .unwrap();
                        json!({})
                    }
                    other => panic!("unexpected request {other}"),
                };
                socket
                    .send(tungstenite::Message::Text(
                        json!({"id":request["id"],"result":result})
                            .to_string()
                            .into(),
                    ))
                    .unwrap();
                if request["method"] == "thread/archive"
                    || (running && request["method"] == "thread/turns/list")
                {
                    break;
                }
            }
        });
        cleanup().unwrap();
        server.join().unwrap();
        let sessions = provider.refresh(2).sessions;
        assert_eq!(sessions.len(), 14);
        let session = sessions
            .iter()
            .find(|s| s.id.id == "thread-0")
            .expect("archiving preserves the list entry");
        assert_eq!(session.archived, !running);
        assert_eq!(session.status, SessionStatus::Inactive);
    }
}

#[test]
fn terminal_cleanup_preserves_any_thread_that_started_a_turn() {
    let records: Vec<Value> = include_str!("../../../tests/fixtures/codex-thread-clock.jsonl")
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let started = records
        .iter()
        .find(|r| r["payload"]["type"] == "task_started")
        .unwrap();
    let ended = records
        .iter()
        .find(|r| r["payload"]["type"] == "task_complete")
        .unwrap();
    for change in [
        "started",
        "completed",
        "partial",
        "truncated",
        "rewritten",
        "invalid",
    ] {
        let (_dir, provider, session) = fixture();
        let cleanup = provider
            .terminal_cleanup(&session)
            .unwrap()
            .expect("inactive terminal needs cleanup");
        let path = session.transcript.as_ref().unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        match change {
            "started" => writeln!(file, "{started}").unwrap(),
            "completed" => writeln!(file, "{started}\n{ended}").unwrap(),
            "partial" => file.write_all(b"{\"type\":").unwrap(),
            "truncated" => file.set_len(0).unwrap(),
            "rewritten" => {
                let len = file.metadata().unwrap().len() as usize;
                fs::write(path, " ".repeat(len)).unwrap();
            }
            "invalid" => writeln!(file, "invalid JSON").unwrap(),
            _ => unreachable!(),
        }
        let result = cleanup();
        if change == "invalid" {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
        } else {
            // No daemon is available: attempting to archive would fail this assertion.
            result.unwrap();
        }
    }
}
