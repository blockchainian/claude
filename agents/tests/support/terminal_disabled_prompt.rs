use super::*;

#[test]
fn terminal_disabled_prompt_keeps_label_and_dim_marker() {
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
    db.execute(
        "DELETE FROM threads WHERE id NOT IN ('thread-0', 'thread-1')",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?1, cwd=?2, archived=0",
        rusqlite::params![
            transcript.to_str().unwrap(),
            directory.path().to_str().unwrap()
        ],
    )
    .unwrap();
    db.execute(
        "UPDATE threads SET title='升级命令中心', name='升级命令中心' WHERE id='thread-0'",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO thread_spawn_edges VALUES ('thread-1', 'thread-0', 'completed')",
        [],
    )
    .unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-disabled-prompt-{}", std::process::id()));
    tmux.run(&[
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
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Inactive 2"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    tmux.wait_for(|capture| capture.contains("Example thread 1 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "Down"]);
    let expected = "升级命令中心 ❮";
    tmux.wait_for(|capture| capture.contains(expected));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&capture, 160, 45);
    let screen = parser.screen();
    let (row, line) = screen
        .contents()
        .lines()
        .enumerate()
        .find_map(|(row, line)| line.contains(expected).then(|| (row, line.to_owned())))
        .unwrap();
    let column = line[..line.find(expected).unwrap()].width();
    assert_eq!(line.trim(), expected);
    let marker = column + "升级命令中心 ".width();
    assert_eq!(
        screen.cell(row as u16, marker as u16).unwrap().contents(),
        "❮"
    );
    use unicode_segmentation::UnicodeSegmentation;
    let mut x = column;
    for grapheme in expected.graphemes(true) {
        let cell = screen.cell(row as u16, x as u16).unwrap();
        assert_eq!(cell.contents(), grapheme);
        if x < marker - 1 {
            assert!(cell.bold(), "label cell ({x}, {row}): {capture}");
            assert!(!cell.dim(), "label cell ({x}, {row}): {capture}");
        } else {
            assert!(cell.dim(), "marker cell ({x}, {row}): {capture}");
        }
        x += grapheme.width();
    }
    let path = std::env::temp_dir().join("agents-disabled-prompt-tmux.txt");
    std::fs::write(&path, &capture).unwrap();
    let verdict = std::env::temp_dir().join("agents-disabled-prompt-verdict.json");
    std::fs::write(
        &verdict,
        serde_json::json!({
            "verdict": "pass",
            "disabled_prompt": "pass",
            "dim_marker": "pass",
            "capture": path,
        })
        .to_string(),
    )
    .unwrap();
    println!("{}", verdict.display());
}

#[test]
fn terminal_foreground_prompt_shows_only_bg() {
    let (directory, tmux, _transcript) = start_probe("foreground-prompt");
    let config = directory.path().join("claude");
    let mut registry: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/registry.json")).unwrap();
    let pid = std::process::id();
    let start = Command::new("ps")
        .env("TZ", "UTC")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .unwrap();
    assert!(start.status.success());
    registry["sessionId"] = "4165651d-b9e3-458d-8612-f8114e312363".into();
    registry["pid"] = pid.into();
    registry["procStart"] = String::from_utf8(start.stdout).unwrap().trim().into();
    registry["cwd"] = directory.path().to_str().unwrap().into();
    registry["kind"] = "interactive".into();
    std::fs::create_dir_all(config.join("sessions")).unwrap();
    std::fs::write(
        config.join(format!("sessions/{pid}.json")),
        serde_json::to_vec(&registry).unwrap(),
    )
    .unwrap();
    let capture = tmux.capture_when(|capture| capture.contains("❮ /bg"));
    assert!(!capture.contains("Running in a terminal"), "{capture}");
    let verdict = std::env::temp_dir().join("agents-foreground-prompt-verdict.json");
    std::fs::write(
        &verdict,
        "{\"verdict\":\"pass\",\"foreground_prompt\":\"/bg\"}\n",
    )
    .unwrap();
    println!("{}", verdict.display());
}

#[test]
fn terminal_no_task_prompt_shows_only_label_and_marker() {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-no-task-prompt-{}", std::process::id()));
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
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.wait_for(|capture| capture.lines().any(|line| line.trim() == "No task ❮"));
    let verdict = std::env::temp_dir().join("agents-no-task-prompt-verdict.json");
    std::fs::write(
        &verdict,
        "{\"verdict\":\"pass\",\"no_task_prompt\":\"pass\"}\n",
    )
    .unwrap();
    println!("{}", verdict.display());
}
