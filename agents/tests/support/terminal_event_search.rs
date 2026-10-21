use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_event_search_finds_trailing_tool_after_unicode() {
    let fixture = include_str!("../fixtures/codex-inner/command.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            if record["type"] == "event_msg" {
                record["payload"]["item"]["command"] =
                    serde_json::json!(["/bin/zsh", "-lc", "检查日志"]);
                record["payload"]["item"]["parsed_cmd"] =
                    serde_json::json!([{"type": "unknown", "cmd": "检查日志"}]);
            }
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (_directory, tmux) = terminal_codex_exit::start_codex_probe("unicode-search", &fixture);
    let capture = tmux.capture_when(|capture| capture.contains("检查日志 · 1"));
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains('│') && line.contains("检查日志 · 1"))
        .unwrap();
    let column = line[..line.find("检查日志 · 1").unwrap()].width();
    tmux.mouse(0, column, row, true);
    tmux.run(&["send-keys", "-t", "probe", "/"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "exec_command"]);
    let capture = tmux.capture_when(|capture| capture.contains("Search ❯ exec_command"));
    assert!(
        capture
            .lines()
            .any(|line| line.contains('│') && line.contains("检查日志 · 1")),
        "Unicode event disappeared from search results: {capture}"
    );
    assert!(capture.contains("Exec command"), "{capture}");
    let path = std::env::temp_dir().join("agents-unicode-search-tmux.txt");
    std::fs::write(&path, capture).unwrap();
    let verdict = serde_json::json!({"verdict": "pass", "capture": path});
    std::fs::write(
        std::env::temp_dir().join("agents-unicode-search-verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
