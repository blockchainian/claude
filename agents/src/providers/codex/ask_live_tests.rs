//! Real daemon and dashboard question flow; no simulated server or model responses.
use super::*;
use crate::app::live_tests as dashboard;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

struct Archive {
    home: PathBuf,
    id: String,
}
impl Drop for Archive {
    fn drop(&mut self) {
        let result = app_server::Client::connect(&self.home).and_then(|mut client| {
            app_server::thread::stop(&mut client, &self.id)?;
            app_server::thread::archive(&mut client, &self.id)
        });
        eprintln!("archive {}: {result:?}", self.id);
        assert!(result.is_ok(), "throwaway thread cleanup failed");
    }
}

#[test]
#[ignore = "uses local auth and shared Codex daemon; asks and answers on a real gpt-6-luna low thread, then archives it"]
fn answer_real_codex_question_in_dashboard() {
    let configured = CodexProvider::from_config().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut owner = app_server::Client::connect(&configured.home).unwrap();
    let started: Value = owner
        .request(
            "thread/start",
            json!({
                "cwd": workspace.path(), "approvalPolicy": "on-request", "sandbox": "read-only",
                "ephemeral": false, "serviceName": "agents-ask-test", "model": "gpt-6-luna",
                "config": {"model_reasoning_effort": "low"}
            }),
        )
        .unwrap();
    let id = started["thread"]["id"].as_str().unwrap().to_owned();
    let _archive = Archive {
        home: configured.home.clone(),
        id: id.clone(),
    };
    eprintln!("throwaway thread {id}");
    let rollout = PathBuf::from(started["thread"]["path"].as_str().unwrap());
    let _: Value = owner.request("turn/start", json!({
        "threadId": id, "effort": "low",
        "input": [{"type":"text", "text":"Use the request_user_input tool to ask me one question with id color, header Color, question 'Pick a color?', options Red and Blue. After I answer, reply with exactly: GOT <answer>. Do nothing else."}]
    })).unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let summary = loop {
        let summary = rollout::summary(&rollout).unwrap();
        if summary.pending_question_call.is_some() {
            break summary;
        }
        assert!(Instant::now() < deadline, "model did not ask a question");
        std::thread::sleep(Duration::from_millis(100));
    };
    let (call, _) = summary.pending_question_call.unwrap();
    let original = owner
        .wait_question(&id, &call, Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(original.params["itemId"], call);
    eprintln!(
        "confirmed itemId == rollout call_id: {call}; request id {}",
        original.id
    );
    drop(owner);

    // Restrict dashboard discovery to this one real thread while using the shared daemon.
    let home = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        configured.home.join("app-server-control"),
        home.path().join("app-server-control"),
    )
    .unwrap();
    let db = Connection::open(home.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute(
        "UPDATE threads SET id=?1,rollout_path=?2,cwd=?3,archived=0 WHERE id='thread-0'",
        rusqlite::params![
            id,
            rollout.to_str().unwrap(),
            workspace.path().to_str().unwrap()
        ],
    )
    .unwrap();
    drop(db);
    let preferences = tempfile::tempdir().unwrap();
    let provider = CodexProvider::new(home.path().into());
    let mut app = dashboard::app(Box::new(provider), preferences.path().into());
    let session = dashboard::session(&app, &id);
    assert_eq!(session.ask.as_ref().unwrap().id, call);
    // Build a second provider instance to prove the answer survives a dashboard restart.
    let restarted = CodexProvider::new(home.path().into());
    let replayed = restarted.snapshot(1).unwrap().sessions.remove(0);
    assert_eq!(replayed.ask, session.ask);
    dashboard::action(
        &mut app,
        Action::PersistQuestion {
            id: session.id.clone(),
            ask: Some(crate::state::StoredAsk {
                id: call.clone(),
                page: 0,
                answers: Default::default(),
            }),
        },
    );
    let saved: Value =
        serde_json::from_slice(&std::fs::read(preferences.path().join("agents.json")).unwrap())
            .unwrap();
    assert_eq!(
        saved["state"]["tasks"][session.id.to_string()]["ask"]["id"],
        call
    );
    dashboard::answer_questions(&mut app, session, vec![vec!["Blue".into()]]);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(preferences.path().join("agents.json")).unwrap())
            .unwrap();
    assert!(
        saved["state"]["tasks"][format!("codex:{id}")]
            .get("ask")
            .is_none()
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let summary = rollout::summary(&rollout).unwrap();
        if summary.turn_ended {
            assert_eq!(summary.last_message.as_deref(), Some("GOT Blue"));
            assert!(summary.pending_question_call.is_none());
            break;
        }
        assert!(Instant::now() < deadline, "answered turn did not finish");
        std::thread::sleep(Duration::from_millis(100));
    }
    dashboard::refresh(&mut app);
    assert!(dashboard::session(&app, &id).ask.is_none());
    // Ended turns do not replay a stale question.
    let mut client = app_server::Client::connect(&configured.home).unwrap();
    assert!(
        app_server::thread::question(&mut client, &id, &call)
            .unwrap()
            .is_none()
    );
    eprintln!(
        "dashboard keyboard answer accepted; matching function_call_output; final GOT Blue; ended turn has no replay"
    );
    drop(_archive);
    let db = Connection::open_with_flags(
        configured.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let archived: bool = db
        .query_row("SELECT archived FROM threads WHERE id=?1", [&id], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(archived);
    if let Some(path) = std::env::var_os("AGENTS_ASK_VERDICT") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"passed":true,"thread":id,"call":call,"model":"gpt-6-luna","effort":"low","restart":true,"dashboard_keyboard":true,"rollout_output":true,"final":"GOT Blue","ended_no_replay":true,"draft_cleared":true,"archived":true})).unwrap()).unwrap();
    }
}
