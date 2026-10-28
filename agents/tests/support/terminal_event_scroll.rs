use super::*;

fn scroll_transcript() -> String {
    let base = include_str!("../fixtures/transcript.jsonl");
    let records = base
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let user = records
        .iter()
        .find(|record| record["type"] == "user" && record["message"]["content"].is_string())
        .unwrap();
    let assistant = records
        .iter()
        .find(|record| {
            record["type"] == "assistant"
                && record["message"]["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|part| part["type"] == "text"))
        })
        .unwrap();
    let mut contents = base.to_owned();
    for (index, text, template) in [
        (0, "scroll before".to_owned(), user),
        (
            1,
            (1..=60)
                .map(|i| format!("window row {i:02}"))
                .collect::<Vec<_>>()
                .join("  \n"),
            assistant,
        ),
        (2, "scroll after".to_owned(), user),
    ] {
        let mut record = template.clone();
        record["uuid"] = format!("log-scroll-{index}").into();
        record["parentUuid"] = records.last().unwrap()["uuid"].clone();
        record["timestamp"] = format!("2026-10-18T01:00:0{index}Z").into();
        if index == 1 {
            record["message"]["content"] = serde_json::json!([{"type":"text", "text":text}]);
            record["message"]["id"] = "log-scroll-message".into();
        } else {
            record["message"]["content"] = text.into();
        }
        contents.push('\n');
        contents.push_str(&record.to_string());
    }
    contents.push('\n');
    contents
}

#[test]
fn terminal_event_scroll_reaches_top_before_handoff() {
    let contents = scroll_transcript()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            if record["uuid"] == "log-scroll-1" {
                record["message"]["content"][0]["text"] = format!(
                    "{}\n\n{}",
                    "请检查日志详情滚动和标题，确保完整显示。".repeat(30),
                    serde_json::json!({"status":"ok", "results": (0..30).collect::<Vec<_>>()})
                )
                .into();
            }
            format!("{record}\n")
        })
        .collect::<String>();
    let (_directory, tmux, _) = start_probe_with_transcript("scroll-top", &contents);
    tmux.run(&["resize-window", "-t", "probe", "-x", "120", "-y", "50"]);
    let capture = tmux.capture_when(|text| text.contains("scroll after"));
    let row = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("scroll after"))
        .map(|(row, _)| row)
        .max()
        .unwrap();
    tmux.mouse(64, 90, row, false);
    let initial = tmux.capture_when(|text| text.contains("请检查日志"));
    tmux.mouse(0, 90, row, true);
    tmux.run(&["send-keys", "-t", "probe", "Down", "Down"]);
    tmux.capture_when(|text| text != initial);
    tmux.mouse(64, 90, row, false);
    let top = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let detail = top.lines().skip(row.saturating_sub(2)).collect::<Vec<_>>();
    assert!(
        detail.iter().any(|line| line.contains("请检查日志")),
        "wheel must reach the title before leaving the entry: {top}"
    );
    assert!(
        detail
            .iter()
            .any(|line| line.trim_start().starts_with("Assistant")),
        "the restored title must be visible: {top}"
    );
    tmux.mouse(64, 90, row, false);
    tmux.capture_when(|text| {
        text.lines()
            .skip(row.saturating_sub(2))
            .any(|line| line.contains("scroll before"))
    });
    println!(
        "{}",
        serde_json::json!({"verdict":"pass", "narrow_chinese_json":true, "title_before_handoff":true})
    );
}
