//! Real daemon and Codex PTY lifecycle; this probe never starts a model turn.
use super::*;
use crate::session_terminal::SessionTerminal;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

struct DeleteThread {
    home: PathBuf,
    id: String,
}

impl Drop for DeleteThread {
    fn drop(&mut self) {
        let result = app_server::Client::connect(&self.home)
            .and_then(|mut client| app_server::thread::delete(&mut client, &self.id));
        eprintln!("delete throwaway {}: {result:?}", self.id);
        assert!(result.is_ok(), "throwaway deletion failed");
    }
}

fn session(provider: &CodexProvider, id: &str) -> Session {
    let mut snapshot = provider.discover_sessions(10_000).unwrap();
    snapshot.sessions.retain(|session| session.id.id == id);
    provider
        .enrich_sessions(snapshot)
        .unwrap()
        .sessions
        .into_iter()
        .find(|session| session.id.id == id)
        .expect("throwaway remains in the actual dashboard list")
}

fn wait_status(provider: &CodexProvider, id: &str, status: SessionStatus) -> Session {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let found = session(provider, id);
        if found.status == status {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {status:?}, got {:?}",
            found.status
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_loaded(provider: &CodexProvider, id: &str) -> Session {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let found = session(provider, id);
        if found.process.is_some() {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "terminal did not load its thread"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn terminal(provider: &CodexProvider, session: &Session) -> SessionTerminal {
    let cleanup = provider.terminal_cleanup(session).unwrap();
    let TerminalCommand { command, detach } = provider.terminal_command(session).unwrap();
    let mut terminal =
        SessionTerminal::spawn(session.id.clone(), command, detach, (40, 120), || {}).unwrap();
    terminal.cleanup = cleanup;
    terminal
}

fn close(mut terminal: SessionTerminal) {
    let cleanup = terminal.cleanup.take();
    std::thread::spawn(move || {
        terminal.close().unwrap();
        if let Some(cleanup) = cleanup {
            cleanup().unwrap();
        }
    })
    .join()
    .unwrap();
}

#[test]
#[ignore = "creates and deletes its own real Codex thread; opens real PTYs; no model turns"]
fn idle_terminal_archives_real_codex_thread_without_hiding_it() {
    let provider = CodexProvider::from_config().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut client = app_server::Client::connect(&provider.home).unwrap();
    let started: Value = client.request("thread/start", json!({
        "cwd":workspace.path(), "approvalPolicy":"on-request", "sandbox":"read-only",
        "ephemeral":false, "serviceName":"agents-terminal-idle-probe"
    })).unwrap();
    let id = started["thread"]["id"].as_str().unwrap().to_owned();
    let cleanup = DeleteThread {
        home: provider.home.clone(),
        id: id.clone(),
    };
    let rollout = PathBuf::from(started["thread"]["path"].as_str().unwrap());
    eprintln!(
        "created own thread {id}; rollout exists={}",
        rollout.exists()
    );
    app_server::thread::archive(&mut client, &id).unwrap();
    let mut results = Vec::new();
    for archived in [false, true] {
        if !archived {
            app_server::thread::unarchive(&mut client, &id).unwrap();
        }
        let initial = wait_status(&provider, &id, SessionStatus::Inactive);
        assert_eq!(initial.archived, archived);
        let terminal = terminal(&provider, &initial);
        // Empty histories have no turn-ended marker, so the dashboard labels them Working.
        let loaded = wait_loaded(&provider, &id);
        assert!(
            app_server::thread::running_turn(&mut client, &id)
                .unwrap()
                .is_none()
        );
        let started_close = Instant::now();
        close(terminal);
        let after = wait_status(&provider, &id, SessionStatus::Inactive);
        let elapsed = started_close.elapsed().as_millis();
        assert!(after.archived);
        assert!(elapsed < 5_000, "idle cleanup took {elapsed} ms");
        results.push(
            json!({"initially_archived":archived,"loaded_status":format!("{:?}",loaded.status),"close_ms":elapsed,"inactive":true,"listed":true}),
        );
    }
    app_server::thread::unarchive(&mut client, &id).unwrap();
    app_server::thread::resume(&mut client, &id).unwrap();
    let loaded = wait_loaded(&provider, &id);
    let opened_loaded = terminal(&provider, &loaded);
    close(opened_loaded);
    assert_eq!(session(&provider, &id).status, loaded.status);
    assert!(!session(&provider, &id).archived);
    assert!(
        app_server::thread::running_turn(&mut client, &id)
            .unwrap()
            .is_none()
    );
    drop(client);
    drop(cleanup);
    let db = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM threads WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    assert!(!rollout.exists());
    if let Some(path) = std::env::var_os("AGENTS_TERMINAL_IDLE_VERDICT") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"passed":true,"thread":id,"cases":results,"loaded_preserved":true,"model_turns":0,"deleted":true})).unwrap()).unwrap();
    }
}
