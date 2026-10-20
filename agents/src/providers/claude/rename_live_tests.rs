//! Opt-in rename probe against a disposable Claude background job.
use super::*;
use crate::app::live_tests as dashboard;
use std::time::{Duration, Instant};

#[test]
#[ignore = "starts and removes one real Haiku background job"]
fn rename_real_background_session() {
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for operation in ["stop", "rm"] {
                let output = Command::new("claude")
                    .args([operation, &self.0])
                    .output()
                    .expect("Claude cleanup command");
                assert!(
                    output.status.success(),
                    "cleanup {operation}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
    let cwd = std::env::var_os("AGENTS_RENAME_CWD")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let output = Command::new("claude")
        .args([
            "--bg",
            "--model",
            "haiku",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--",
            "Reply exactly READY. Do not use tools.",
        ])
        .current_dir(&cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let job = created_job(&output).unwrap();
    let cleanup = Cleanup(job.clone());
    let config = config_dir();
    let state_path = config.join("jobs").join(&job).join("state.json");
    let state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let id = state["sessionId"]
        .as_str()
        .expect("Claude job sessionId")
        .to_owned();
    let preferences = tempfile::tempdir().unwrap();
    let mut provider = ClaudeProvider::new(config.clone());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if provider
            .refresh(1)
            .sessions
            .iter()
            .any(|session| session.id.id == id && session.is_live_background())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "created background session was not registered: {state}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut app = dashboard::app(Box::new(provider), preferences.path().into());
    let session = dashboard::session(&app, &id);
    let name = format!("agents rename {} 中文 \"quote\" $HOME ; /exit", &job);
    dashboard::send(
        &mut app,
        session.id.clone(),
        SessionInput::Rename(name.clone()),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state: serde_json::Value =
            serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        let registry: serde_json::Value = serde_json::from_slice(
            &fs::read(
                config
                    .join("sessions")
                    .join(format!("{}.json", session.process.as_ref().unwrap().pid)),
            )
            .unwrap(),
        )
        .unwrap();
        if state["name"] == name && registry["name"] == name {
            let screen = attach::inspect(&job, &cwd).unwrap();
            assert!(
                screen
                    .lines()
                    .any(|line| line.contains('─') && line.contains(&name)),
                "Claude terminal title: {screen}"
            );
            let listing = Command::new("claude")
                .args(["agents", "--json"])
                .output()
                .unwrap();
            assert!(listing.status.success());
            let listing: serde_json::Value = serde_json::from_slice(&listing.stdout).unwrap();
            assert!(
                listing
                    .as_array()
                    .expect("Claude agents JSON array")
                    .iter()
                    .any(|entry| entry["name"] == name),
                "renamed session missing from Claude agents --json"
            );
            dashboard::refresh(&mut app);
            assert_eq!(
                dashboard::session(&app, &id).name.as_deref(),
                Some(name.as_str())
            );
            println!(
                "{}",
                serde_json::json!({"verdict":"pass", "job":job,"name":name,"state":true,"registry":true,"listing":true,"reattached_screen":screen})
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Claude did not publish rename: {state}, {registry}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(cleanup);
}
