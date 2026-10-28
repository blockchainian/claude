use super::*;

#[test]
fn terminal_claude_chrome_tabs_close() {
    use std::io::Write;
    let (_directory, tmux, transcript) = start_probe("chrome-tabs-close");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap()
        .write_all(include_str!("../fixtures/claude-chrome-tabs-close.jsonl").as_bytes())
        .unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let plain = tmux.capture_when(|capture| {
        capture.contains("Closed tab 1002.")
            && capture.lines().any(|line| {
                line.trim_start()
                    .starts_with("claude-in-chrome tabs close mcp")
                    && line.split_whitespace().any(|word| word == "1002")
            })
    });
    assert!(
        !plain.contains("\"tabId\""),
        "unexpected input JSON: {plain}"
    );
    let capture = std::env::temp_dir().join("agents-chrome-tabs-close.ansi");
    std::fs::write(
        &capture,
        tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]),
    )
    .unwrap();
    let verdict = serde_json::json!({"verdict": "pass", "capture": capture});
    std::fs::write(
        std::env::temp_dir().join("agents-chrome-tabs-close.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
