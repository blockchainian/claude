use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_question_footer_styles_and_clicks() {
    let mut cases = Vec::new();
    for action in ["Enter", "Esc"] {
        let (directory, tmux) = terminal_codex_exit::start_codex_probe(
            &format!("question-footer-{}", action.to_lowercase()),
            include_str!("../fixtures/codex-command-failed.jsonl"),
        );
        std::fs::write(
            directory.path().join("codex/failed.jsonl"),
            include_str!("../fixtures/codex-input/daemon-pending.jsonl"),
        )
        .unwrap();
        tmux.wait_for(|capture| capture.lines().next().unwrap().contains("Active 1"));
        tmux.run(&["send-keys", "-t", "probe", "Tab"]);
        let capture = tmux.capture_when(|capture| capture.contains("Enter to confirm"));
        let (row, line) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Enter to confirm"))
            .unwrap();
        let ansi = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let parser = captured_screen(&ansi, 160, 45);
        for key in ["↑/↓", "Enter", "Esc"] {
            let start = line[..line.find(key).unwrap()].width();
            for column in start..start + key.width() {
                let cell = parser.screen().cell(row as u16, column as u16).unwrap();
                assert!(cell.bold(), "{key}: {ansi}");
                assert!(!cell.dim(), "{key}: {ansi}");
            }
            let label = parser
                .screen()
                .cell(row as u16, (start + key.width() + 1) as u16)
                .unwrap();
            assert!(label.dim());
            assert!(!label.bold());
        }
        // Select the free-text row and keep it editing when clicking either action.
        tmux.run(&["send-keys", "-t", "probe", "Down"]);
        tmux.run(&["send-keys", "-t", "probe", "Down"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "typed answer"]);
        tmux.wait_for(|capture| capture.contains("typed answer"));
        let column = line[..line.find(action).unwrap()].width() + 1;
        let click = format!("\x1b[<0;{column};{}M\x1b[<0;{column};{}m", row + 1, row + 1);
        tmux.run(&["send-keys", "-t", "probe", "-l", &click]);
        // Both actions reach the real transport, isolated from the user's daemon.
        let result =
            tmux.capture_when(|capture| capture.contains("app-server daemon isn't running"));
        assert!(result.contains("typed answer"), "{result}");
        assert!(
            !result.contains(&format!("typed answer{action}")),
            "{result}"
        );
        std::fs::write(
            std::env::temp_dir().join(format!("agents-question-footer-{action}.txt")),
            result,
        )
        .unwrap();
        cases.push(action);
    }
    let verdict = serde_json::json!({
        "verdict": "pass", "bold_keys": true, "dim_labels": true,
        "clicks": cases,
        "transport": "isolated daemon unavailable; dispatch verified"
    });
    std::fs::write(
        std::env::temp_dir().join("agents-question-footer-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

fn click_hint(tmux: &Tmux, capture: &str, key: &str) {
    let (row, line) = capture.lines().enumerate().last().unwrap();
    let column = line[..line.find(key).expect(capture)].width() + 1;
    let click = format!("\x1b[<0;{column};{}M\x1b[<0;{column};{}m", row + 1, row + 1);
    tmux.run(&["send-keys", "-t", "probe", "-l", &click]);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_shared_footer_clicks_and_help_keys() {
    let (_directory, tmux, _) = start_probe("shared-footer");
    let capture = tmux.capture_when(|capture| capture.lines().last().unwrap().trim() == "? help");
    click_hint(&tmux, &capture, "?");
    let help = tmux.capture_when(|capture| capture.contains("Task shortcuts"));
    assert!(help.contains("PgUp"));
    assert!(help.contains("Backspace"));
    assert!(help.contains("Tab/⇧Tab"));
    let (row, line) = help
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Enter") && line.contains("Open"))
        .unwrap();
    let column = line[..line.find("Enter").unwrap()].width() + 1;
    let click = format!("\x1b[<0;{column};{}M\x1b[<0;{column};{}m", row + 1, row + 1);
    tmux.run(&["send-keys", "-t", "probe", "-l", &click]);
    // A queued capture is processed after the click; the help page remains unchanged.
    assert_eq!(tmux.run(&["capture-pane", "-t", "probe", "-p"]), help);
    assert_eq!(help.lines().last().unwrap().trim(), "Esc back");
    click_hint(&tmux, &help, "Esc");
    tmux.wait_for(|capture| !capture.contains("Task shortcuts"));
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    assert_title_caret(&tmux, "New task", 0);
    tmux.run(&["send-keys", "-t", "probe", "-l", "Footer probe"]);
    let capture = tmux.capture_when(|capture| capture.contains("Footer probe"));
    click_hint(&tmux, &capture, "?");
    let help = tmux.capture_when(|capture| capture.contains("Task shortcuts"));
    assert_eq!(help.lines().last().unwrap().trim(), "Esc back");
    click_hint(&tmux, &help, "Esc");
    assert_title_caret(&tmux, "Footer probe", "Footer probe".width());
    // Continue to the repository browser and focus the prompt editor.
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.split('│').any(|part| part.trim() == ".."))
    });
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.run(&["send-keys", "-t", "probe", "C-w"]);
    tmux.run(&["send-keys", "-t", "probe", "j"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "prompt text"]);
    let capture = tmux.capture_when(|capture| capture.contains("prompt text"));
    click_hint(&tmux, &capture, "?");
    let help = tmux.capture_when(|capture| capture.contains("Task shortcuts"));
    assert_eq!(help.lines().last().unwrap().trim(), "Esc back");
    click_hint(&tmux, &help, "Esc");
    let restored = tmux.capture_when(|capture| capture.contains("prompt text"));
    assert!(!restored.contains("prompt text?"), "{restored}");
    let returned = tmux.capture_when(|capture| capture.lines().last().unwrap().trim() == "? help");
    tmux.run(&["send-keys", "-t", "probe", "-l", " retained"]);
    tmux.wait_for(|capture| capture.contains("prompt text retained"));
    let evidence = serde_json::json!({"help":help, "returned":returned});
    std::fs::write(
        std::env::temp_dir().join("agents-help-footer-captures.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    let verdict = serde_json::json!({"verdict":"pass", "clicks":["?","Esc"], "focused_inputs":["title","prompt"], "help_keys":"display only"});
    std::fs::write(
        std::env::temp_dir().join("agents-shared-footer-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_pending_questions_wrap_in_full() {
    let (directory, tmux, transcript) = start_probe("pending-question-wrap");
    let parent = transcript.file_stem().unwrap().to_str().unwrap();
    let rollout = directory.path().join("codex/pending.jsonl");
    std::fs::write(
        &rollout,
        include_str!("../fixtures/codex-command-failed.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(directory.path().join("codex/state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute("UPDATE threads SET rollout_path=?1, name='Pending question probe', cwd=?2, archived=0 WHERE id='thread-0'",
        rusqlite::params![rollout.to_str().unwrap(), directory.path().to_str().unwrap()]).unwrap();
    let manager = directory.path().join(parent);
    std::fs::create_dir(&manager).unwrap();
    std::fs::create_dir(directory.path().join("threads")).unwrap();
    let mut state: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/codex-manager/attached-state.json"
    ))
    .unwrap();
    state["threads"][0]["id"] = "thread-0".into();
    std::fs::write(
        manager.join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let mut supervisor: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/codex-manager/attached-supervisor.json"
    ))
    .unwrap();
    supervisor["sessionId"] = parent.into();
    std::fs::write(
        directory.path().join("threads/thread-0.json"),
        serde_json::to_vec(&supervisor).unwrap(),
    )
    .unwrap();
    let ask_path = manager.join("thread-0.ask.pending.json");
    std::fs::write(&ask_path, include_str!("../fixtures/supervision-ask.json")).unwrap();
    tmux.run(&["resize-window", "-t", "probe", "-x", "120", "-y", "80"]);
    tmux.capture_when(|capture| capture.lines().next().unwrap().contains("Active 2"));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let selected = tmux.capture_when(|capture| capture.contains("Pending question probe"));
    let (row, line) = selected
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Pending question probe"))
        .unwrap();
    let column = line[..line.find("Pending question probe").unwrap()].width();
    tmux.mouse(0, column, row, true);
    let ask_capture = tmux.capture_when(|capture| capture.contains("Needs input"));
    let ask: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/supervision-ask.json")).unwrap();
    let full_text = |capture: &str| {
        let panel = capture
            .lines()
            .filter_map(|line| line.split('│').nth(1))
            .collect::<Vec<_>>()
            .join("\n");
        panel
            .split_once("Needs input")
            .unwrap()
            .1
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '`')
            .collect::<String>()
    };
    let expected: String = ask["text"]
        .as_str()
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(full_text(&ask_capture).contains(&expected), "{ask_capture}");
    assert!(
        !ask_capture.contains("Open task to review."),
        "{ask_capture}"
    );
    let record: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/claude-questions-pending.jsonl")).unwrap();
    let mut long_ask = ask;
    long_ask["text"] = record["message"]["content"][0]["input"]["questions"][0]["question"].clone();
    std::fs::write(&ask_path, serde_json::to_vec(&long_ask).unwrap()).unwrap();
    let long_capture = tmux.capture_when(|capture| capture.contains("怎么判断"));
    let expected: String = long_ask["text"]
        .as_str()
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '`')
        .collect();
    assert!(
        full_text(&long_capture).contains(&expected),
        "{long_capture}"
    );
    let evidence = serde_json::json!({"verdict": "pass", "ask_capture": ask_capture, "long_capture": long_capture, "width": 120});
    std::fs::write(
        std::env::temp_dir().join("agents-pending-question-wrap.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    println!("{evidence}");
}
