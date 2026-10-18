use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_log_scroll_window_and_boundaries() {
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
    let (directory, tmux, _) = start_probe_with_transcript("log-scroll", &contents);
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
    let advanced = tmux.capture_when(|text| {
        text != keyboard
            && text
                .lines()
                .skip(detail_row.saturating_sub(5))
                .any(|line| line.contains("window row"))
    });
    tmux.run(&["send-keys", "-t", "probe", "k"]);
    let reversed = tmux.capture_when(|text| text == keyboard);
    assert_ne!(advanced, reversed);
    std::fs::write(directory.path().join("capture.txt"), &keyboard).unwrap();
    let verdict = serde_json::json!({"verdict":"pass", "internal_scroll":true,"unfold":true,"bottom_handoff":true,"top_handoff":true,"short_handoff":true,"restore":true,"keyboard":true});
    let output = std::env::var_os("AGENTS_SCROLL_VERDICT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-log-scroll-verdict.json"));
    std::fs::write(output.with_extension("capture.txt"), keyboard).unwrap();
    std::fs::write(&output, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
}
