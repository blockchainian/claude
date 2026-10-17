use super::*;

#[test]
#[ignore = "requires tmux and a desktop clipboard; run with --ignored --nocapture"]
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
    let mut clipboard = arboard::Clipboard::new().unwrap();
    let saved_text = clipboard.get_text().ok();
    let saved_image = clipboard.get_image().ok();
    let saved_files = clipboard
        .get()
        .file_list()
        .ok()
        .filter(|files| !files.is_empty());
    let rgba = image::load_from_memory(&bytes).unwrap().into_rgba8();
    clipboard
        .set_image(arboard::ImageData {
            width: rgba.width() as usize,
            height: rgba.height() as usize,
            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
        })
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        tmux.run(&["send-keys", "-t", "probe", "C-v"]);
        tmux.wait_for(|capture| capture.contains("[Image #1]"));
        tmux.run(&["send-keys", "-t", "probe", "Left"]);
        tmux.run(&["send-keys", "-t", "probe", "Delete"]);
        tmux.wait_for(|capture| !capture.contains("[Image #1]"));
    });
    if let Some(files) = saved_files {
        clipboard.set().file_list(&files).unwrap();
    } else if let Some(image) = saved_image {
        clipboard.set_image(image).unwrap();
    } else if let Some(text) = saved_text {
        clipboard.set_text(text).unwrap();
    } else {
        clipboard.clear().unwrap();
    }
    result.unwrap();
    println!("{{\"image_path\":\"pass\",\"clipboard\":\"pass\",\"atomic_delete\":\"pass\"}}");
}
