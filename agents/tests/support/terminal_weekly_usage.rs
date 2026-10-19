use super::*;

#[test]
#[ignore = "requires tmux; starts the real app with isolated preferences and reads account usage"]
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
    let capture = tmux.capture_when(|capture| capture.contains("Weekly "));
    let compact = capture.lines().next().unwrap().to_owned();
    assert!(
        compact.contains("% left") && !compact.contains("until"),
        "{compact}"
    );
    let x = compact[..compact.find("Weekly ").unwrap()].width();
    tmux.mouse(0, x, 0, true);
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Weekly ") && line.contains("until"))
    });
    let detailed = capture.lines().next().unwrap().to_owned();
    for (name, reset) in [
        ("Claude", "2026-10-23T16:00:00Z"),
        ("Codex", "2026-10-25T08:33:00Z"),
    ] {
        let reset = reset.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        let time = reset.with_timezone(&chrono::Local).format("%a %H:%M");
        let percent = if name == "Claude" { 79 } else { 84 };
        assert!(
            detailed.contains(&format!("{name} {percent}% left until {time}")),
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
            .is_some_and(|line| line.contains("Weekly ") && line.contains("until"))
    });
    let line = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let line = line.lines().next().unwrap();
    let start = line.find("Weekly ").unwrap();
    let end = line.rfind("Inactive").unwrap();
    let right = line[start..end].trim_end();
    let last = line[..start].width() + right.width() - 1;
    tmux.mouse(0, last, 0, true);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Weekly ") && !line.contains("until"))
    });
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state_path).unwrap()).unwrap();
    assert_eq!(saved["state"]["usage_view"], "compact");
    let result = serde_json::json!({"verdict":"passed","compact":compact.trim(),"detailed":detailed.trim(),"restart":true,"reverted":true});
    if let Some(path) = std::env::var_os("AGENTS_WEEKLY_TERMINAL_RESULT") {
        std::fs::write(path, serde_json::to_vec(&result).unwrap()).unwrap();
    }
    println!("{result}");
}
