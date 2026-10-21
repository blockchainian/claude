use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_wide_golden_layout() {
    let (_directory, tmux, _transcript) = start_probe("golden-layout");
    let mut measurements = Vec::new();
    for (width, height) in [(200, 60), (180, 40), (150, 50), (200, 61)] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ]);
        let capture = tmux.capture_when(|capture| {
            let lines: Vec<_> = capture.lines().collect();
            lines.len() == height
                && lines.iter().any(|line| line.width() == width)
                && lines.last().is_some_and(|line| line.contains("? help"))
        });
        let lines: Vec<_> = capture.lines().collect();
        let top = lines[2..]
            .iter()
            .take_while(|line| line.matches('│').count() == 2)
            .count();
        let prompt = 2 + top + 1;
        assert!(lines[prompt].contains('❯'), "{capture}");
        let event_detail_rule = prompt + 1;
        assert!(
            lines[event_detail_rule].trim().chars().all(|c| c == '─'),
            "{capture}"
        );
        let bottom = height - event_detail_rule - 2;
        let available = height - 6;
        assert_eq!(top, available * 618 / 1000, "{capture}");
        assert_eq!(top + bottom, available, "{capture}");
        measurements.push(serde_json::json!({
            "width": width, "height": height, "top": top, "bottom": bottom,
            "fixed": height - available,
        }));
    }
    let verdict = serde_json::json!({"verdict": "pass", "measurements": measurements});
    if let Some(path) = std::env::var_os("AGENTS_GOLDEN_LAYOUT_RESULT") {
        std::fs::write(path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    }
    println!("{verdict}");
}
