use crate::providers::codex::*;
use serde_json::{Value, json};
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

struct Cleanup(String, PathBuf, bool);
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.2 {
            return;
        }
        let mut client = super::Client::connect(&self.1).unwrap();
        super::thread::delete(&mut client, &self.0).expect("throwaway cleanup failed");
    }
}

/// Creates a throwaway thread with `codex exec` and deletes it on drop.
fn throwaway_thread(provider: &CodexProvider) -> (String, Cleanup) {
    // Check before exec: never let exec start a missing daemon for this test.
    let socket = provider
        .home
        .join("app-server-control/app-server-control.sock");
    std::os::unix::net::UnixStream::connect(socket)
        .expect("Codex app-server daemon must already be running");
    let output = Command::new("codex")
        .args([
            "exec",
            "--ignore-user-config",
            "--skip-git-repo-check",
            "--json",
            "Reply OK only. Do not use tools.",
        ])
        .env("CODEX_HOME", &provider.home)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let event: Value = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|v| v["type"] == "thread.started")
        .expect("exec must create a thread");
    let id = event["thread_id"].as_str().unwrap().to_owned();
    let cleanup = Cleanup(id.clone(), provider.home.clone(), true);
    assert!(output.status.success(), "codex exec failed");
    (id, cleanup)
}

fn stored_session(provider: &CodexProvider, id: &str) -> Session {
    let db = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    db.query_row("SELECT * FROM threads WHERE id=?1", [id], |row| {
        super::super::session(row, &HashMap::new())
    })
    .unwrap()
    .unwrap()
}

#[test]
#[ignore = "requires the running shared Codex daemon and local auth; creates and deletes a real throwaway thread"]
fn rename_real_codex_thread() {
    let provider = CodexProvider::from_config().unwrap();
    let (id, _cleanup) = throwaway_thread(&provider);
    let session = stored_session(&provider, &id);
    assert!(provider.capabilities().rename);
    provider
        .rename(&session, "  agents-rpc-integration  ")
        .unwrap();
    let db = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let name: String = db
        .query_row("SELECT name FROM threads WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(name, "agents-rpc-integration");
    let index = std::fs::read_to_string(provider.home.join("session_index.jsonl")).unwrap();
    let line = index
        .lines()
        .rev()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|v| v["id"] == id)
        .unwrap();
    assert_eq!(line["thread_name"], name);
    let mut client = super::Client::connect(&provider.home).unwrap();
    super::thread::set_name(&mut client, &id, "agents-rpc-integration").unwrap();
    assert_eq!(
        super::thread::set_name(&mut client, &id, "   ")
            .unwrap_err()
            .to_string(),
        "thread name must not be empty"
    );
}

#[test]
#[ignore = "requires the running shared Codex daemon and local auth; starts, interrupts, archives and deletes a real throwaway thread"]
fn stop_and_archive_real_codex_thread() {
    stop_and_archive_thread(false);
}

#[test]
#[ignore = "requires the shared Codex daemon and local auth; interrupts, archives and deletes a manager-linked throwaway thread"]
fn operate_real_manager_thread() {
    stop_and_archive_thread(true);
}

fn stop_and_archive_thread(manager: bool) {
    let mut provider = CodexProvider::from_config().unwrap();
    let manager_dir = tempfile::tempdir().unwrap();
    let state = manager_dir.path().join("state.json");
    let manager_bytes =
        include_bytes!("../../../../tests/fixtures/codex-manager/attached-state.json");
    std::fs::write(&state, manager_bytes).unwrap();
    provider.manager_home = manager_dir.path().into();
    let (id, mut cleanup) = throwaway_thread(&provider);
    let mut session = stored_session(&provider, &id);
    if manager {
        session.parent = Some(SessionKey {
            provider: ProviderId::Claude,
            id: "claude-parent".into(),
        });
    }
    assert!(provider.capabilities().stop && provider.capabilities().archive);

    // Load the thread into the daemon and start a long turn on it.
    let mut owner = super::Client::connect(&provider.home).unwrap();
    let _: Value = owner
        .request("thread/resume", json!({"threadId": id}))
        .unwrap();
    let started: Value = owner
        .request(
            "turn/start",
            json!({
                "threadId": id,
                "input": [{"type": "text", "text": "Do not use tools. Write a 3000-word essay about rivers."}],
            }),
        )
        .unwrap();
    let turn = started["turn"]["id"].as_str().unwrap().to_owned();

    // Right after turn/start the daemon can still report the turn as interrupted;
    // wait until it reports the turn running, as a user pressing `x` would see it.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut client = super::Client::connect(&provider.home).unwrap();
        if super::thread::running_turn(&mut client, &id).unwrap() == Some(turn.clone()) {
            break;
        }
        assert!(Instant::now() < deadline, "turn never reported running");
        std::thread::sleep(Duration::from_millis(100));
    }
    provider.stop(&session).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        let mut client = super::Client::connect(&provider.home).unwrap();
        let turns: Value = client
            .request(
                "thread/turns/list",
                json!({"threadId": id, "limit": 1, "sortDirection": "desc"}),
            )
            .unwrap();
        let latest = &turns["data"][0];
        assert_eq!(latest["id"], turn);
        if latest["status"] != "inProgress" || Instant::now() > deadline {
            break latest["status"].clone();
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(status, "interrupted");
    // No running turn: stop does nothing.
    provider.stop(&session).unwrap();
    drop(owner);

    provider.archive(&session).unwrap();
    let archived: bool = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row("SELECT archived FROM threads WHERE id=?1", [&id], |row| {
        row.get(0)
    })
    .unwrap();
    assert!(archived);
    assert!(
        !provider
            .snapshot(1)
            .unwrap()
            .sessions
            .iter()
            .any(|candidate| candidate.id == session.id)
    );
    assert_eq!(std::fs::read(&state).unwrap(), manager_bytes);
    provider.delete(&session).unwrap();
    cleanup.2 = false;
    assert_eq!(std::fs::read(&state).unwrap(), manager_bytes);
}

