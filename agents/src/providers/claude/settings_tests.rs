use super::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "uses local auth; creates one fixed-prompt test session, wakes it, changes its settings and deletes it"]
fn settings_real_claude_session() {
    struct Cleanup(PathBuf, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = Command::new("claude").args(["stop", &self.1[..8]]).output();
            let mut provider = ClaudeProvider::new(self.0.clone());
            if let Some(session) = provider
                .refresh(1)
                .sessions
                .into_iter()
                .find(|session| session.id.id == self.1)
            {
                provider.delete(&session).unwrap()().unwrap();
            }
        }
    }
    fn wait(provider: &mut ClaudeProvider, id: &str, ready: impl Fn(&Session) -> bool) -> Session {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(session) = provider
                .refresh(1)
                .sessions
                .into_iter()
                .find(|session| session.id.id == id)
                && ready(&session)
            {
                return session;
            }
            assert!(
                Instant::now() < deadline,
                "Claude settings were not published"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let mut provider = ClaudeProvider::from_config().unwrap();
    let choices = provider.input_choices();
    let sonnet = choices.model("sonnet").unwrap().id.clone();
    let opus = choices.model("opus").unwrap().id.clone();
    let id = format!(
        "00000000-0000-4000-8000-{:012x}",
        Utc::now().timestamp_millis()
    );
    let _cleanup = Cleanup(provider.config_dir.clone(), id.clone());
    let output = Command::new("claude")
        .args([
            "-p",
            "--session-id",
            &id,
            "--model",
            &sonnet,
            "--effort",
            "medium",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--",
            "Reply READY only. Do not use tools.",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let inactive = wait(&mut provider, &id, |session| {
        session.status == SessionStatus::Inactive
    });
    provider
        .send(&inactive, SessionInput::SetModel(opus.clone()))
        .unwrap()()
    .unwrap();
    let live = wait(&mut provider, &id, |session| {
        session.is_live_background() && session.model.as_deref() == Some(&opus)
    });
    provider
        .send(&live, SessionInput::SetEffort("high".into()))
        .unwrap()()
    .unwrap();
    let live = wait(&mut provider, &id, |session| {
        session.effort.as_deref() == Some("high")
    });
    provider
        .send(&live, SessionInput::SetModel(sonnet.clone()))
        .unwrap()()
    .unwrap();
    wait(&mut provider, &id, |session| {
        session.is_live_background() && session.model.as_deref() == Some(&sonnet)
    });
    println!(
        "{{\"verdict\":\"pass\",\"inactive_wakes\":true,\"live_model\":true,\"live_effort\":true}}"
    );
}
