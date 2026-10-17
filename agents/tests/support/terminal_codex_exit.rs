use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_exit_sections() {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("failed.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../fixtures/codex-command-failed.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute("UPDATE threads SET rollout_path=?1, name='Exit status probe', cwd=?2, archived=0 WHERE id='thread-0'",
        rusqlite::params![transcript.to_str().unwrap(), directory.path().to_str().unwrap()]).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux(format!("agents-exit-status-{}", std::process::id()));
    tmux.run(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "160",
        "-y",
        "45",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    std::thread::sleep(Duration::from_secs(2));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    std::thread::sleep(Duration::from_secs(1));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let path = std::env::temp_dir().join("agents-codex-exit-tmux.txt");
    std::fs::write(&path, &capture).unwrap();
    assert!(capture.contains("false → 1"), "{capture}");
    let rows: Vec<_> = capture.lines().collect();
    let status = rows
        .iter()
        .position(|row| row.contains("exit code: 1"))
        .expect(&capture);
    assert!(rows[status - 2].contains("false"), "{capture}");
    assert_eq!(rows[status - 1].trim(), "↓", "{capture}");
    assert_eq!(rows[status + 1].trim(), "↓", "{capture}");
    assert!(rows[status + 2].contains("[scrubbed]"), "{capture}");
    let arrows: Vec<_> = [status - 1, status + 1]
        .map(|row| rows[row].find('↓').unwrap())
        .into();
    assert_eq!(arrows[0], arrows[1]);
    println!(
        "{{\"verdict\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}
