use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;

#[test]
#[ignore = "requires tmux, AGENTS_BRANCH_TRANSCRIPT and AGENTS_PROBE_BIN"]
fn installed_background_branch_border() {
    let transcript = PathBuf::from(std::env::var_os("AGENTS_BRANCH_TRANSCRIPT").unwrap());
    let binary = PathBuf::from(std::env::var_os("AGENTS_PROBE_BIN").unwrap());
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::copy(&transcript, project.join(transcript.file_name().unwrap())).unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    let tmux = Tmux::new(format!("agents-branch-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "149",
        "-y",
        "50",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", config.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CLAUDE_CODE_TMPDIR={}", directory.path().display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut selected = false;
    loop {
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        if !selected && capture.contains("Inactive 1") {
            tmux.run(&["send-keys", "-t", "probe", "Tab"]);
            selected = true;
        }
        if selected
            && capture
                .lines()
                .any(|row| row.contains("─") && row.trim_end().ends_with("example.com╶─"))
        {
            let result = serde_json::json!({"passed":true,"binary":binary,"transcript":transcript,"capture":capture});
            if let Some(path) = std::env::var_os("AGENTS_BRANCH_VERDICT") {
                std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
            }
            return;
        }
        assert!(
            Instant::now() < deadline,
            "branch absent from footer: {capture}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
