use super::*;

#[test]
fn terminal_background_await_shows_message_without_task_id() {
    use std::io::Write;
    let (_directory, tmux) = if let Some(path) = std::env::var_os("AGENTS_AWAIT_LIVE_TRANSCRIPT") {
        let contents = std::fs::read_to_string(path).unwrap();
        let mut records = Vec::new();
        let mut found = false;
        for line in contents.lines() {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            records.push(line);
            if record["type"] == "queue-operation"
                && record["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("<task-id>b4sczpw7k</task-id>"))
            {
                found = true;
                break;
            }
        }
        assert!(found, "recorded await task notification must exist");
        let (directory, tmux, _) =
            start_probe_with_transcript("background-await-live", &(records.join("\n") + "\n"));
        (directory, tmux)
    } else {
        let (directory, tmux, transcript) = start_probe("background-await");
        let output = directory.path().join("await.output");
        let fixture = include_str!("../fixtures/background/codex-await.output");
        std::fs::write(&output, fixture).unwrap();
        let record = include_str!("../fixtures/claude-task-notifications.jsonl")
            .lines()
            .next()
            .unwrap();
        let mut record: serde_json::Value = serde_json::from_str(record).unwrap();
        record["content"] = record["content"]
            .as_str()
            .unwrap()
            .replace(
                "/tmp/agents-notification/tasks/b9wi31ww0.output",
                output.to_str().unwrap(),
            )
            .into();
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(transcript)
                .unwrap(),
            "{record}"
        )
        .unwrap();
        (directory, tmux)
    };
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let capture =
        tmux.capture_when(|capture| capture.contains("d0b23338c838499217c04732ac1dd23ec36a8ddf"));
    let rows: Vec<_> = capture.lines().collect();
    let detail_rows = event_detail_rows(&capture, 200).unwrap();
    let detail = rows[detail_rows.clone()].join("\n");
    assert!(detail.contains("已提交并推送 example.com"), "{capture}");
    for raw in [
        "Task b9wi31ww0",
        "Task b4sczpw7k",
        "lastMessage",
        "turnId",
        "[exited with code",
        "`example.com`",
    ] {
        assert!(!detail.contains(raw), "{raw}: {capture}");
    }
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    let (row, column) = rows
        .iter()
        .enumerate()
        .skip(detail_rows.start)
        .take(detail_rows.len())
        .find_map(|(y, line)| line.find("example.com").map(|x| (y, line[..x].width())))
        .unwrap();
    let code = parser.screen().cell(row as u16, column as u16).unwrap();
    let prose = parser
        .screen()
        .cell(row as u16, (column - 2) as u16)
        .unwrap();
    assert_ne!(
        code.fgcolor(),
        prose.fgcolor(),
        "inline code uses Markdown color"
    );
    if let Some(path) = std::env::var_os("AGENTS_AWAIT_PROBE_RESULT") {
        std::fs::write(Path::new(&path).with_extension("txt"), &capture).unwrap();
        std::fs::write(
            path,
            "{\"status\":\"passed\",\"message\":true,\"markdown\":true,\"task_id_hidden\":true}\n",
        )
        .unwrap();
    }
    println!("{capture}\n{{\"background_await\":\"passed\"}}");
}
