use serde_json::Value;
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

#[path = "support/tmux.rs"]
mod tmux;

fn capture_status(tmux: &tmux::Tmux, status: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        if capture
            .lines()
            .any(|line| line.contains("wait") && line.contains(status))
        {
            return capture;
        }
        assert!(
            Instant::now() < deadline,
            "missing wait {status}: {capture}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn terminal_finished_wait_updates_from_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    let id = "4165651d-b9e3-458d-8612-f8114e312363";
    let children = project.join(id).join("subagents");
    fs::create_dir_all(&children).unwrap();
    fs::create_dir_all(config.join("sessions")).unwrap();
    let codex = directory.path().join("codex");
    fs::create_dir(&codex).unwrap();
    fs::write(
        project.join(format!("{id}.jsonl")),
        include_str!("fixtures/transcript.jsonl"),
    )
    .unwrap();
    fs::write(
        children.join("agent-wait.jsonl"),
        include_str!("fixtures/claude-finished-wait.jsonl"),
    )
    .unwrap();
    let metadata_path = children.join("agent-wait.meta.json");
    let mut metadata: Value =
        serde_json::from_str(include_str!("fixtures/claude-finished-wait.meta.json")).unwrap();
    metadata["finished"] = false.into();
    fs::write(&metadata_path, metadata.to_string()).unwrap();
    let mut registry: Value = serde_json::from_str(include_str!("fixtures/registry.json")).unwrap();
    let started = Command::new("ps")
        .env("TZ", "UTC")
        .args(["-p", &std::process::id().to_string(), "-o", "lstart="])
        .output()
        .unwrap();
    assert!(started.status.success());
    registry["pid"] = std::process::id().into();
    registry["procStart"] = String::from_utf8(started.stdout).unwrap().trim().into();
    registry["sessionId"] = id.into();
    registry["cwd"] = directory.path().to_str().unwrap().into();
    registry["status"] = "idle".into();
    fs::write(config.join("sessions/probe.json"), registry.to_string()).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = std::path::Path::new(&binary).canonicalize().unwrap();
    let tmux = tmux::Tmux::new(format!("agents-finished-wait-{}", std::process::id()));
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
        &format!("CLAUDE_CONFIG_DIR={}", config.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    let working = capture_status(&tmux, "Working");
    metadata["finished"] = true.into();
    fs::write(&metadata_path, metadata.to_string()).unwrap();
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let inactive = capture_status(&tmux, "Inactive");
    metadata["finished"] = false.into();
    fs::write(&metadata_path, metadata.to_string()).unwrap();
    tmux.run(&["send-keys", "-t", "probe", "BTab"]);
    let reverted = capture_status(&tmux, "Working");
    let verdict = serde_json::json!({
        "verdict": "pass", "working": working, "inactive": inactive,
        "reverted": reverted, "metadata_only_roundtrip": true,
    });
    let path = std::env::var_os("AGENTS_FINISHED_WAIT_VERDICT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-finished-wait-verdict.json"));
    fs::write(path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
}
