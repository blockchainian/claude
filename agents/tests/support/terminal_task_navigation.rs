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
    // Both keyboard and wheel stay on the last and first task.
    tmux.run(&["send-keys", "-t", "probe", "Down", "/"]);
    tmux.wait_for(|s| s.contains("Search ❯") && s.contains("Example task 13 ❮"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|s| !s.contains("Search ❯"));
    tmux.mouse(65, 5, row, false);
    tmux.wait_for(|s| s.contains("Example task 13 ❮"));
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    tmux.wait_for(|s| s.contains("Example task 12 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "Up", "/"]);
    tmux.wait_for(|s| s.contains("Search ❯") && s.contains("Example task 12 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|s| !s.contains("Search ❯"));
    tmux.mouse(64, 5, row, false);
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
        serde_json::json!({"task_list_boundaries": "passed", "task_history": "passed", "opening_keys": "passed", "search_escape": "passed", "help": "passed"})
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_border_double_clicks() {
    let (directory, tmux, transcript) = start_probe("task-borders");
    let project = transcript.parent().unwrap();
    for index in 0..9 {
        let id = if index == 0 {
            "4165651d-b9e3-458d-8612-f8114e312363".to_owned()
        } else {
            format!("00000000-0000-0000-0000-000000000{index:03}")
        };
        let records = include_str!("../fixtures/transcript.jsonl")
            .lines()
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                record["sessionId"] = id.clone().into();
                if record["type"] == "ai-title" {
                    record["aiTitle"] = format!("Border task {index:02}").into();
                }
                record.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(project.join(format!("{id}.jsonl")), records + "\n").unwrap();
    }
    tmux.wait_for(|capture| capture.contains("Inactive 9"));
    let selected = |capture: &str| {
        (0..9)
            .map(|index| format!("Border task {index:02}"))
            .find(|name| {
                ['❯', '❮']
                    .iter()
                    .any(|marker| capture.contains(&format!("{name} {marker}")))
            })
            .unwrap_or_else(|| panic!("selected task missing: {capture}"))
    };
    for width in [100, 149, 150, 200] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "20",
        ]);
        tmux.wait_for(|capture| {
            capture.lines().count() == 20 && capture.lines().nth(1).unwrap().width() == width
        });
        // Establish both ends through the existing keyboard navigation.
        tmux.mouse(0, 4, 3, true);
        tmux.run(&["send-keys", "-t", "probe", "Home"]);
        let top = tmux.capture_when(|capture| capture.lines().nth(2).unwrap().contains("/claude"));
        let first = selected(&top);
        tmux.run(&["send-keys", "-t", "probe", "End"]);
        let bottom =
            tmux.capture_when(|capture| !capture.lines().nth(2).unwrap().contains("/claude"));
        let last = selected(&bottom);
        assert_ne!(first, last);
        let border = bottom
            .lines()
            .enumerate()
            .skip(2)
            .find(|(_, line)| line.chars().filter(|c| *c == '─').count() > width / 2)
            .unwrap()
            .0;
        let double_click = |row: usize| {
            let sequence = format!("\x1b[<0;5;{}M\x1b[<0;5;{}m", row + 1, row + 1).repeat(2);
            tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
        };
        double_click(1);
        let top = tmux.capture_when(|capture| {
            selected(capture) == first && capture.lines().nth(2).unwrap().contains("/claude")
        });
        std::fs::write(directory.path().join(format!("top-{width}.txt")), top).unwrap();
        double_click(border);
        let bottom = tmux.capture_when(|capture| {
            selected(capture) == last && !capture.lines().nth(2).unwrap().contains("/claude")
        });
        std::fs::write(directory.path().join(format!("bottom-{width}.txt")), bottom).unwrap();
    }
    let verdict = serde_json::json!({"task_border_double_clicks":"pass", "widths":[100,149,150,200], "ends":["top","bottom"]});
    if let Some(path) = std::env::var_os("AGENTS_PROBE_VERDICT") {
        std::fs::write(path, verdict.to_string()).unwrap();
    }
    println!("{verdict}");
}
