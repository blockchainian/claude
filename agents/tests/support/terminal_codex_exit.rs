use super::*;

pub(super) fn start_codex_probe(name: &str, fixture: &str) -> (tempfile::TempDir, Tmux) {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("failed.jsonl");
    std::fs::write(&transcript, fixture).unwrap();
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
    let tmux = Tmux::new(format!("agents-{name}-{}", std::process::id()));
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
    tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|tabs| tabs.contains("Inactive 1"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    (directory, tmux)
}

#[test]
fn terminal_codex_shell_titles() {
    let mut captures = Vec::new();
    for (name, fixture, command, summary) in [
        (
            "shell-titles",
            include_str!("../fixtures/codex-inner/shell.jsonl"),
            "sleep 60",
            "sleep",
        ),
        (
            "chain-titles",
            include_str!("../fixtures/codex-inner/git-chain.jsonl"),
            "git status --short",
            "git {status, diff}, sed",
        ),
    ] {
        let (_directory, tmux) = start_codex_probe(name, fixture);
        let capture = tmux
            .capture_when(|capture| capture.contains("Exec command") && capture.contains(command));
        assert_eq!(capture.matches("Exec command").count(), 1, "{capture}");
        let title = capture
            .lines()
            .find(|line| line.trim_start().starts_with("Exec command"))
            .unwrap();
        assert_eq!(title.matches("Exec command").count(), 1, "{capture}");
        assert!(title.contains(summary), "{capture}");
        assert!(
            !title.contains(command),
            "arguments only belong in the body: {capture}"
        );
        assert!(!capture.contains("exec_command"), "{capture}");
        assert!(!capture.contains("CommandExecution"), "{capture}");
        captures.push(capture);
    }
    let path = std::env::temp_dir().join("agents-codex-shell-tmux.txt");
    std::fs::write(&path, captures.join("\n\n")).unwrap();
    let verdict = serde_json::json!({"verdict": "pass", "scenarios": 2, "capture": path});
    std::fs::write(
        std::env::temp_dir().join("agents-codex-shell.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
fn terminal_failed_plain_output_uses_default_foreground() {
    for (name, fixture, (title_marker, title), message) in [
        (
            "mcp-failed-output",
            include_str!("../fixtures/codex-mcp-errors.jsonl"),
            (
                "codebase-memory-mcp trace path",
                "codebase-memory-mcp trace path",
            ),
            "user rejected MCP tool call",
        ),
        (
            "command-failed-output",
            include_str!("../fixtures/codex-command-failed.jsonl"),
            ("Exec command · 1", "Exec command"),
            "[scrubbed]",
        ),
    ] {
        let (_directory, tmux) = start_codex_probe(name, fixture);
        tmux.wait_for(|capture| capture.contains(message));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let parser = captured_screen(&capture, 160, 45);
        let screen = parser.screen();
        let (title_row, title_text) = screen
            .rows(0, 160)
            .enumerate()
            .filter(|(_, row)| row.contains(title_marker))
            .last()
            .unwrap();
        let title_column = title_text[..title_text.find(title).unwrap()].width();
        assert!(screen.cell(title_row as u16, title_column as u16).is_some());
        assert_eq!(
            title_column, 1,
            "tool name stays in the left column: {capture}"
        );
        let row = screen
            .rows(0, 160)
            .position(|row| row.contains(message))
            .unwrap();
        let text = screen.rows(0, 160).nth(row).unwrap();
        let column = text.find(message).unwrap();
        for offset in 0..message.len() {
            let cell = screen.cell(row as u16, (column + offset) as u16).unwrap();
            assert_eq!(cell.fgcolor(), vt100::Color::Default, "{name}: {capture}");
            assert!(!cell.bold(), "error body keeps ordinary weight");
        }
        let path = std::env::temp_dir().join(format!("agents-{name}.ansi"));
        std::fs::write(&path, capture).unwrap();
        println!(
            "{{\"verdict\":\"pass\",\"capture\":\"{}\"}}",
            path.display()
        );
    }
}