#[test]
fn delete_uses_thread_rpc_without_signalling_the_writer() {
    use std::os::unix::net::UnixListener;
    use tungstenite::Message;

    for live in [false, true] {
        let home = tempfile::tempdir().unwrap();
        Connection::open(home.path().join("state_5.sqlite"))
            .unwrap()
            .execute_batch(include_str!("../../../../tests/fixtures/codex-state.sql"))
            .unwrap();
        let provider = CodexProvider::new(home.path().into(), home.path().join("manager"));
        let mut session = stored_session(&provider, "thread-0");
        if live {
            // The writer may be a shared daemon: its identity must not affect deletion.
            session.process = Some(Process {
                pid: std::process::id(),
                label: Some("codex".into()),
                started_at: None,
            });
        }
        let control = home.path().join("app-server-control");
        std::fs::create_dir(&control).unwrap();
        let listener = UnixListener::bind(control.join("app-server-control.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return false;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let initialize: Value =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(initialize["method"], "initialize");
            socket
                .send(Message::text(
                    json!({"id": initialize["id"], "result": {"userAgent": "codex"}}).to_string(),
                ))
                .unwrap();
            let initialized: Value =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(initialized, json!({"method": "initialized"}));
            let request: Value =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                request,
                json!({"id": 2, "method": "thread/delete", "params": {"threadId": "thread-0"}})
            );
            // Recorded from the live daemon deleting our throwaway thread.
            let mut response: Value = serde_json::from_str(include_str!(
                "../../../../tests/fixtures/codex-delete-response.json"
            ))
            .unwrap();
            response["id"] = request["id"].clone();
            socket.send(Message::text(response.to_string())).unwrap();
            true
        });
        let result = provider.delete(&session);
        let received = server.join().unwrap();
        assert!(received, "delete did not reach thread/delete: {result:?}");
        result.unwrap();
    }
}

#[test]
#[ignore = "requires the shared Codex daemon and local auth; creates and deletes idle, running and archived throwaway threads"]
fn delete_real_codex_sessions() {
    let provider = CodexProvider::from_config().unwrap();
    for (active, archived) in [(false, false), (true, false), (false, true)] {
        let (id, mut cleanup) = throwaway_thread(&provider);
        let mut owner = super::Client::connect(&provider.home).unwrap();
        let _: Value = owner
            .request("thread/resume", json!({"threadId": id}))
            .unwrap();
        if active {
            let started: Value = owner.request("turn/start", json!({
                "threadId": id,
                "input": [{"type": "text", "text": "Do not use tools. Write a detailed 10000-word essay about rivers."}],
                "effort": "low",
            })).unwrap();
            assert_eq!(started["turn"]["status"], "inProgress");
        }
        let mut session = stored_session(&provider, &id);
        let lock = provider
            .home
            .join("thread-writer-locks")
            .join(format!("{id}.lock"));
        assert!(lock_held(&lock).unwrap());
        let pid = live_writers(&provider.home.join("thread-writer-locks")).unwrap()[&id];
        let start = crate::processes::start_time(pid).expect("writer must be alive");
        session.process = Some(Process {
            pid,
            label: Some("codex".into()),
            started_at: Some(start),
        });
        let rollout = session.transcript.clone().unwrap();
        assert!(rollout.exists());
        if archived {
            provider.archive(&session).unwrap();
            let path: String = Connection::open_with_flags(
                provider.home.join("state_5.sqlite"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap()
            .query_row(
                "SELECT rollout_path FROM threads WHERE id=?1",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
            session.transcript = Some(PathBuf::from(path));
        }
        let deleted_rollout = session.transcript.clone().unwrap();
        provider.delete(&session).unwrap();
        cleanup.2 = false;
        assert!(!rollout.exists());
        assert!(!deleted_rollout.exists());
        assert!(!lock_held(&lock).unwrap());
        assert!(!lock.exists());
        let loaded: Value = owner.request("thread/loaded/list", json!({})).unwrap();
        assert!(
            !loaded["data"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loaded| loaded == &id)
        );
        assert_eq!(
            crate::processes::start_time(pid),
            Some(start),
            "shared daemon must survive deletion"
        );
        let count: i64 = Connection::open_with_flags(
            provider.home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
        .query_row("SELECT COUNT(*) FROM threads WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            provider.delete(&session).unwrap_err().to_string(),
            format!("no rollout found for thread id {id}")
        );
    }
}
