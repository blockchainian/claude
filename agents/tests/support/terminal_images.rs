use super::*;

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
    tmux.wait_for(|capture| capture.contains("New task ❯"));
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
