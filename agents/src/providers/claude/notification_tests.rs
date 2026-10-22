// Excerpts from the supplied handback (57-58, 153-154), reply (41-44), and
// stopped (34, 80-83) transcripts; identifiers and local paths are redacted.
fn notification_output(contents: Option<&str>, summary: &str) -> Vec<Section> {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("agent.jsonl");
    if let Some(contents) = contents {
        std::fs::write(&file, contents).unwrap();
    }
    let output = directory.path().join("task.output");
    std::os::unix::fs::symlink(&file, &output).unwrap();
    let mut record: Value = serde_json::from_str(
        include_str!("../../../tests/fixtures/claude-task-notifications.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let text = record["content"]
        .as_str()
        .unwrap()
        .replace(
            "/tmp/agents-notification/tasks/b9wi31ww0.output",
            output.to_str().unwrap(),
        )
        .replace(
            "Background command \"等 tab 线程\" completed (exit code 0)",
            summary,
        );
    record["content"] = text.into();
    events_from(vec![record])[0].calls[0]
        .output
        .clone()
        .unwrap()
        .into_iter()
        .map(|section| match section {
            Section::OutputFile(path) => Section::read_output_file(&path),
            section => section,
        })
        .collect()
}

#[test]
fn notification_output_extracts_handback_and_final_reply_by_content() {
    for fixture in [
        include_str!("../../../tests/fixtures/claude-notification-handback.jsonl"),
        include_str!("../../../tests/fixtures/claude-notification-reply.jsonl"),
    ] {
        let rows: Vec<Value> = fixture
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let block = if rows.last().unwrap()["type"] == "assistant" {
            &rows.last().unwrap()["message"]["content"][0]
        } else {
            &rows[rows.len() - 2]["message"]["content"][0]
        };
        let text = block["input"]["message"]
            .as_str()
            .or_else(|| block["text"].as_str())
            .unwrap();
        for summary in ["Agent finished", "Background command completed"] {
            assert_eq!(
                notification_output(Some(fixture), summary),
                vec![Section::Markdown {
                    label: String::new(),
                    text: text.into()
                }]
            );
        }
    }
}

#[test]
fn notification_output_uses_last_handback_even_after_a_text_reply() {
    let fixture = include_str!("../../../tests/fixtures/claude-notification-handback.jsonl");
    let mut rows: Vec<Value> = fixture
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut handback = rows[2].clone();
    handback["message"]["content"][0]["input"]["message"] = "**Last report**".into();
    rows.push(handback);
    rows.push(
        serde_json::from_str::<Value>(
            include_str!("../../../tests/fixtures/claude-notification-reply.jsonl")
                .lines()
                .last()
                .unwrap(),
        )
        .unwrap(),
    );
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        notification_output(Some(&text), "Agent finished"),
        vec![Section::Markdown {
            label: String::new(),
            text: "**Last report**".into()
        }]
    );
}

#[test]
fn notification_output_does_not_use_intermediate_text_or_tool_calls() {
    let stopped = include_str!("../../../tests/fixtures/claude-notification-stopped.jsonl");
    assert_eq!(
        notification_output(Some(stopped), "Agent stopped"),
        vec![Section::Markdown {
            label: String::new(),
            text: String::new()
        }]
    );
    let mut rows: Vec<Value> = stopped
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let tool = rows[1]["message"]["content"][0].clone();
    rows.truncate(1);
    rows[0]["message"]["content"]
        .as_array_mut()
        .unwrap()
        .push(tool);
    assert_eq!(
        notification_output(Some(&rows[0].to_string()), "Agent stopped"),
        vec![Section::Markdown {
            label: String::new(),
            text: String::new()
        }]
    );
}

#[test]
fn notification_output_keeps_plain_text_and_unrelated_json() {
    for text in [
        "plain output\n  unchanged\n",
        "{\"type\":\"result\",\"message\":\"done\"}\n",
        "",
    ] {
        for summary in ["Agent finished", "Background command completed"] {
            assert_eq!(
                notification_output(Some(text), summary),
                vec![Section::Text {
                    label: String::new(),
                    text: text.into()
                }]
            );
        }
    }
}

#[test]
fn notification_output_marks_missing_files() {
    assert_eq!(
        notification_output(None, "Agent finished"),
        vec![Section::Placeholder("File no longer exists".into())]
    );
}

#[test]
fn notification_await_completion_is_markdown() {
    let fixture = include_str!("../../../tests/fixtures/codex-manager-await.jsonl");
    let event: Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(
        notification_output(Some(fixture), "Background command completed"),
        vec![Section::Markdown {
            label: String::new(),
            text: event["lastMessage"].as_str().unwrap().into()
        }]
    );
}

#[test]
fn notification_await_events_keep_order_and_errors() {
    let fixture = include_str!("../../../tests/fixtures/codex-manager-await.jsonl");
    let mut event: Value = serde_json::from_str(fixture).unwrap();
    event["error"] = "Turn failed".into();
    let output = format!("{fixture}{}\n[exited with code 1]\n", event);
    assert_eq!(
        notification_output(Some(&output), "Background command completed"),
        vec![Section::Markdown {
            label: String::new(),
            text: format!(
                "{}\n\n{}\n\nTurn failed",
                event["lastMessage"].as_str().unwrap(),
                event["lastMessage"].as_str().unwrap()
            )
        }]
    );
}

#[test]
fn background_notifications_shorten_only_the_display_and_keep_raw_records() {
    let fixture = include_str!("../../../tests/fixtures/claude-background-notifications.jsonl");
    let expected = [
        ("睡12秒再输出 ✓", Some(0)),
        ("给 a16z 视频改名 · 1", Some(1)),
        ("等右键改动 · -1", Some(-1)),
        ("三个配置依次快速启动 · 144", Some(144)),
        ("Sleep 600 seconds in background killed", None),
        ("mkdir, cd, npm {init, i}, tail ✓", Some(0)),
    ];
    for (line, (title, code)) in fixture.lines().zip(expected) {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{line}").unwrap();
        let event = tail(file.path(), 1).unwrap().remove(0);
        assert_eq!(event.body, title);
        let call = &event.calls[0];
        assert_eq!(call.title, title);
        assert_eq!(call.event_detail_title, title);
        assert_eq!(call.exit_code, code);
        assert_eq!(event.raw_text().unwrap(), line);
    }
    let chain = fixture.lines().nth(5).unwrap();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "{chain}").unwrap();
    let call = tail(file.path(), 1).unwrap().remove(0).calls.remove(0);
    assert_eq!(
        call.input,
        [Section::Shell {
            text: "mkdir -p /tmp/mermaid-debug/m && cd /tmp/mermaid-debug/m && npm init -y >/dev/null && npm i mermaid@11.12.2 jsdom 2>&1 | tail -3".into(),
            prefix: 0,
        }]
    );
    let row: Value = serde_json::from_str(fixture.lines().last().unwrap()).unwrap();
    let summary = row["content"]
        .as_str()
        .unwrap()
        .split_once("<summary>")
        .unwrap()
        .1
        .split_once("</summary>")
        .unwrap()
        .0;
    let description = summary
        .strip_prefix("Background command \"")
        .unwrap()
        .strip_suffix("\" completed (exit code 0)")
        .unwrap();
    let event = events_from(vec![row.clone()]).remove(0);
    assert_eq!(event.body, format!("{description} ✓"));
    assert_eq!(event.calls[0].exit_code, Some(0));
    for summary in ["Agent finished", "Monitor completed"] {
        let mut record = row.clone();
        let text = record["content"].as_str().unwrap();
        let start = text.find("<summary>").unwrap() + "<summary>".len();
        let end = text.find("</summary>").unwrap();
        record["content"] = format!("{}{summary}{}", &text[..start], &text[end..]).into();
        let event = events_from(vec![record]).remove(0);
        assert_eq!(event.body, summary);
        assert_eq!(event.calls[0].title, summary);
        assert_eq!(event.calls[0].exit_code, None);
    }
}
