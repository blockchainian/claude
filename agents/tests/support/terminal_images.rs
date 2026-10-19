use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_middle_paste_uses_clicked_input() {
    use base64::Engine;
    let (directory, tmux, _transcript) = start_probe("middle-paste");
    let clipboard = directory.path().join("clipboard/text");
    std::fs::write(&clipboard, "贴🦀").unwrap();
    begin_named_draft(&tmux, "Middle paste");
    let locate = |text: &str| {
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        capture
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                let byte = line.find(text)?;
                Some((line[..byte].width(), row))
            })
            .unwrap_or_else(|| panic!("missing {text}: {capture}"))
    };
    let caret = |x: usize, y: usize| {
        let actual = tmux.run(&[
            "display-message",
            "-t",
            "probe",
            "-p",
            "#{cursor_x},#{cursor_y}",
        ]);
        assert_eq!(actual.trim(), format!("{x},{y}"));
    };
    let (column, row) = locate("Middle paste ❯");
    let start = column + "Middle paste ❯ ".width();
    tmux.mouse(1, start + 30, row, true);
    tmux.wait_for(|capture| capture.contains("Middle paste ❯ 贴🦀"));
    caret(start + "贴🦀".width(), row);
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    tmux.mouse(1, start + 30, row, true);
    tmux.wait_for(|capture| capture.contains("Middle paste ❯ 贴🦀贴🦀"));
    caret(start + "贴🦀贴🦀".width(), row);
    tmux.run(&["send-keys", "-t", "probe", "C-u"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "剪贴板后"]);
    tmux.wait_for(|capture| capture.contains("Middle paste ❯ 剪贴板后"));
    // Focus task search, then middle-click the second display cell of 板.
    tmux.mouse(0, 2, 4, true);
    tmux.run(&["send-keys", "-t", "probe", "/"]);
    tmux.wait_for(|capture| capture.contains("Search ❯"));
    tmux.mouse(1, start + 5, row, true);
    tmux.wait_for(|capture| capture.contains("Middle paste ❯ 剪贴板贴🦀后"));
    caret(start + "剪贴板贴🦀".width(), row);
    tmux.mouse(1, 2, 4, true);
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(capture.contains("Middle paste ❯ 剪贴板贴🦀后"), "{capture}");
    assert!(!capture.contains("贴🦀贴🦀后"), "{capture}");
    tmux.mouse(0, start + 30, row, true);
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.run(&["send-keys", "-t", "probe", "C-u"]);
    tmux.wait_for(|capture| !capture.contains("剪贴板"));

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
    tmux.mouse(1, start + 30, row, true);
    tmux.wait_for(|capture| capture.contains("[Image #1]"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "tail"]);
    let capture = tmux.capture_when(|capture| capture.contains("[Image #1]tail"));
    std::fs::remove_file(&clipboard).unwrap();
    tmux.mouse(1, start + 30, row, true);
    let error = tmux.capture_when(|capture| capture.contains("No such file or directory"));
    assert!(error.contains("[Image #1]tail"), "{error}");
    let path = std::env::temp_dir().join("agents-middle-paste-tmux.txt");
    std::fs::write(&path, capture).unwrap();
    let verdict = serde_json::json!({
        "verdict": "pass",
        "clicked_input_and_cursor": "pass",
        "prompt_image_path": "pass",
        "missing_clipboard_notice": "pass",
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
