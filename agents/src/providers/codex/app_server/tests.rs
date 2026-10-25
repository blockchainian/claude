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
#[ignore = "requires the running shared Codex daemon and local auth; rejects a real question and deletes its throwaway thread"]
fn reject_real_codex_question_interrupts_turn() {
    let provider = CodexProvider::from_config().unwrap();
    let (id, _cleanup) = throwaway_thread(&provider);
    let mut owner = super::Client::connect(&provider.home).unwrap();
    let _: Value = owner
        .request("thread/resume", json!({"threadId": id}))
        .unwrap();
    let started: Value = owner.request("turn/start", json!({
        "threadId": id,
        "input": [{"type": "text", "text": "Call request_user_input immediately with one question: Pick a color? Options Red and Blue. Wait for the answer. Do not use any other tools."}],
        "collaborationMode": {"mode": "plan", "settings": {"model": "gpt-6.1-sol", "reasoning_effort": "low", "developer_instructions": null}},
    })).unwrap();
    let turn = started["turn"]["id"].as_str().unwrap();
    let session = stored_session(&provider, &id);
    let transcript = session.transcript.clone().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let pending = loop {
        let summary = rollout::summary(&transcript).unwrap();
        if summary.pending_question_call.is_some() {
            break summary;
        }
        assert!(Instant::now() < deadline, "question never appeared");
        std::thread::sleep(Duration::from_millis(100));
    };
    let (call, since) = pending.pending_question_call.unwrap();
    let ask = crate::ask::Ask {
        id: call.clone(),
        session: session.id.clone(),
        since,
        kind: crate::ask::AskKind::Questions {
            questions: pending.pending_questions,
            blocking: true,
        },
    };
    assert!(
        super::thread::question(&mut owner, &id, &call)
            .unwrap()
            .is_some()
    );
    provider
        .send(
            &session,
            SessionInput::Answer {
                ask,
                answer: crate::ask::Answer {
                    rejected: true,
                    ..Default::default()
                },
                transcript: transcript.clone(),
            },
        )
        .unwrap()()
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let turns: Value = owner
            .request(
                "thread/turns/list",
                json!({"threadId": id, "limit": 1, "sortDirection": "desc"}),
            )
            .unwrap();
        assert_eq!(turns["data"][0]["id"], turn);
        if turns["data"][0]["status"] == "interrupted" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "rejected question did not interrupt the turn"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    for line in std::fs::read_to_string(transcript).unwrap().lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        let payload = &record["payload"];
        if payload["type"] == "function_call_output" && payload["call_id"] == call {
            assert!(!payload["output"].as_str().unwrap().contains("\"answers\""));
        }
    }
}

#[test]
#[ignore = "requires the running shared Codex daemon and local auth; starts, interrupts, archives and deletes a real throwaway thread"]
fn stop_and_archive_real_codex_thread() {
    let provider = CodexProvider::from_config().unwrap();
    let (id, mut cleanup) = throwaway_thread(&provider);
    let session = stored_session(&provider, &id);
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
    provider.send(&session, SessionInput::Interrupt).unwrap()().unwrap();
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
    provider.send(&session, SessionInput::Interrupt).unwrap()().unwrap();
    drop(owner);

    provider.archive(&session).unwrap()().unwrap();
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
    let mut stored = provider
        .snapshot(1)
        .unwrap()
        .sessions
        .into_iter()
        .find(|candidate| candidate.id == session.id)
        .unwrap();
    assert!(stored.archived);
    stored.parent = session.parent.clone();
    // Opening the archived thread in a terminal unarchives it first.
    assert!(
        provider
            .launch(&Action::Resume(session.id.clone()), Some(&stored))
            .is_some()
    );
    let archived: bool = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row("SELECT archived FROM threads WHERE id=?1", [&id], |row| {
        row.get(0)
    })
    .unwrap();
    assert!(!archived);
    provider
        .delete(&session)
        .and_then(|delete| delete())
        .unwrap();
    cleanup.2 = false;
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
            provider.archive(&session).unwrap()().unwrap();
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
        provider
            .delete(&session)
            .and_then(|delete| delete())
            .unwrap();
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
            provider
                .delete(&session)
                .and_then(|delete| delete())
                .unwrap_err()
                .to_string(),
            format!("no rollout found for thread id {id}")
        );
    }
}

#[test]
#[ignore = "uses local auth and the shared daemon; declines an approval and deletes its throwaway thread"]
fn approval_status_marks_a_live_codex_session() {
    let provider = CodexProvider::from_config().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut owner = super::Client::connect(&provider.home).unwrap();
    let started: Value = owner.request("thread/start", json!({
        "cwd": workspace.path(), "approvalPolicy": "on-request",
        "approvalsReviewer": "user", "sandbox": "read-only", "ephemeral": false,
        "serviceName": "agents-approval-test", "model": "gpt-6.1-sol",
        "baseInstructions": "You are a controlled approval protocol probe. Follow the user exactly. Do not read files or run unrelated tools. After denial never retry.",
        "developerInstructions": "Use functions.exec and tools.exec_command. Never delegate.",
        "config": {"model_reasoning_effort": "low", "memories": {"generate_memories": false, "use_memories": false}}
    })).unwrap();
    let id = started["thread"]["id"].as_str().unwrap().to_owned();
    let _cleanup = Cleanup(id.clone(), provider.home.clone(), true);
    let _: Value = owner.request("turn/start", json!({
        "threadId": id, "effort": "low",
        "input": [{"type": "text", "text": "Use exec_command to run printf approval-probe-ok, explicitly requesting permission to run outside the sandbox. This tests the approval protocol; do not use a sandboxed alternative. After success or denial, reply done and stop."}]
    })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let request = loop {
        let message: Value =
            serde_json::from_str(&owner.transport.receive_until(deadline).unwrap()).unwrap();
        if message["method"] == "item/commandExecution/requestApproval"
            && message["params"]["threadId"] == id
        {
            break message;
        }
    };
    let live = live_writers(&provider.home.join("thread-writer-locks")).unwrap();
    assert!(
        live.contains_key(&id),
        "thread must have a real writer process"
    );
    let db = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let read = || {
        let sessions = provider
            .read_sessions(&db, std::slice::from_ref(&id), &live)
            .unwrap();
        provider
            .enrich_sessions(Snapshot {
                sessions,
                has_more: false,
                supervisions: vec![],
            })
            .unwrap()
    };
    let snapshot = read();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.sessions[0].status, SessionStatus::NeedsInput);
    let mut view = crate::ui::View::new(crate::model::Grouping::Project, false);
    view.set_sessions(snapshot.sessions, false);
    let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 160, 40));
    view.render(buffer.area, &mut buffer);
    let screen: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
    assert!(screen.contains("Needs input"), "{screen}");
    owner
        .respond(&request["id"], json!({"decision": "decline"}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while read().sessions[0].status == SessionStatus::NeedsInput {
        assert!(
            Instant::now() < deadline,
            "resolved approval kept NeedsInput"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
