use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_ask_questions_render_real_rejection_and_answer() {
    let (_directory, tmux, transcript) = start_probe("ask-questions");
    tmux.run(&["resize-window", "-t", "probe", "-x", "220", "-y", "140"]);
    for (name, fixture, header, output) in [
        (
            "rejected",
            include_str!("../fixtures/claude-questions-rejected.jsonl"),
            "AskUserQuestion",
            "Questions asked:",
        ),
        (
            "answered",
            include_str!("../fixtures/claude-questions-answered.jsonl"),
            "AskUserQuestion",
            "Your questions have been answered:",
        ),
    ] {
        std::fs::write(&transcript, fixture).unwrap();
        tmux.run(&["send-keys", "-t", "probe", "End"]);
        std::thread::sleep(Duration::from_secs(2));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-ask-question-{name}.txt"));
        std::fs::write(&path, &capture).unwrap();
        assert!(capture.contains(header), "{capture}");
        assert!(capture.contains("• "), "{capture}");
        assert!(!capture.contains("\"questions\": ["), "{capture}");
        assert!(capture.contains("(Recommended)"), "{capture}");
        assert!(!capture.contains("questions: [{"), "{capture}");
        let output_at = capture.find(output).expect(&capture);
        if name == "rejected" {
            assert!(
                output_at
                    < capture
                        .find("The user doesn't want to proceed")
                        .expect(&capture)
            );
            assert!(capture.contains("(No answer provided)"), "{capture}");
        }
        println!(
            "{{\"case\":\"{name}\",\"verdict\":\"pass\",\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

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
        tmux.run(&["send-keys", "-t", "probe", "Down"]);
        tmux.wait_for(|capture| capture.contains("› 2. Blue"));
        let column = line[..line.find(action).unwrap()].width() + 1;
        let click = format!("\x1b[<0;{column};{}M\x1b[<0;{column};{}m", row + 1, row + 1);
        tmux.run(&["send-keys", "-t", "probe", "-l", &click]);
        // Both actions reach the real transport, isolated from the user's daemon.
        let result =
            tmux.capture_when(|capture| capture.contains("app-server daemon isn't running"));
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
    let capture = tmux.capture_when(|capture| capture.contains("Enter open"));
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
    click_hint(&tmux, &help, "Esc");
    let capture = tmux.capture_when(|capture| !capture.contains("Task shortcuts"));
    click_hint(&tmux, &capture, "n new");
    assert_title_caret(&tmux, "New task", 0);
    tmux.run(&["send-keys", "-t", "probe", "-l", "Footer probe"]);
    let capture = tmux.capture_when(|capture| capture.contains("Enter next"));
    click_hint(&tmux, &capture, "Enter");
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.split('│').any(|part| part.trim() == ".."))
    });
    let verdict = serde_json::json!({"verdict":"pass", "clicks":["?","Esc","n","Enter"], "help_keys":"display only"});
    std::fs::write(
        std::env::temp_dir().join("agents-shared-footer-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
