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
