use super::*;
use crate::providers::SessionInput;
use serde_json::{Value, json};
use std::{
    process::Stdio,
    time::{Duration, Instant},
};

struct Cleanup {
    home: PathBuf,
    id: String,
    active: bool,
}
impl Cleanup {
    fn finish(&mut self) {
        let mut client = app_server::Client::connect(&self.home).unwrap();
        app_server::thread::delete(&mut client, &self.id).expect("throwaway cleanup failed");
        self.active = false;
        let db = Connection::open_with_flags(
            self.home.join("state_5.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let remaining: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE id=?1",
                [&self.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0, "throwaway thread must be deleted");
        eprintln!("deleted throwaway Codex thread {}", self.id);
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let result = app_server::Client::connect(&self.home)
            .and_then(|mut client| app_server::thread::delete(&mut client, &self.id));
        if let Err(error) = result {
            eprintln!("throwaway cleanup failed for {}: {error}", self.id);
        }
    }
}

#[test]
#[ignore = "uses local auth and the real shared Codex daemon; creates, sends, steers, interrupts and deletes its own thread"]
fn send_real_codex_thread() {
    let provider = CodexProvider::from_config().unwrap();
    let mut owner = app_server::Client::connect(&provider.home).unwrap();
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
    let event = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|event| event["type"] == "thread.started")
        .expect("thread created");
    let id = event["thread_id"].as_str().unwrap().to_owned();
    let mut cleanup = Cleanup {
        home: provider.home.clone(),
        id: id.clone(),
        active: true,
    };
    assert!(output.status.success(), "codex exec failed");
    let db = Connection::open_with_flags(
        provider.home.join("state_5.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let session = db
        .query_row("SELECT * FROM threads WHERE id=?1", [&id], |row| {
            super::session(row, &HashMap::new())
        })
        .unwrap()
        .unwrap();
    for input in [
        SessionInput::SetModel("haiku".into()),
        SessionInput::SetEffort("high".into()),
    ] {
        assert!(provider.send(&session, input).is_none());
    }
    let prompt =
        "agents-send-idle-中文\nDo not use tools. Write a detailed 10000-word essay about rivers.";
    std::thread::spawn(
        provider
            .send(&session, SessionInput::Prompt(prompt.into()))
            .expect("supports prompt"),
    )
    .join()
    .unwrap()
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let turn = loop {
        if let Some(turn) = app_server::thread::running_turn(&mut owner, &id).unwrap() {
            break turn;
        }
        assert!(Instant::now() < deadline, "turn never became active");
        std::thread::sleep(Duration::from_millis(100));
    };
    let steer = "agents-send-steer-中文\nContinue the essay; do not use tools.";
    std::thread::spawn(
        provider
            .send(&session, SessionInput::Prompt(steer.into()))
            .unwrap(),
    )
    .join()
    .unwrap()
    .unwrap();
    assert_eq!(
        app_server::thread::running_turn(&mut owner, &id).unwrap(),
        Some(turn.clone())
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    let items = loop {
        let items: Value = owner
            .request(
                "thread/items/list",
                json!({"threadId": id, "turnId": turn, "limit": 100}),
            )
            .unwrap();
        let messages: Vec<_> = items["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| &entry["item"])
            .filter(|item| item["type"] == "userMessage")
            .flat_map(|item| item["content"].as_array().unwrap())
            .filter_map(|content| content["text"].as_str())
            .collect();
        if messages.contains(&prompt) && messages.contains(&steer) {
            break items;
        }
        assert!(Instant::now() < deadline, "sent messages absent: {items}");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(
        items["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| &entry["item"])
            .filter(|item| item["type"] == "userMessage")
            .count(),
        2
    );
    assert_eq!(
        app_server::thread::running_turn(&mut owner, &id).unwrap(),
        Some(turn)
    );
    std::thread::spawn(provider.send(&session, SessionInput::Interrupt).unwrap())
        .join()
        .unwrap()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while app_server::thread::running_turn(&mut owner, &id)
        .unwrap()
        .is_some()
    {
        assert!(Instant::now() < deadline, "interrupt did not stop turn");
        std::thread::sleep(Duration::from_millis(100));
    }

    cleanup.finish();
}
