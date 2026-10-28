use super::*;

fn detail_top(capture: &str) -> usize {
    capture
        .lines()
        .enumerate()
        .filter(|(_, line)| line.chars().filter(|&ch| ch == '─').count() > 100)
        .map(|(row, _)| row)
        .filter(|&row| row < 48)
        .last()
        .expect("full-width detail rule")
}

fn detail_body(capture: &str, top: usize) -> String {
    capture
        .lines()
        .skip(top + 1)
        .take(47 - top)
        .collect::<Vec<_>>()
        .join("\n")
}

fn claude_records() -> String {
    let base = include_str!("../fixtures/transcript.jsonl");
    let mut record = base
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["type"] == "assistant")
        .unwrap();
    record["uuid"] = "transcript-mode-event".into();
    record["message"]["id"] = "transcript-mode-message".into();
    record["message"]["content"] = serde_json::json!([{
        "type":"text", "text":"Transcript mode probe",
    }]);
    record["timestamp"] = "2026-10-21T01:02:03Z".into();
    record["000_probe_multiline"] = "probe first line\nprobe second line".into();
    let long: serde_json::Value = serde_json::from_str(
        include_str!("../fixtures/codex-raw-detail.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    record["001_probe_long"] = long["payload"]["content"][0]["text"].clone();
    record["zzz_probe_tail"] = "transcript raw end".into();
    record["aaa_probe_rows"] = (1..=60)
        .map(|index| format!("raw_row_{index:02}"))
        .collect::<Vec<_>>()
        .into();
    format!("{base}\n{record}\n")
}

#[test]
fn terminal_transcript_modes() {
    let report = std::env::var_os("AGENTS_TRANSCRIPT_PROBE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-transcript-probe"));
    std::fs::create_dir_all(&report).unwrap();
    let (directory, tmux, _) = start_probe_with_transcript("transcript-claude", &claude_records());
    let event = tmux.capture_when(|capture| capture.contains("Transcript mode probe"));
    let top = detail_top(&event);
    tmux.mouse(0, 90, top + 2, true);
    tmux.run(&["send-keys", "-t", "probe", "Space"]);
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    let start = tmux.capture_when(|capture| {
        detail_body(capture, top).contains("\"aaa_probe_rows\": [")
            && detail_body(capture, top).contains("\"raw_row_01\",")
    });
    assert!(
        start
            .lines()
            .skip(top + 1)
            .take(2)
            .any(|line| line.contains("assistant")),
        "{start}"
    );
    std::fs::write(report.join("claude-start.txt"), &start).unwrap();
    let body = detail_body(&start, top);
    let first = body
        .lines()
        .position(|line| line.contains("probe first line"))
        .unwrap();
    let second = body
        .lines()
        .position(|line| line.contains("probe second line"))
        .unwrap();
    assert_eq!(second, first + 1, "{start}");
    let long_start = body
        .lines()
        .position(|line| line.contains("return empty bubblemap data"))
        .unwrap();
    let long_end = body
        .lines()
        .position(|line| line.contains("mobile app"))
        .unwrap();
    assert!(long_end > long_start, "long JSON value must wrap: {start}");
    assert!(!body.contains('…'), "{start}");
    assert!(
        !body.contains(r"probe first line\nprobe second line"),
        "{start}"
    );

    assert!(!body.contains("raw_row_60"), "{start}");
    let row = start
        .lines()
        .position(|line| line.contains("\"raw_row_01\","))
        .unwrap();
    let line = start.lines().nth(row).unwrap();
    let column = line[..line.find("raw_row_01").unwrap()].width();
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    assert_ne!(
        parser
            .screen()
            .cell(row as u16, column as u16)
            .unwrap()
            .fgcolor(),
        vt100::Color::Default,
        "JSON string must have syntax color: {colored}"
    );
    std::fs::write(report.join("claude-start.ansi"), colored).unwrap();

    let clipboard = directory.path().join("clipboard/text");
    tmux.mouse(0, column, row, false);
    tmux.mouse(32, column + "raw_row_01".width(), row, false);
    let release = format!("\x1b[<0;{};{}m", column + "raw_row_01".width() + 1, row + 1);
    tmux.run(&["send-keys", "-t", "probe", "-l", &release]);
    tmux.wait_for(|_| std::fs::read_to_string(&clipboard).is_ok_and(|text| text == "raw_row_01"));
    assert_eq!(std::fs::read_to_string(clipboard).unwrap(), "raw_row_01");

    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let end = tmux.capture_when(|capture| {
        detail_body(capture, top).contains("transcript raw end")
            && !detail_body(capture, top).contains("raw_row_01")
    });
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    tmux.wait_for(|capture| detail_body(capture, top).contains("raw_row_01"));
    tmux.run(&["send-keys", "-t", "probe", "Down"]);
    tmux.wait_for(|capture| detail_body(capture, top) != body);
    tmux.run(&["send-keys", "-t", "probe", "/"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "raw_row_40"]);
    tmux.wait_for(|capture| {
        capture.contains("Search ❯ raw_row_40")
            && detail_body(capture, top).contains("\"raw_row_40\",")
    });
    let cursor = || {
        tmux.run(&[
            "display-message",
            "-t",
            "probe",
            "-p",
            "#{cursor_x},#{cursor_y}",
        ])
        .trim()
        .to_owned()
    };
    let before = cursor();
    tmux.run(&["send-keys", "-t", "probe", "Left"]);
    tmux.wait_for(|capture| capture.contains("Search ❯ raw_row_40") && cursor() != before);
    tmux.run(&["send-keys", "-t", "probe", "Right"]);
    tmux.wait_for(|capture| capture.contains("Search ❯ raw_row_40") && cursor() == before);
    tmux.run(&["send-keys", "-t", "probe", "Space"]);
    tmux.wait_for(|capture| {
        capture.contains("Search ❯ raw_row_40")
            && cursor() != before
            && detail_body(capture, top).contains("raw_row_40")
    });
    tmux.run(&["send-keys", "-t", "probe", "BSpace"]);
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.run(&["send-keys", "-t", "probe", "Space"]);
    let restored = tmux.capture_when(|capture| {
        let body = detail_body(capture, top);
        body.contains("Transcript mode probe") && !body.contains("aaa_probe_rows")
    });
    tmux.run(&["send-keys", "-t", "probe", "Space"]);
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    tmux.wait_for(|capture| detail_body(capture, top).contains("aaa_probe_rows"));
    std::fs::write(report.join("claude-end.txt"), end).unwrap();
    std::fs::write(report.join("claude-restored-event.txt"), restored).unwrap();

    let (_codex_directory, codex) = terminal_codex_exit::start_codex_probe(
        "transcript-codex",
        include_str!("../fixtures/codex-raw-detail.jsonl"),
    );
    codex.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "50"]);
    let event = codex
        .capture_when(|capture| capture.lines().count() == 50 && capture.contains("Write stdin"));
    let top = detail_top(&event);
    codex.mouse(0, 90, top + 2, true);
    codex.run(&["send-keys", "-t", "probe", "Space"]);
    codex.run(&["send-keys", "-t", "probe", "Home"]);
    let source = codex.capture_when(|capture| {
        capture.contains("response_item / function_call")
            && detail_body(capture, top).contains("\"call_id\":")
    });
    assert!(
        detail_body(&source, top).contains("\"payload\": {"),
        "{source}"
    );
    let body = detail_body(&source, top);
    assert!(!body.contains('…'), "{source}");
    let keys = [
        "\"type\": \"function_call\"",
        "\"name\":",
        "\"arguments\":",
        "\"call_id\":",
    ];
    let positions = keys.map(|key| body.find(key).expect("original call key is visible"));
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{source}"
    );
    codex.run(&["send-keys", "-t", "probe", "End"]);
    let tail = codex.capture_when(|capture| detail_body(capture, top).contains("819.416125"));
    assert!(!detail_body(&tail, top).contains('…'), "{tail}");
    std::fs::write(report.join("codex-start.txt"), source).unwrap();
    std::fs::write(report.join("codex-end.txt"), tail).unwrap();
    let verdict = serde_json::json!({
        "verdict":"pass", "claude":true, "codex":true,
        "pretty_json":true, "json_syntax_color":true, "space_toggle":true, "multiline_string":true, "long_line_wrap":true,
        "home_end":true, "scroll":true, "search":true,
        "original_key_order":true, "complete_records_without_ellipsis":true,
        "search_caret_keeps_mode":true, "text_selection":true,
        "captures":report,
    });
    std::fs::write(
        report.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
