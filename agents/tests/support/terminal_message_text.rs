use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_message_text_shares_structured_layout() {
    let xml = include_str!("../fixtures/structured_text/real-theme.plist");
    let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .take(2)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    records[0]["message"]["content"] = xml.into();
    records[1]["message"]["content"][0]["text"] = xml.into();
    let mut queue: serde_json::Value = include_str!("../fixtures/claude-main-model-history.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["type"] == "queue-operation" && record["operation"] == "enqueue")
        .unwrap();
    queue["content"] = xml.into();
    records.push(queue);
    let transcript = records
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let (_directory, tmux, _transcript) = start_probe_with_transcript("message-text", &transcript);
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "120"]);
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .filter(|line| line.contains("<plist version=\"1.0\">"))
            .count()
            >= 4
            && capture.contains("Queue")
    });
    let rows: Vec<_> = capture.lines().collect();
    let mut structured = 0;
    for pair in rows.windows(2) {
        for (start, _) in pair[0].match_indices("<plist version=\"1.0\">") {
            if pair[1].match_indices("<dict>").any(|(child, _)| {
                pair[1][..child].width() == pair[0][..start].width() + 2
            }) {
                structured += 1;
            }
        }
    }
    let verdict = serde_json::json!({
        "verdict": if structured == 4 { "pass" } else { "fail" },
        "indented_bodies": structured,
        "surfaces": ["Update", "Prompt", "queued prompt", "log detail"],
    });
    std::fs::write(
        std::env::temp_dir().join("agents-message-text-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    std::fs::write(
        std::env::temp_dir().join("agents-message-text-probe.txt"),
        &capture,
    )
    .unwrap();
    println!("{verdict}");
    assert_eq!(verdict["verdict"], "pass", "{capture}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_background_text_expands_tabs_and_removes_ansi() {
    let (_directory, tmux, transcript) =
        background_text_probe("background-text", "\x1b[2mRunning\tchecks\x1b[0m\n");
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap()
        .write_all(include_bytes!("../fixtures/background/records.jsonl"))
        .unwrap();
    let capture = tmux.capture_when(|capture| capture.contains("Running    checks"));
    let verdict = serde_json::json!({"verdict": "pass", "expanded_tabs": true, "ansi_payload_visible": capture.contains("[2m")});
    assert!(!capture.contains("[2m"), "{capture}");
    std::fs::write(
        std::env::temp_dir().join("agents-background-text-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
