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
        let title = "\"等 tab 线程\" ✓";
        let capture = tmux.capture_when(|capture| {
            capture
                .rsplit_once(title)
                .is_some_and(|(_, detail)| detail.contains(expected))
        });
        let detail = capture.rsplit_once(title).unwrap().1;
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

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_background_notifications_shorten_titles_and_color_exit_numbers() {
    use std::io::Write;
    let (directory, tmux, transcript) = start_probe("background-notification-titles");
    let evidence =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/claude-notification-check/terminal");
    std::fs::create_dir_all(&evidence).unwrap();
    let expected = [
        ("\"睡12秒再输出\" ✓", None),
        ("\"给 a16z 视频改名\" · 1", Some("1")),
        ("\"等右键改动\" · -1", Some("-1")),
        ("\"三个配置依次快速启动\" · 144", Some("144")),
        ("\"Sleep 600 seconds in background\" killed", None),
    ];
    let fixture = include_str!("../fixtures/claude-background-notifications.jsonl");
    for (index, (record, (title, code))) in fixture.lines().zip(expected).enumerate() {
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .unwrap(),
            "{record}"
        )
        .unwrap();
        let capture = tmux.capture_when(|capture| capture.contains(title));
        assert!(!capture.contains("task-notification ·"), "{capture}");
        assert!(!capture.contains("Background command"), "{capture}");
        assert!(capture.contains("File no longer exists"), "{capture}");
        let rows: Vec<_> = capture.lines().collect();
        let rule = rows.iter().rposition(|line| line.contains(title)).unwrap() - 1;
        let header = capture.lines().nth(rule + 1).unwrap();
        if let Some(code) = code {
            assert_eq!(header.split_whitespace().next(), Some(code), "{capture}");
            let raw = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
            let parser = captured_screen(&raw, 200, 50);
            let column = header.find(code).unwrap();
            let cell = parser
                .screen()
                .cell((rule + 1) as u16, column as u16)
                .unwrap();
            assert!(!cell.dim(), "{raw}");
            // The detail number uses the same error color as the list's exit number.
            let (row, line) = capture
                .lines()
                .enumerate()
                .find(|(row, line)| *row < rule && line.contains(title))
                .unwrap();
            let column = line.find(title).unwrap();
            let column = line[..column].width() + title.width() - code.width();
            assert_eq!(
                cell.fgcolor(),
                parser
                    .screen()
                    .cell(row as u16, column as u16)
                    .unwrap()
                    .fgcolor(),
                "{raw}"
            );
            std::fs::write(evidence.join(format!("{index}.ansi")), raw).unwrap();
        }
        std::fs::write(evidence.join(format!("{index}.txt")), capture).unwrap();
    }
    drop(directory);
    let verdict = serde_json::json!({"background_notifications":"pass","forms":5,"numeric_exit_headers":true});
    std::fs::write(evidence.join("verdict.json"), verdict.to_string()).unwrap();
    println!("{verdict}");
}
