use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_notification_transcripts_show_only_the_report() {
    use std::io::Write;
    for (name, fixture, expected) in [
        (
            "handback",
            include_str!("../fixtures/claude-notification-handback.jsonl"),
            "报告已写好",
        ),
        (
            "reply",
            include_str!("../fixtures/claude-notification-reply.jsonl"),
            "结论:",
        ),
        (
            "stopped",
            include_str!("../fixtures/claude-notification-stopped.jsonl"),
            "",
        ),
    ] {
        let (directory, tmux, transcript) = start_probe(&format!("notification-{name}"));
        let file = directory.path().join("agent.jsonl");
        std::fs::write(&file, fixture).unwrap();
        let output = directory.path().join("task.output");
        std::os::unix::fs::symlink(file, &output).unwrap();
        let mut record: serde_json::Value = serde_json::from_str(
            include_str!("../fixtures/claude-task-notifications.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
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
        let capture = tmux.capture_when(|capture| {
            capture.contains("Task b9wi31ww0") && capture.contains(expected)
        });
        let detail = capture.split("Task b9wi31ww0").nth(1).unwrap();
        for hidden in [
            "Now ",
            "\"parentUuid\"",
            "\"tool_use\"",
            "**结论:**",
            "output-file:",
        ] {
            assert!(!detail.contains(hidden), "{name}: {capture}");
        }
        assert!(!detail.contains("(no output)"), "{name}: {capture}");
        if name == "reply" {
            let raw = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
            let parser = captured_screen(&raw, 200, 50);
            let (row, line) = capture
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("结论:"))
                .unwrap();
            let column = line[..line.find("结论:").unwrap()].width();
            assert!(
                parser
                    .screen()
                    .cell(row as u16, column as u16)
                    .unwrap()
                    .bold(),
                "{raw}"
            );
        }
        println!("{{\"notification_{name}\":\"pass\",\"raw_transcript\":false}}");
    }
}
