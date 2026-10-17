use super::*;
use std::io::Write;

/// Exercise a private copy of a large real transcript with an agent-owned binary.
/// AGENTS_LIVE_TRANSCRIPT supplies the source; AGENTS_PROBE_BIN selects release.
#[test]
#[ignore = "requires tmux and AGENTS_LIVE_TRANSCRIPT; run with --ignored --nocapture"]
fn terminal_chronological_large_live_transcript() {
    let Some(source) = std::env::var_os("AGENTS_LIVE_TRANSCRIPT") else {
        eprintln!("AGENTS_LIVE_TRANSCRIPT unset; optional large-session walk skipped");
        return;
    };
    let contents = std::fs::read_to_string(source).unwrap();
    assert!(
        contents.lines().count() > 1000,
        "large-session probe needs real history"
    );
    let mut arrival: serde_json::Value = serde_json::from_str(
        include_str!("../fixtures/transcript.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    for width in [200, 100] {
        let (_directory, tmux, transcript) =
            start_probe_with_transcript(&format!("chronological-live-{width}"), &contents);
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        tmux.wait_for(|capture| capture.lines().count() == 50 && capture.contains("Update"));
        let append = |record: &serde_json::Value| {
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&transcript)
                    .unwrap(),
                "{record}"
            )
            .unwrap();
        };
        arrival["uuid"] = format!("terminal-live-pinned-{width}").into();
        arrival["timestamp"] = chrono::Utc::now().to_rfc3339().into();
        arrival["message"]["content"] = "terminal-live-pinned-proof".into();
        append(&arrival);
        tmux.wait_for(|capture| capture.matches("terminal-live-pinned-proof").count() >= 2);
        let pinned = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let geometry = |capture: &str| {
            let rows: Vec<_> = capture.lines().collect();
            let rules: Vec<_> = rows.iter().enumerate()
                .filter_map(|(index, line)| (line.matches('─').count() > width / 2).then_some(index))
                .collect();
            if width >= 150 {
                let separator = rows[2].rfind('│').unwrap();
                (rows[2][..separator].width() + 2, rules[0], rules[1])
            } else {
                (1, rules[rules.len() - 2], rules[rules.len() - 1])
            }
        };
        let (column, top, bottom) = geometry(&pinned);
        let list = |capture: &str| {
            capture
                .lines()
                .skip(top + 1)
                .take(bottom - top - 1)
                .map(|line| {
                    if width >= 150 {
                        line.split('│').next_back().unwrap()
                    } else {
                        line
                    }
                })
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(
            list(&pinned)
                .lines()
                .last()
                .unwrap()
                .contains("terminal-live-pinned-proof"),
            "newest row is not at bottom: {pinned}"
        );
        // A list click owns keyboard navigation; moving upward releases follow.
        tmux.mouse(0, column, bottom - 1, true);
        tmux.run(&["send-keys", "-t", "probe", "PPage"]);
        tmux.wait_for(|capture| capture.contains("↓ Latest"));
        let away = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        arrival["uuid"] = format!("terminal-live-away-{width}").into();
        arrival["timestamp"] = chrono::Utc::now().to_rfc3339().into();
        arrival["message"]["content"] = "terminal-live-away-proof".into();
        append(&arrival);
        tmux.wait_for(|capture| capture.contains("↓ 1 New"));
        let grown = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert_eq!(
            list(&away),
            list(&grown),
            "arrival moved the scrolled viewport"
        );
        assert!(
            !grown
                .lines()
                .skip(bottom + 1)
                .any(|line| line.contains("terminal-live-away-proof")),
            "arrival changed selected detail: {grown}"
        );
        let double_click = |row: usize| {
            let x = column + (width - column) / 3 + 1;
            let y = row + 1;
            tmux.run(&[
                "send-keys",
                "-t",
                "probe",
                "-l",
                &format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").repeat(2),
            ]);
        };
        double_click(top);
        tmux.wait_for(|capture| capture.contains("↓ 1 New") && list(capture) != list(&grown));
        let oldest = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            !list(&oldest).contains("terminal-live-away-proof"),
            "top border stayed at newest: {oldest}"
        );
        double_click(bottom);
        tmux.wait_for(|capture| {
            capture.matches("terminal-live-away-proof").count() >= 2 && !capture.contains("↓ 1 New")
        });
        let latest = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            list(&latest)
                .lines()
                .last()
                .unwrap()
                .contains("terminal-live-away-proof"),
            "bottom border missed latest: {latest}"
        );
        for (name, capture) in [
            ("pinned", pinned),
            ("away", grown),
            ("oldest", oldest),
            ("latest", latest),
        ] {
            let path =
                std::env::temp_dir().join(format!("agents-chronological-live-{width}-{name}.txt"));
            std::fs::write(path, capture).unwrap();
        }
        println!(
            "{{\"chronological_live\":\"pass\",\"width\":{width},\"pinned_append\":\"pass\",\"away_anchor\":\"pass\",\"new_count\":\"pass\",\"top_oldest\":\"pass\",\"bottom_latest\":\"pass\"}}"
        );
    }
}
