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
fn terminal_send_preserves_reading_position() {
    use std::io::Write;
    let (directory, tmux, transcript) =
        start_probe_with_transcript("send-preserve", &scroll_transcript());
    let config = directory.path().join("claude");
    let mut registry: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/registry.json")).unwrap();
    let pid = std::process::id();
    let start = Command::new("ps")
        .env("TZ", "UTC")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .unwrap();
    assert!(start.status.success());
    registry["sessionId"] = "4165651d-b9e3-458d-8612-f8114e312363".into();
    registry["pid"] = pid.into();
    registry["procStart"] = String::from_utf8(start.stdout).unwrap().trim().into();
    registry["cwd"] = directory.path().to_str().unwrap().into();
    registry["kind"] = "bg".into();
    registry["jobId"] = "".into();
    registry["status"] = "idle".into();
    std::fs::create_dir_all(config.join("sessions")).unwrap();
    let registry_path = config.join(format!("sessions/{pid}.json"));
    std::fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
    tmux.wait_for(|text| text.lines().next().unwrap().contains("Active 1"));
    let capture = tmux.capture_when(|text| text.contains("scroll after"));
    let event_detail_row = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("scroll after"))
        .map(|(row, _)| row)
        .max()
        .unwrap();
    // Enter the earlier long event from the end, then scroll its full output up.
    tmux.mouse(64, 150, event_detail_row, false);
    tmux.wait_for(|text| {
        text.lines()
            .skip(32)
            .any(|line| line.contains("window row 60"))
    });
    for _ in 0..3 {
        tmux.mouse(64, 150, 40, false);
    }
    let before = tmux.capture_when(|text| {
        let output = text
            .lines()
            .skip(32)
            .take(16)
            .collect::<Vec<_>>()
            .join("\n");
        output.contains("window row")
            && !output.contains("window row 01")
            && !output.contains("window row 60")
    });
    let output = |text: &str| {
        text.lines()
            .skip(32)
            .take(16)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let prompt_row = before.lines().position(|line| line.contains('❯')).unwrap();
    tmux.mouse(0, 100, prompt_row, true);
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        "preserve reading position",
    ]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    // Missing job identity exercises the real send path without launching a model turn.
    let sent = tmux.capture_when(|text| text.contains("Claude background session has no job id"));
    assert_eq!(output(&sent), output(&before));
    registry["status"] = "busy".into();
    registry["updatedAt"] = chrono::Utc::now().timestamp_millis().into();
    std::fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
    let working = tmux.capture_when(|text| text.contains("Working"));
    assert_eq!(output(&working), output(&before));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    write!(file, "{}", await_records(20)).unwrap();
    let appended = tmux.capture_when(|text| text.contains("↓ 1 New"));
    assert_eq!(output(&appended), output(&before));
    let replacement = transcript.with_extension("replacement");
    std::fs::copy(&transcript, &replacement).unwrap();
    std::fs::rename(replacement, &transcript).unwrap();
    // A second append acknowledges that the replacement has been processed.
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    write!(file, "{}", await_records(21)).unwrap();
    let reset = tmux.capture_when(|text| text.contains("await item 21"));
    assert_eq!(output(&reset), output(&before));
    let path = std::env::temp_dir().join("agents-send-preserve-verdict.json");
    let verdict = serde_json::json!({"verdict":"pass", "send":true,"session_refresh":true,"append":true,"reset":true,"event_detail_unchanged":true});
    std::fs::write(&path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    std::fs::write(path.with_extension("before.txt"), &before).unwrap();
    std::fs::write(path.with_extension("sent.txt"), &sent).unwrap();
    std::fs::write(path.with_extension("working.txt"), &working).unwrap();
    std::fs::write(path.with_extension("appended.txt"), &appended).unwrap();
    std::fs::write(path.with_extension("capture.txt"), reset).unwrap();
    println!("verdict: {}", path.display());
    println!("{verdict}");
}

