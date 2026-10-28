use super::*;

#[test]
fn terminal_prompt_cache_countdown_on_task_details_border() {
    let mut borders = Vec::new();
    // Replay a real request with ten minutes of its recorded one-hour TTL left, then expired.
    for (minutes_ago, wall_time) in [(50, "50m"), (70, "1h 10m")] {
        borders.extend(prompt_cache_borders(minutes_ago, wall_time));
    }
    let capture = std::env::temp_dir().join("agents-prompt-cache-border.txt");
    std::fs::write(&capture, borders.join("\n")).unwrap();
    let verdict = serde_json::json!({
        "verdict": "pass", "surfaces": ["details top border"],
        "widths": [200, 149], "color": "error red", "capture": capture,
    });
    std::fs::write(
        std::env::temp_dir().join("agents-prompt-cache-border.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

/// Captures the details border at each width for a request `minutes_ago` minutes old, asserting
/// a red countdown before `wall_time`, or a bold ⏸︎ after the status once the one-hour TTL has
/// passed.
fn prompt_cache_borders(minutes_ago: i64, wall_time: &str) -> Vec<String> {
    let now = chrono::Utc::now();
    let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let request_at = now - chrono::Duration::minutes(minutes_ago);
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
        let expired = minutes_ago > 60;
        let marker = if expired { "Inactive ⏸" } else { " · " };
        let plain = tmux.capture_when(|capture| {
            capture.lines().any(|row| {
                row.width() == usize::from(width)
                    && row.contains("Inactive")
                    && row.contains(marker)
                    && row.contains(wall_time)
            })
        });
        let border = plain
            .lines()
            .find(|row| row.contains("Inactive") && row.contains(marker))
            .unwrap();
        let ansi = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let parser = captured_screen(&ansi, width, 50);
        let screen = parser.screen();
        let (y, row) = screen
            .rows(0, width)
            .enumerate()
            .find(|(_, row)| row.contains("Inactive") && row.contains(marker))
            .unwrap();
        if expired {
            assert!(!border.contains(" · "), "{border}");
            let x = row[..row.find('⏸').unwrap()].width();
            assert!(screen.cell(y as u16, x as u16).unwrap().bold(), "{border}");
        } else {
            let (before, wall) = border.split_once(" · ").unwrap();
            assert!(
                before.ends_with('s') && wall.starts_with(wall_time),
                "{border}"
            );
            let x = row[..row.find(" · ").unwrap()].width() - 1;
            assert_eq!(
                screen.cell(y as u16, x as u16).unwrap().fgcolor(),
                vt100::Color::Idx(1)
            );
        }
        borders.push(border.to_owned());
    }
    borders
}
