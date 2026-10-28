use super::*;

fn output_border_transcript() -> String {
    let base = include_str!("../fixtures/transcript.jsonl");
    let mut event = base
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["type"] == "assistant")
        .unwrap();
    event["uuid"] = "output-border-event".into();
    event["message"]["id"] = "output-border-message".into();
    event["message"]["content"] = serde_json::json!([{
        "type": "text",
        "text": (1..=80).map(|row| format!("output border row {row:02}"))
            .collect::<Vec<_>>().join("  \n"),
    }]);
    event["timestamp"] = "2026-10-18T01:00:00Z".into();
    format!("{base}\n{event}\n")
}

#[test]
fn terminal_output_border_double_clicks() {
    let mut evidence = Vec::new();
    for width in [100, 200] {
        let (_directory, tmux, _) = start_probe_with_transcript(
            &format!("output-border-{width}"),
            &output_border_transcript(),
        );
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        let initial = tmux.capture_when(|capture| {
            capture.lines().count() == 50
                && capture
                    .lines()
                    .nth(1)
                    .is_some_and(|row| row.width() == width)
                && capture.contains("output border row")
        });
        let rows = initial.lines().collect::<Vec<_>>();
        let bottom = 48;
        assert!(
            rows[bottom].chars().all(|ch| ch == '─') && rows[bottom].width() == width,
            "output needs a full-width rule immediately above footer hints: {initial}"
        );
        let top = rows[..bottom]
            .iter()
            .rposition(|row| row.chars().filter(|&ch| ch == '─').count() > width / 2)
            .expect("output top rule");
        let output = |capture: &str| {
            capture
                .lines()
                .skip(top + 1)
                .take(bottom - top - 1)
                .collect::<Vec<_>>()
                .join("\n")
        };
        // The right side also overlaps the event list's bottom rule in narrow mode.
        let column = (width * 3 / 4..width - 2)
            .find(|&column| rows[top].chars().nth(column) == Some('─'))
            .expect("plain top rule cell");
        let double_click = |row| {
            let sequence = format!(
                "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                column + 1,
                row + 1,
                column + 1,
                row + 1,
            )
            .repeat(2);
            tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
        };
        double_click(bottom);
        let end = tmux.capture_when(|capture| output(capture).contains("output border row 80"));
        assert!(!output(&end).contains("output border row 01"), "{end}");
        // Paging must follow the focus set by the border click.
        tmux.run(&["send-keys", "-t", "probe", "C-b"]);
        tmux.capture_when(|capture| {
            output(capture) != output(&end) && output(capture).contains("output border row")
        });
        double_click(top);
        let start = tmux.capture_when(|capture| {
            output(capture).contains("output border row 01")
                && !output(capture).contains("output border row 80")
        });
        tmux.run(&["send-keys", "-t", "probe", "C-f"]);
        tmux.capture_when(|capture| {
            output(capture) != output(&start) && output(capture).contains("output border row")
        });
        double_click(bottom);
        let restored =
            tmux.capture_when(|capture| output(capture).contains("output border row 80"));
        let path = std::env::temp_dir().join(format!("agents-output-border-{width}.txt"));
        std::fs::write(&path, &restored).unwrap();
        evidence.push(serde_json::json!({
            "width":width, "top":top, "bottom":bottom, "top_reached":true,
            "bottom_reached":true, "border_sets_output_focus":true, "capture":path,
        }));
    }
    let verdict = serde_json::json!({"verdict":"pass", "evidence":evidence});
    let path = std::env::var_os("AGENTS_OUTPUT_BORDER_VERDICT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-output-border-verdict.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
}