#[test]
fn terminal_event_scroll_window_and_boundaries() {
    let (directory, tmux, _) = start_probe_with_transcript("log-scroll", &scroll_transcript());
    let capture = tmux.capture_when(|text| text.contains("scroll after"));
    let event_detail_row = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("scroll after"))
        .map(|(row, _)| row)
        .max()
        .unwrap();
    let column = 90;
    tmux.mouse(64, column, event_detail_row, false);
    let folded = tmux.capture_when(|text| {
        text.lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains("window row 01"))
    });
    assert!(
        folded
            .lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains('…')),
        "{folded}"
    );
    tmux.mouse(65, column, event_detail_row, false);
    let scrolled = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert_ne!(folded, scrolled);
    assert!(scrolled.contains("window row 10"), "{scrolled}");
    // Each wheel report is allowed to hand off at most once to the list.
    let mut bottom = scrolled;
    for _ in 0..70 {
        if bottom
            .lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll after"))
        {
            break;
        }
        tmux.mouse(65, column, event_detail_row, false);
        bottom = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    }
    assert!(
        bottom
            .lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll after")),
        "{bottom}"
    );
    tmux.mouse(64, column, event_detail_row, false);
    let returned = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(
        returned
            .lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains("window row 60")),
        "{returned}"
    );
    let mut top = returned;
    for _ in 0..70 {
        if top
            .lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll before"))
        {
            break;
        }
        tmux.mouse(64, column, event_detail_row, false);
        top = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    }
    assert!(
        top.lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll before")),
        "{top}"
    );
    // Short content hands off immediately; keyboard uses the same window handler.
    tmux.mouse(0, column, event_detail_row, true);
    tmux.run(&["send-keys", "-t", "probe", "j"]);
    let keyboard = tmux.capture_when(|text| {
        text.lines()
            .skip(event_detail_row.saturating_sub(5))
            .any(|line| line.contains("window row"))
    });
    tmux.run(&["send-keys", "-t", "probe", "j"]);
    let event_detail_text = |text: &str| {
        text.lines()
            .skip(event_detail_row.saturating_sub(2))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let advanced = tmux.capture_when(|text| {
        event_detail_text(text) != event_detail_text(&keyboard)
            && text
                .lines()
                .skip(event_detail_row.saturating_sub(5))
                .any(|line| line.contains("window row"))
    });
    tmux.run(&["send-keys", "-t", "probe", "k"]);
    let reversed =
        tmux.capture_when(|text| event_detail_text(text) == event_detail_text(&keyboard));
    assert_ne!(event_detail_text(&advanced), event_detail_text(&reversed));
    std::fs::write(directory.path().join("capture.txt"), &keyboard).unwrap();
    let verdict = serde_json::json!({"verdict":"pass", "internal_scroll":true,"unfold":true,"bottom_handoff":true,"top_handoff":true,"short_handoff":true,"restore":true,"keyboard":true});
    let output = std::env::var_os("AGENTS_SCROLL_VERDICT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-log-scroll-verdict.json"));
    std::fs::write(output.with_extension("capture.txt"), keyboard).unwrap();
    std::fs::write(&output, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
}

fn await_records(index: usize) -> String {
    let mut records = include_str!("../fixtures/background/two-awaits.jsonl")
        .lines()
        .take(2)
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let id = format!("scroll-await-{index}");
    for (part, record) in records.iter_mut().enumerate() {
        record["uuid"] = format!("{id}-{part}").into();
        record["timestamp"] = (chrono::Utc::now() + chrono::Duration::milliseconds(index as i64))
            .to_rfc3339()
            .into();
    }
    records[0]["message"]["id"] = id.clone().into();
    records[0]["message"]["content"][0]["id"] = id.clone().into();
    records[0]["message"]["content"][0]["input"]["description"] =
        format!("await item {index:02}").into();
    records[1]["message"]["content"][0]["tool_use_id"] = id.into();
    records.iter().map(|record| format!("{record}\n")).collect()
}
