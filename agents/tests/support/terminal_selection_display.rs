use super::*;

fn drag(tmux: &Tmux, from: (usize, usize), to: (usize, usize)) {
    tmux.mouse(0, from.0, from.1, false);
    tmux.mouse(32, to.0, to.1, false);
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        &format!("\x1b[<0;{};{}m", to.0 + 1, to.1 + 1),
    ]);
}

fn position(capture: &str, needle: &str, after: usize) -> (usize, usize) {
    capture
        .lines()
        .enumerate()
        .skip(after)
        .find_map(|(y, line)| line.find(needle).map(|byte| (line[..byte].width(), y)))
        .unwrap_or_else(|| panic!("missing {needle}: {capture}"))
}

#[test]
fn terminal_drag_copies_display_lines_and_joins_markdown_wraps() {
    use std::io::Write;
    let report = std::env::var_os("AGENTS_SELECTION_PROBE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-selection-display"));
    std::fs::create_dir_all(&report).unwrap();
    let (directory, tmux, transcript) = start_probe("selection-display");
    let clipboard = directory.path().join("clipboard/text");
    let mut record: serde_json::Value = serde_json::from_str(
        include_str!("../fixtures/claude-tool-calls.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    record["message"]["content"][0]["input"]["command"] =
        "cat /workspace/a && printf longer && echo done; pwd".into();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    writeln!(file, "{record}").unwrap();
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .skip(25)
            .any(|line| line.contains("printf longer"))
    });
    std::fs::write(report.join("shell-screen.txt"), &capture).unwrap();
    let first = position(&capture, "cat /workspace/a", 25);
    let last = position(&capture, "pwd", first.1 + 1);
    drag(&tmux, first, (last.0 + 3, last.1));
    let shell = "cat /workspace/a\nprintf longer\necho done\npwd";
    tmux.wait_for(|_| std::fs::read_to_string(&clipboard).is_ok_and(|text| text == shell));
    assert_eq!(std::fs::read_to_string(&clipboard).unwrap(), shell);

    // Reuse a recorded assistant envelope with its Markdown body.
    let mut record = include_str!("../fixtures/claude-notification-reply.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| {
            record["message"]["content"][0]["text"]
                .as_str()
                .is_some_and(|text| text.starts_with("**结论:**"))
        })
        .unwrap();
    let body = record["message"]["content"][0]["text"].as_str().unwrap();
    let body = body[..body.find("原").unwrap()].trim_end().to_owned();
    record["message"]["content"][0]["text"] = body.clone().into();
    record["isSidechain"] = false.into();
    record.as_object_mut().unwrap().remove("agentId");
    record["uuid"] = "selection-display-markdown".into();
    record["message"]["id"] = "selection-display-markdown".into();
    writeln!(file, "{record}").unwrap();
    tmux.run(&["resize-window", "-t", "probe", "-x", "50", "-y", "50"]);
    let capture =
        tmux.capture_when(|capture| capture.lines().skip(25).any(|line| line.contains("结论:")));
    std::fs::write(report.join("markdown-screen.txt"), &capture).unwrap();
    let first = position(&capture, "结论:", 25);
    let last = position(&capture, "Bash。", first.1 + 1);
    drag(&tmux, first, (last.0 + "Bash。".width(), last.1));
    let markdown = body.replace("**", "").replace('`', "");
    tmux.wait_for(|_| std::fs::read_to_string(&clipboard).is_ok_and(|text| text == markdown));
    assert_eq!(std::fs::read_to_string(&clipboard).unwrap(), markdown);
    let details =
        tmux.capture_when(|capture| capture.lines().take(18).any(|line| line.contains("结论:")));
    let first = position(&details, "结论:", 0);
    let last = position(&details, "Bash。", first.1 + 1);
    assert!(last.1 < 18, "task details Markdown must wrap: {details}");
    std::fs::remove_file(&clipboard).unwrap();
    drag(&tmux, first, (last.0 + "Bash。".width(), last.1));
    tmux.wait_for(|_| std::fs::read_to_string(&clipboard).is_ok_and(|text| text == markdown));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let task = position(&capture, "Codex manager", 0);
    drag(&tmux, task, (task.0 + 5, task.1));
    tmux.wait_for(|_| std::fs::read_to_string(&clipboard).is_ok_and(|text| text == "Codex"));
    let event = position(&capture, "结论:", 18);
    assert!(event.1 < 25, "event list Markdown: {capture}");
    drag(&tmux, event, (event.0 + "结论:".width(), event.1));
    tmux.wait_for(|_| std::fs::read_to_string(&clipboard).is_ok_and(|text| text == "结论:"));
    let verdict = serde_json::json!({"verdict":"pass", "shell":shell,"markdown":markdown,"panels":["event","task details","task list","event list"]});
    std::fs::write(
        report.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
