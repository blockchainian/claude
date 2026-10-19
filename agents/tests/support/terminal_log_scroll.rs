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
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_log_scroll_window_and_boundaries() {
    let (directory, tmux, _) = start_probe_with_transcript("log-scroll", &scroll_transcript());
    let capture = tmux.capture_when(|text| text.contains("scroll after"));
    let detail_row = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("scroll after"))
        .map(|(row, _)| row)
        .max()
        .unwrap();
    let column = 90;
    tmux.mouse(64, column, detail_row, false);
    let folded = tmux.capture_when(|text| {
        text.lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains("window row 01"))
    });
    assert!(
        folded
            .lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains('…')),
        "{folded}"
    );
    tmux.mouse(65, column, detail_row, false);
    let scrolled = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert_ne!(folded, scrolled);
    assert!(scrolled.contains("window row 10"), "{scrolled}");
    // Each wheel report is allowed to hand off at most once to the list.
    let mut bottom = scrolled;
    for _ in 0..70 {
        if bottom
            .lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll after"))
        {
            break;
        }
        tmux.mouse(65, column, detail_row, false);
        bottom = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    }
    assert!(
        bottom
            .lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll after")),
        "{bottom}"
    );
    tmux.mouse(64, column, detail_row, false);
    let returned = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(
        returned
            .lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains("window row 60")),
        "{returned}"
    );
    let mut top = returned;
    for _ in 0..70 {
        if top
            .lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll before"))
        {
            break;
        }
        tmux.mouse(64, column, detail_row, false);
        top = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    }
    assert!(
        top.lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains("scroll before")),
        "{top}"
    );
    // Short content hands off immediately; keyboard uses the same window handler.
    tmux.mouse(0, column, detail_row, true);
    tmux.run(&["send-keys", "-t", "probe", "j"]);
    let keyboard = tmux.capture_when(|text| {
        text.lines()
            .skip(detail_row.saturating_sub(5))
            .any(|line| line.contains("window row"))
    });
    tmux.run(&["send-keys", "-t", "probe", "j"]);
    let detail_text = |text: &str| {
        text.lines()
            .skip(detail_row.saturating_sub(2))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let advanced = tmux.capture_when(|text| {
        detail_text(text) != detail_text(&keyboard)
            && text
                .lines()
                .skip(detail_row.saturating_sub(5))
                .any(|line| line.contains("window row"))
    });
    tmux.run(&["send-keys", "-t", "probe", "k"]);
    let reversed = tmux.capture_when(|text| detail_text(text) == detail_text(&keyboard));
    assert_ne!(detail_text(&advanced), detail_text(&reversed));
    std::fs::write(directory.path().join("capture.txt"), &keyboard).unwrap();
    let verdict = serde_json::json!({"verdict":"pass", "internal_scroll":true,"unfold":true,"bottom_handoff":true,"top_handoff":true,"short_handoff":true,"restore":true,"keyboard":true});
    let output = std::env::var_os("AGENTS_SCROLL_VERDICT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-log-scroll-verdict.json"));
    std::fs::write(output.with_extension("capture.txt"), keyboard).unwrap();
    std::fs::write(&output, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_log_scroll_reaches_top_before_handoff() {
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

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_log_scroll_live_appends_keep_short_entries_moving() {
    use std::io::Write;
    for new in [2, 3] {
        let mut contents = include_str!("../fixtures/transcript.jsonl").to_owned();
        contents.push('\n');
        for index in 0..20 {
            contents.push_str(&await_records(index));
        }
        let (_directory, tmux, transcript) =
            start_probe_with_transcript(&format!("scroll-live-{new}"), &contents);
        let capture = tmux.capture_when(|text| text.contains("await item 19"));
        let row = capture
            .lines()
            .position(|line| line.contains("await item 12"))
            .expect("short entry must be in the middle of the visible list");
        tmux.mouse(0, 150, row, true);
        let detail = |text: &str, index| {
            text.lines()
                .skip(27)
                .any(|line| line.contains(&format!("await item {index:02}")))
        };
        tmux.capture_when(|text| detail(text, 12));
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        for index in 20..20 + new {
            write!(file, "{}", await_records(index)).unwrap();
        }
        tmux.capture_when(|text| text.contains(&format!("↓ {new} New")));
        for (step, forward) in [true, false, true, false].into_iter().enumerate() {
            // Deliver another real transcript update before every navigation input.
            write!(file, "{}", await_records(20 + new + step)).unwrap();
            tmux.capture_when(|text| text.contains(&format!("↓ {} New", new + step + 1)));
            tmux.mouse(if forward { 65 } else { 64 }, 90, 35, false);
            tmux.capture_when(|text| detail(text, if forward { 13 } else { 12 }));
        }
        tmux.mouse(65, 150, row, false);
        tmux.capture_when(|text| detail(text, 13));
        tmux.mouse(64, 150, row, false);
        let capture = tmux.capture_when(|text| detail(text, 12));
        let output = std::env::temp_dir().join(format!("agents-scroll-live-{new}.json"));
        let verdict = serde_json::json!({"verdict":"pass", "new":new,"list_middle":true,"one_wheel_per_short_entry":true,"append_between_each_input":true});
        std::fs::write(output, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
        std::fs::write(
            std::env::temp_dir().join(format!("agents-scroll-live-{new}.txt")),
            capture,
        )
        .unwrap();
        println!("{verdict}");
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_scrollbar_uses_right_edge_blocks_below_rule() {
    let (_directory, tmux, _) = start_probe_with_transcript("scrollbar", &scroll_transcript());
    let capture = tmux.capture_when(|text| text.contains("scroll after"));
    let detail_row = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("scroll after"))
        .map(|(row, _)| row)
        .last()
        .unwrap();
    tmux.mouse(64, 90, detail_row, false);
    let capture = tmux.capture_when(|text| text.contains('▕'));
    let rows = capture.lines().collect::<Vec<_>>();
    let top = rows.iter().rposition(|line| line.starts_with('─')).unwrap();
    assert_eq!(rows[top].chars().last(), Some('─'), "{capture}");
    assert_eq!(rows[top + 1].chars().last(), Some('▕'), "{capture}");
    for row in &rows[top + 1..rows.len() - 1] {
        assert!(
            matches!(row.chars().nth(199), Some('▕' | ' ') | None),
            "{capture}"
        );
    }
    let verdict =
        serde_json::json!({"verdict":"pass", "thumb":"▕", "top_rule":"─", "track":"blank"});
    if let Some(output) = std::env::var_os("AGENTS_SCROLLBAR_VERDICT") {
        let output = std::path::PathBuf::from(output);
        std::fs::write(output.with_extension("capture.txt"), &capture).unwrap();
        std::fs::write(output, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    }
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_log_scroll_footer_shows_scrolled_off_title() {
    let (_directory, tmux, _) = start_probe_with_transcript("footer-title", &scroll_transcript());
    let capture = tmux.capture_when(|text| text.contains("scroll after"));
    let row = capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("scroll after"))
        .map(|(row, _)| row)
        .max()
        .unwrap();
    tmux.mouse(64, 90, row, false);
    let entered = tmux.capture_when(|text| text.contains("window row 60"));
    let footer = entered.lines().last().unwrap();
    assert!(footer.contains("Assistant"), "{footer}");
}
