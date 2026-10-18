use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_navigation_and_opening_keys() {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("rollout.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../fixtures/codex-command-failed.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute_batch("DELETE FROM threads WHERE id NOT IN ('thread-12', 'thread-13'); DELETE FROM thread_spawn_edges; INSERT INTO thread_spawn_edges VALUES ('thread-12','thread-13','completed');").unwrap();
    db.execute("UPDATE threads SET rollout_path=?1, cwd=?2, archived=0, name=CASE id WHEN 'thread-12' THEN 'Example task 12' ELSE 'Example task 13' END", rusqlite::params![transcript.to_str().unwrap(), directory.path().to_str().unwrap()]).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux(format!("agents-task-navigation-{}", std::process::id()));
    tmux.run(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "200",
        "-y",
        "50",
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.wait_for(|s| s.contains("Inactive 2"));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let capture =
        tmux.capture_when(|s| s.contains("Example task 12") && s.contains("Example task 13"));
    let row = capture
        .lines()
        .position(|line| line.contains("Example task 12") && line.contains("Inactive"))
        .unwrap();
    tmux.mouse(0, 5, row, true);
    tmux.wait_for(|s| s.contains("Example task 12 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "Down"]);
    tmux.wait_for(|s| s.contains("Example task 13 ❮"));
    for key in ["Enter", "Right", "C-l", "o"] {
        tmux.run(&["send-keys", "-t", "probe", key]);
        tmux.wait_for(|s| s.contains("Terminal is not supported"));
        // A harmless key clears the notice so each opening key has its own assertion.
        tmux.run(&["send-keys", "-t", "probe", "Left"]);
        tmux.wait_for(|s| !s.contains("Terminal is not supported"));
    }
    tmux.run(&["send-keys", "-t", "probe", "/"]);
    tmux.wait_for(|s| s.contains("Search ❯"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|s| !s.contains("Search ❯") && s.contains("Example task 13 ❮"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    let capture =
        tmux.capture_when(|s| s.contains("Example task 12") && s.contains("Example task 13"));
    let row = capture
        .lines()
        .position(|line| line.contains("Example task 12") && line.contains("Inactive"))
        .unwrap();
    tmux.mouse(0, 5, row, true);
    tmux.wait_for(|s| s.contains("Example task 12 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "?"]);
    tmux.wait_for(|s| s.contains("Previous") && s.contains("→/⌃l"));
    println!(
        "{}",
        serde_json::json!({"task_history": "passed", "opening_keys": "passed", "search_escape": "passed", "help": "passed"})
    );
}
