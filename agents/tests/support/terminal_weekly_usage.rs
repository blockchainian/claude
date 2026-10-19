use super::*;

#[test]
#[ignore = "requires tmux; starts the real app with isolated preferences and cached usage"]
fn terminal_weekly_usage_cache_toggle_and_restart() {
    let initial = serde_json::json!({
        "config": {"grouping":"project"},
        "state": {"usage_view":"compact", "usage":{"weekly":{
            "claude":{"used_percent":21,"resets_at":"2026-10-23T15:59:59.837Z","reset_credits":1},
            "codex":{"used_percent":16,"resets_at":"2026-10-25T08:32:50Z","reset_credits":2}
        }}}
    });
    let (directory, tmux, _) = start_probe_with_setup(
        "weekly-usage",
        include_str!("../fixtures/transcript.jsonl"),
        |directory| {
            std::fs::write(directory.join("claude/agents.json"), initial.to_string()).unwrap()
        },
    );
    let state_path = directory.path().join("claude/agents.json");
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("21%") && line.contains("16%"))
    });
    let compact = capture.lines().next().unwrap().to_owned();
    assert!(
        compact.contains('%') && !compact.contains("left") && !compact.contains("until"),
        "{compact}"
    );
    assert_compact(&compact, 200);
    let x = 200 / 2;
    tmux.mouse(0, x, 0, true);
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("resets"))
    });
    let detailed = capture.lines().next().unwrap().to_owned();
    assert_detailed(&detailed, 200);
    for (name, reset) in [
        ("Claude", "2026-10-23T16:00:00Z"),
        ("Codex", "2026-10-25T08:33:00Z"),
    ] {
        let reset = reset.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        let time = reset.with_timezone(&chrono::Local).format("%a %H:%M");
        let credits = if name == "Claude" {
            "1 credit"
        } else {
            "2 credits"
        };
        assert!(
            detailed.contains(&format!("{name} resets {time}, {credits}")),
            "{detailed}"
        );
    }

    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(saved["state"]["usage_view"], "detailed");
    assert_eq!(saved["config"], initial["config"]);
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    tmux.run(&[
        "respawn-pane",
        "-k",
        "-t",
        "probe",
        binary.to_str().unwrap(),
    ]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("resets"))
    });
    let line = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let line = line.lines().next().unwrap();
    let last = line[..line.rfind(" credits").unwrap()].width() + " credits".width() - 1;
    tmux.mouse(0, last, 0, true);
    tmux.wait_for(|capture| {
        capture.lines().next().is_some_and(|line| {
            line.contains("21%") && line.contains("16%") && !line.contains("resets")
        })
    });
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state_path).unwrap()).unwrap();
    assert_eq!(saved["state"]["usage_view"], "compact");
    let mut captures = vec![serde_json::json!({"width":200,"compact":compact,"detailed":detailed})];
    tmux.run(&["resize-window", "-t", "probe", "-x", "80", "-y", "50"]);
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.width() <= 80 && line.contains("21%") && line.contains("16%"))
    });
    let compact = capture.lines().next().unwrap().to_owned();
    assert_compact(&compact, 80);
    tmux.mouse(0, 40, 0, true);
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.width() <= 80 && !line.contains('%'))
    });
    let detailed = capture.lines().next().unwrap().to_owned();
    assert!(
        !detailed.contains("Weekly") && !detailed.contains("resets") && !detailed.contains('·'),
        "{detailed}"
    );
    assert!(
        detailed.contains("Active ") && detailed.contains("Inactive "),
        "{detailed}"
    );
    let saved: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("claude/agents.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["state"]["usage_view"], "detailed");
    // Restore through the visible wide target, then confirm the narrow view again.
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "50"]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("resets"))
    });
    tmux.mouse(0, 100, 0, true);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("21%") && line.contains("16%"))
    });
    tmux.run(&["resize-window", "-t", "probe", "-x", "80", "-y", "50"]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.width() <= 80 && line.contains("21%") && line.contains("16%"))
    });
    let saved: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("claude/agents.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["state"]["usage_view"], "compact");
    captures.push(serde_json::json!({"width":80,"compact":compact,"detailed":detailed}));
    let result =
        serde_json::json!({"verdict":"passed","captures":captures,"restart":true,"reverted":true});
    if let Some(path) = std::env::var_os("AGENTS_WEEKLY_TERMINAL_RESULT") {
        std::fs::write(path, serde_json::to_vec(&result).unwrap()).unwrap();
    }
    println!("{result}");
}

fn assert_centered(line: &str, width: usize) {
    assert_eq!(line[..line.find('·').unwrap()].width(), width / 2, "{line}");
    assert!(line.trim_start().starts_with("Active "), "{line}");
    assert!(line.contains("Inactive "), "{line}");
    assert!(!line.contains("Weekly"), "{line}");
}

fn assert_compact(line: &str, width: usize) {
    assert_centered(line, width);
    assert!(line.contains("21% Claude · Codex 16%"), "{line}");
    assert!(
        !line.contains("reset") && !line.contains("credit"),
        "{line}"
    );
    let cells = line.chars().collect::<Vec<_>>();
    assert!(
        cells[..width / 2].contains(&'⣿') && cells[width / 2 + 1..].contains(&'⣿'),
        "{line}"
    );
}

fn assert_detailed(line: &str, width: usize) {
    assert_centered(line, width);
    assert!(
        line.contains("Claude resets ") && line.contains("Codex resets "),
        "{line}"
    );
    assert!(!line.contains('%') && !line.contains('█'), "{line}");
}
