use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_disabled_prompt_keeps_mirrored_marker_and_dim_reason() {
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
    let tmux = Tmux(format!("agents-disabled-prompt-{}", std::process::id()));
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
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Inactive 2"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    tmux.wait_for(|capture| capture.contains("Example thread 1 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "Down"]);
    let expected = "升级命令中心 ❮ Not supported";
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
        assert!(cell.dim(), "cell ({x}, {row}): {capture}");
        x += grapheme.width();
    }
    let path = std::env::temp_dir().join("agents-disabled-prompt-tmux.txt");
    std::fs::write(&path, &capture).unwrap();
    println!(
        "{{\"verdict\":\"pass\",\"disabled_prompt\":\"pass\",\"dim\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}
