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
    let tmux = Tmux(format!("agents-{name}-{}", std::process::id()));
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
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_exit_sections() {
    let (_directory, tmux) = start_codex_probe(
        "exit-status",
        include_str!("../fixtures/codex-command-failed.jsonl"),
    );
    let capture = tmux.capture_when(|capture| {
        capture.contains("Exec command")
            && capture.contains("· 1")
            && capture.contains("[scrubbed]")
    });
    let path = std::env::temp_dir().join("agents-codex-exit-tmux.txt");
    std::fs::write(&path, &capture).unwrap();
    assert_eq!(capture.matches("Exec command").count(), 1, "{capture}");
    assert!(capture.contains("false (1)"), "{capture}");
    assert!(!capture.contains("exec_command"), "{capture}");
    let rows: Vec<_> = capture.lines().collect();
    let status = rows
        .iter()
        .position(|row| row.trim_start().starts_with("Exec command"))
        .expect(&capture);
    assert!(rows[status].contains("Exec command"), "{capture}");
    assert!(rows[status + 2].contains("false"), "{capture}");
    assert_eq!(rows[status + 3].trim(), "↓", "{capture}");
    assert!(rows[status + 4].contains("[scrubbed]"), "{capture}");
    assert!(rows[status].contains("Exec command · 1"), "{capture}");
    assert!(rows[status].contains("false"), "{capture}");
    let ansi = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&ansi, 160, 45);
    let screen = parser.screen();
    for (marker, count) in [("Exec command · 1", 1), ("false (1)", 2)] {
        let rows: Vec<_> = screen
            .rows(0, 160)
            .enumerate()
            .filter(|(_, row)| row.contains(marker))
            .collect();
        assert_eq!(rows.len(), count, "{capture}");
        for (row, text) in rows {
            let start = text[..text.find(marker).unwrap()].width();
            let digit = start + marker.trim_end_matches(')').width() - 1;
            assert_eq!(
                screen.cell(row as u16, digit as u16).unwrap().fgcolor(),
                vt100::Color::Idx(1)
            );
            assert_ne!(
                screen
                    .cell(row as u16, (digit - 2) as u16)
                    .unwrap()
                    .fgcolor(),
                vt100::Color::Idx(1)
            );
        }
    }
    assert!(!capture.contains("exit"), "{capture}");
    let verdict = serde_json::json!({"verdict": "pass", "capture": path});
    std::fs::write(
        std::env::temp_dir().join("agents-exit-status-verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
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
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_completed_subagent_calls() {
    let (_directory, tmux) = start_codex_probe(
        "subagent-completed",
        include_str!("../fixtures/codex-interaction/parent.jsonl"),
    );
    let capture = tmux.capture_when(|capture| capture.contains("Wait agent"));
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Wait agent"))
        .unwrap();
    let column = line[..line.find("Wait agent").unwrap()].width();
    tmux.mouse(0, column, row, true);
    let capture = tmux
        .capture_when(|capture| capture.contains("70000") && capture.contains("Wait completed."));
    let log_surface = capture
        .lines()
        .map(|line| line.chars().skip(80).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!log_surface.contains("SubAgentActivity"), "{capture}");
    assert!(!log_surface.contains("CollabAgentToolCall"), "{capture}");
    let path = std::env::temp_dir().join("agents-codex-subagent-completed-tmux.txt");
    std::fs::write(&path, capture).unwrap();
    println!("{}", serde_json::json!({"verdict":"pass","capture":path}));
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_failed_plain_output_uses_default_foreground() {
    for (name, fixture, (title_marker, title), message) in [
        (
            "mcp-failed-output",
            include_str!("../fixtures/codex-mcp-errors.jsonl"),
            (
                "Codebase-memory-mcp trace path",
                "Codebase-memory-mcp trace path",
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
