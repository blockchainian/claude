mod tmux {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/tmux.rs"
    ));
}
use tmux::Tmux;

use super::*;

#[test]
fn settings_inputs_are_supported_for_inactive_and_active_threads() {
    let directory = tempfile::tempdir().unwrap();
    Connection::open(directory.path().join("state_5.sqlite"))
        .unwrap()
        .execute_batch(include_str!("../../../tests/fixtures/codex-state.sql"))
        .unwrap();
    let mut provider =
        CodexProvider::new(directory.path().into());
    let mut session = provider
        .refresh(1)
        .sessions
        .into_iter()
        .find(|session| session.can_resume())
        .unwrap();
    for status in [
        SessionStatus::Inactive,
        SessionStatus::Ready,
        SessionStatus::Working,
    ] {
        session.status = status;
        for input in [
            SessionInput::SetModel("gpt-6.1-sol".into()),
            SessionInput::SetEffort("high".into()),
        ] {
            assert!(
                provider.send(&session, input).is_some(),
                "{status:?} settings must be supported"
            );
        }
    }
}

#[test]
#[ignore = "uses the real shared Codex daemon; creates an empty thread, changes settings and deletes it"]
fn settings_real_codex_thread() {
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    struct Cleanup(PathBuf, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let mut client = app_server::Client::connect(&self.0).unwrap();
            app_server::thread::delete(&mut client, &self.1).unwrap();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let mut provider = CodexProvider::from_config().unwrap();
    let choices = provider.input_choices();
    let models: Vec<_> = choices
        .models
        .iter()
        .filter(|model| model.efforts.iter().any(|effort| effort == "high"))
        .take(2)
        .collect();
    assert_eq!(models.len(), 2, "two cached models supporting high effort");
    let mut owner = app_server::Client::connect(&provider.home).unwrap();
    let created: Value = owner
        .request(
            "thread/start",
            json!({"cwd":directory.path(), "model":models[0].id}),
        )
        .unwrap();
    let id = created["thread"]["id"].as_str().unwrap().to_owned();
    let _cleanup = Cleanup(provider.home.clone(), id.clone());
    fn archive(owner: &mut app_server::Client, id: &str) {
        app_server::thread::archive(owner, id).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let loaded: Value = owner.request("thread/loaded/list", json!({})).unwrap();
            if !loaded["data"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loaded| loaded == id)
            {
                break;
            }
            assert!(Instant::now() < deadline, "thread did not unload");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    archive(&mut owner, &id);
    for (expected_status, input, model, effort) in [
        (
            SessionStatus::Inactive,
            SessionInput::SetModel(models[1].id.clone()),
            models[1].id.as_str(),
            None,
        ),
        (
            SessionStatus::Ready,
            SessionInput::SetModel(models[0].id.clone()),
            models[0].id.as_str(),
            None,
        ),
        (
            SessionStatus::Ready,
            SessionInput::SetEffort("high".into()),
            models[0].id.as_str(),
            Some("high"),
        ),
    ] {
        let session = provider
            .refresh(1)
            .sessions
            .into_iter()
            .find(|session| session.id.id == id)
            .unwrap();
        if expected_status == SessionStatus::Inactive {
            assert_eq!(session.status, expected_status);
        } else {
            assert_ne!(session.status, SessionStatus::Inactive);
        }
        provider.send(&session, input).unwrap()().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state: Value = owner
                .request("thread/resume", json!({"threadId":id,"excludeTurns":true}))
                .unwrap();
            if state["model"] == model
                && effort.is_none_or(|effort| state["reasoningEffort"] == effort)
            {
                break;
            }
            assert!(Instant::now() < deadline, "settings not applied: {state}");
            std::thread::sleep(Duration::from_millis(50));
        }
        let refreshed = provider
            .refresh(1)
            .sessions
            .into_iter()
            .find(|session| session.id.id == id)
            .unwrap();
        assert_ne!(refreshed.status, SessionStatus::Inactive);
        assert_eq!(refreshed.model.as_deref(), Some(model));
        if let Some(effort) = effort {
            assert_eq!(refreshed.effort.as_deref(), Some(effort));
        }
        let turns: Value = owner
            .request(
                "thread/turns/list",
                json!({"threadId":id,"limit":1,"sortDirection":"desc"}),
            )
            .unwrap();
        assert!(
            turns["data"].as_array().unwrap().is_empty(),
            "settings must not start a turn"
        );
    }
    app_server::thread::set_name(&mut owner, &id, &format!("settings-{id}")).unwrap();
    archive(&mut owner, &id);
    terminal_settings_walk(&provider.home, &id, &models[0].label, &models[1].label);
    println!(
        "{{\"verdict\":\"pass\",\"inactive_wakes\":true,\"active_model\":true,\"active_effort\":true,\"turns\":0}}"
    );
}

use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

impl Tmux {
    fn wait(&self, ready: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let screen = self.run(&["capture-pane", "-t", "probe", "-p"]);
            if ready(&screen) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "terminal settings deadline: {screen}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn click(&self, text: &str, border: bool) {
        let screen = self.wait(|screen| {
            screen
                .lines()
                .any(|line| line.contains('─') == border && line.contains(text))
        });
        let (row, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains('─') == border && line.contains(text))
            .unwrap();
        let column = line[..line.find(text).unwrap()].width();
        let click = format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column + 1,
            row + 1,
            column + 1,
            row + 1
        );
        self.run(&["send-keys", "-t", "probe", "-l", &click]);
    }
}

fn terminal_settings_walk(home: &Path, id: &str, before: &str, after: &str) {
    let directory = tempfile::tempdir().unwrap();
    let claude = directory.path().join("claude");
    std::fs::create_dir(&claude).unwrap();
    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("agents");
    let tmux = Tmux::new(format!("agents-settings-{id}"));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "200",
        "-y",
        "50",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_HOME={}", home.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.wait(|screen| screen.contains("Inactive"));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    tmux.run(&["send-keys", "-t", "probe", "/"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", &format!("settings-{id}")]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.click(before, true);
    tmux.click(after, false);
    tmux.wait(|screen| {
        screen
            .lines()
            .next()
            .is_some_and(|line| line.contains('[') || line.contains("Active"))
            && screen
                .lines()
                .any(|line| line.contains('─') && line.contains(after))
    });
    tmux.click("Active", false);
    tmux.wait(|screen| {
        screen
            .lines()
            .any(|line| line.contains('─') && line.contains(after))
    });
    tmux.click("high", true);
    tmux.click("medium", false);
    tmux.wait(|screen| {
        screen
            .lines()
            .any(|line| line.contains('─') && line.contains(after) && line.contains("medium"))
    });
    tmux.click(after, true);
    tmux.click(before, false);
    let screen = tmux.wait(|screen| {
        screen
            .lines()
            .any(|line| line.contains('─') && line.contains(before) && line.contains("medium"))
    });
    if let Some(path) = std::env::var_os("AGENTS_SETTINGS_CAPTURE") {
        std::fs::write(path, screen).unwrap();
    }
}
