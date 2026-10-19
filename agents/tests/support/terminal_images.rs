use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_middle_paste_uses_focused_input() {
    use base64::Engine;
    let (directory, tmux, _transcript) = start_probe("middle-paste");
    let clipboard = directory.path().join("clipboard/text");
    std::fs::write(&clipboard, "middle🦀").unwrap();
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (row, line) = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("Stop hook"))
        .last()
        .unwrap();
    let column = line[..line.find("Stop hook").unwrap()].width();
    tmux.run(&["send-keys", "-t", "probe", "/"]);
    tmux.wait_for(|capture| capture.contains("Search ❯"));
    // A middle click on the log detail must keep the task search focused.
    tmux.mouse(1, column, row, true);
    tmux.wait_for(|capture| capture.contains("Search ❯ middle🦀"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "x"]);
    tmux.wait_for(|capture| capture.contains("Search ❯ middle🦀x"));
    std::fs::remove_file(&clipboard).unwrap();
    tmux.mouse(1, column, row, true);
    let error = tmux.capture_when(|capture| capture.contains("No such file or directory"));
    assert!(error.contains("Search ❯ middle🦀x"), "{error}");
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| capture.contains("Stop hook"));

    begin_named_draft(&tmux, "Middle paste");
    let event: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/claude-queued-image.jsonl").trim()).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(
            event["attachment"]["prompt"][1]["source"]["data"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let image = directory.path().join("middle picture.png");
    std::fs::write(&image, bytes).unwrap();
    std::fs::write(&clipboard, format!("'{}'", image.display())).unwrap();
    tmux.mouse(1, 1, 1, true);
    tmux.wait_for(|capture| capture.contains("[Image #1]"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "tail"]);
    let capture = tmux.capture_when(|capture| capture.contains("[Image #1]tail"));
    let path = std::env::temp_dir().join("agents-middle-paste-tmux.txt");
    std::fs::write(&path, capture).unwrap();
    let verdict = serde_json::json!({
        "verdict": "pass",
        "focused_search": "pass",
        "missing_clipboard_notice": "pass",
        "prompt_image_path": "pass",
        "capture": path,
    });
    std::fs::write(
        std::env::temp_dir().join("agents-middle-paste-verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_image_path_and_clipboard_are_atomic() {
    use base64::Engine;
    let (directory, tmux, _transcript) = start_probe("input-images");
    let event: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/claude-queued-image.jsonl").trim()).unwrap();
    let encoded = event["attachment"]["prompt"][1]["source"]["data"]
        .as_str()
        .unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let source = directory.path().join("a picture.png");
    std::fs::write(&source, &bytes).unwrap();
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    tmux.wait_for(|capture| capture.contains("New prompt ❯"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "probe"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| capture.contains("New prompt ❯"));
    let paste = format!("\x1b[200~'{}'\x1b[201~", source.display());
    tmux.run(&["send-keys", "-t", "probe", "-l", &paste]);
    tmux.wait_for(|capture| capture.contains("[Image #1]"));
    tmux.run(&["send-keys", "-t", "probe", "BSpace"]);
    tmux.wait_for(|capture| !capture.contains("[Image #1]"));
    std::fs::write(directory.path().join("clipboard/image.png"), &bytes).unwrap();
    tmux.run(&["send-keys", "-t", "probe", "C-v"]);
    tmux.wait_for(|capture| capture.contains("[Image #1]"));
    tmux.run(&["send-keys", "-t", "probe", "Left"]);
    tmux.run(&["send-keys", "-t", "probe", "Delete"]);
    tmux.wait_for(|capture| !capture.contains("[Image #1]"));
    println!("{{\"image_path\":\"pass\",\"clipboard\":\"pass\",\"atomic_delete\":\"pass\"}}");
}
