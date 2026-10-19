use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_prompt_cache_countdown_on_details_border() {
    let now = chrono::Utc::now();
    let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // Replay a real request with ten minutes of its recorded one-hour TTL left.
    let request_at = now - chrono::Duration::minutes(50);
    records[0]["timestamp"] = request_at.to_rfc3339().into();
    records[1]["timestamp"] = (request_at + chrono::Duration::seconds(17))
        .to_rfc3339()
        .into();
    let contents: String = records.iter().map(|record| format!("{record}\n")).collect();
    let (_directory, tmux, _transcript) = start_probe_with_transcript("prompt-cache", &contents);
    let mut borders = Vec::new();
    for width in [200, 149] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        let plain = tmux.capture_when(|capture| {
            capture.lines().any(|row| {
                row.width() == usize::from(width) && row.contains("Inactive")
                    && row.contains(" · ") && row.contains("50m")
            })
        });
        let border = plain
            .lines()
            .find(|row| row.contains("Inactive") && row.contains(" · "))
            .unwrap();
        let (before, wall) = border.split_once(" · ").unwrap();
        assert!(before.ends_with('s') && wall.starts_with("50m"), "{border}");
        let ansi = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let parser = captured_screen(&ansi, width, 50);
        let screen = parser.screen();
        let (y, row) = screen
            .rows(0, width)
            .enumerate()
            .find(|(_, row)| row.contains("Inactive") && row.contains(" · "))
            .unwrap();
        let separator = row.find(" · ").unwrap();
        let x = row[..separator].width() - 1;
        assert_eq!(
            screen.cell(y as u16, x as u16).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
        borders.push(border.to_owned());
    }
    let capture = std::env::temp_dir().join("agents-prompt-cache-border.txt");
    std::fs::write(&capture, borders.join("\n")).unwrap();
    let verdict = serde_json::json!({
        "verdict": "pass", "surfaces": ["details top-right border"],
        "widths": [200, 149], "color": "error red", "capture": capture,
    });
    std::fs::write(
        std::env::temp_dir().join("agents-prompt-cache-border.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
