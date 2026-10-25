use std::{path::Path, time::Duration};

#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;

fn capture_when(tmux: &Tmux, ready: impl Fn(&str) -> bool) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        if ready(&capture) {
            return capture;
        }
        assert!(std::time::Instant::now() < deadline, "{capture}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn task_list_unknown_keys_and_help() {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("rollout.jsonl");
    std::fs::write(
        &transcript,
        include_str!("fixtures/codex-command-failed.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("fixtures/codex-state.sql"))
        .unwrap();
    db.execute_batch("DELETE FROM threads WHERE id NOT IN ('thread-12', 'thread-13'); DELETE FROM thread_spawn_edges; INSERT INTO thread_spawn_edges VALUES ('thread-12','thread-13','completed');").unwrap();
    db.execute("UPDATE threads SET rollout_path=?1, cwd=?2, archived=0, name=CASE id WHEN 'thread-12' THEN 'Example task 12' ELSE 'Example task 13' END", rusqlite::params![transcript.to_str().unwrap(), directory.path().to_str().unwrap()]).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-unknown-keys-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "120",
        "-y",
        "40",
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    capture_when(&tmux, |s| s.contains("Inactive 2"));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    capture_when(&tmux, |s| {
        s.contains("Example task 12") && s.contains("Example task 13")
    });
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    let before = capture_when(&tmux, |s| s.contains("Example task 12 ❯"));
    for key in ["h", "z"] {
        tmux.run(&["send-keys", "-t", "probe", key, "?"]);
        let help = capture_when(&tmux, |s| s.contains("Shortcuts"));
        assert!(!help.contains("Hide"), "{help}");
        assert!(
            help.contains("Archive") && help.contains("Delete"),
            "{help}"
        );
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        let after = capture_when(&tmux, |s| s.contains("Example task 12 ❯"));
        assert!(after.contains("Example task 13"), "{after}");
        if let Some(output) = std::env::var_os("AGENTS_PROBE_OUTPUT") {
            std::fs::write(Path::new(&output).join(format!("{key}-help.txt")), help).unwrap();
            std::fs::write(Path::new(&output).join(format!("{key}-after.txt")), after).unwrap();
        }
    }
    if let Some(output) = std::env::var_os("AGENTS_PROBE_OUTPUT") {
        std::fs::write(Path::new(&output).join("before.txt"), before).unwrap();
        std::fs::write(Path::new(&output).join("verdict.json"), "{\"verdict\":\"pass\",\"unknown_keys\":[\"h\",\"z\"],\"selection_preserved\":true,\"tasks_preserved\":true,\"help_has_no_hide\":true}").unwrap();
    }
}
