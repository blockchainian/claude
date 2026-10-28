use std::path::Path;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;

impl Tmux {
    fn wait_for(&self, ready: impl Fn(&str) -> bool) {
        self.capture_when(ready);
    }

    fn capture_when(&self, ready: impl Fn(&str) -> bool) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let capture = self.run(&["capture-pane", "-t", "probe", "-p"]);
            if ready(&capture) {
                return capture;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "terminal did not become ready: {capture}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn mouse(&self, button: u16, column: usize, row: usize, click: bool) {
        let press = format!("\x1b[<{button};{};{}M", column + 1, row + 1);
        let release = if click {
            format!("\x1b[<{button};{};{}m", column + 1, row + 1)
        } else {
            String::new()
        };
        self.run(&[
            "send-keys",
            "-t",
            "probe",
            "-l",
            &format!("{press}{release}"),
        ]);
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_spawn_agent_claude_receipt_and_model() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    // Use the same recorded catalog as the provider's model display tests.
    std::fs::write(
        config.join(".claude.json"),
        r#"{"oauthAccount":{"organizationUuid":"org-1","accountUuid":"ACCOUNT-1"}}"#,
    )
    .unwrap();
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(b"account-1")
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let catalog = config.join("cache/model-catalog");
    std::fs::create_dir_all(&catalog).unwrap();
    std::fs::write(
        catalog.join(format!("org-1-{hash}-cc.json")),
        include_str!("fixtures/claude-model-catalog.json"),
    )
    .unwrap();
    let transcript = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(
        &transcript,
        format!(
            "{}\n{}",
            include_str!("fixtures/transcript.jsonl"),
            include_str!("fixtures/claude-agent-launch.jsonl")
        ),
    )
    .unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-spawn-settings-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "240",
        "-y",
        "60",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", config.display()),
        "-e",
        &format!("CLAUDE_CODE_TMPDIR={}", directory.path().display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Inactive 1"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let capture = tmux.capture_when(|capture| capture.contains("Spawn agent 查 Claude 前台转后台"));
    let (row, line) = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("Spawn agent 查 Claude 前台转后台"))
        .last()
        .unwrap();
    let column = line[..line.find("Spawn agent 查 Claude 前台转后台").unwrap()].width();
    tmux.mouse(0, column, row, true);
    let capture =
        tmux.capture_when(|capture| capture.contains("查 Claude 前台转后台 · Sonnet 5.5"));
    assert!(
        !capture.contains("Async agent launched successfully"),
        "{capture}"
    );
    assert!(!capture.contains("agentId:"), "{capture}");
    assert!(!capture.contains("Default"), "{capture}");
    let capture_path = std::env::temp_dir().join("agents-spawn-settings-tmux.txt");
    std::fs::write(&capture_path, &capture).unwrap();
    let verdict = serde_json::json!({"verdict":"pass","binary":binary,"capture":capture_path,"checks":["description name","resolved model","no default effort","hidden launch receipt"]});
    std::fs::write(
        std::env::temp_dir().join("agents-spawn-settings-probe.json"),
        verdict.to_string(),
    )
    .unwrap();
    println!("{verdict}");
}
