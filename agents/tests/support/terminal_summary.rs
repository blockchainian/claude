use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_summary_styles_match_across_all_three_surfaces() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    let id = "4165651d-b9e3-458d-8612-f8114e312363";
    let children = project.join(id).join("subagents");
    std::fs::create_dir_all(&children).unwrap();
    std::fs::create_dir_all(config.join("sessions")).unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    let records: Vec<serde_json::Value> = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let result = records
        .iter()
        .find(|record| {
            record["message"]["content"][0]["content"]
                .as_str()
                .is_some_and(|text| text.starts_with("Exit code 1\n"))
        })
        .unwrap();
    let call_id = &result["message"]["content"][0]["tool_use_id"];
    let call = records
        .iter()
        .find(|record| record["message"]["content"][0]["id"] == *call_id)
        .unwrap();
    let summary = format!(
        "{} · 1",
        call["message"]["content"][0]["input"]["description"]
            .as_str()
            .unwrap()
    );
    let transcript = [call, result]
        .into_iter()
        .map(|record| {
            let mut record = record.clone();
            record["sessionId"] = id.into();
            record["cwd"] = directory.path().to_str().unwrap().into();
            format!("{record}\n")
        })
        .collect::<String>();
    std::fs::write(project.join(format!("{id}.jsonl")), &transcript).unwrap();
    std::fs::write(children.join("agent-summary.jsonl"), &transcript).unwrap();
    let mut entry: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/registry.json")).unwrap();
    let started = Command::new("ps")
        .env("TZ", "UTC")
        .args(["-p", &std::process::id().to_string(), "-o", "lstart="])
        .output()
        .unwrap();
    assert!(started.status.success());
    entry["pid"] = std::process::id().into();
    entry["procStart"] = String::from_utf8(started.stdout).unwrap().trim().into();
    entry["sessionId"] = id.into();
    entry["cwd"] = directory.path().to_str().unwrap().into();
    entry["status"] = "idle".into();
    entry["name"] = "Summary probe".into();
    std::fs::write(config.join("sessions/probe.json"), entry.to_string()).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-summary-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "240",
        "-y",
        "70",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", config.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.capture_when(|capture| {
        capture.contains("Summary probe")
            && capture.contains("Background")
            && capture.contains("Last activity")
            && capture.matches(&summary).count() >= 3
    });
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parsed = captured_screen(&capture, 240, 70);
    let screen = parsed.screen();
    let mut statuses = 0;
    for (y, row) in screen.rows(0, 240).enumerate() {
        for (byte, _) in row.match_indices(&summary) {
            let x = (row[..byte].width() + summary.width() - 3) as u16;
            assert_eq!(
                screen.cell(y as u16, x + 2).unwrap().fgcolor(),
                vt100::Color::Idx(1)
            );
            assert_ne!(
                screen.cell(y as u16, x).unwrap().fgcolor(),
                vt100::Color::Idx(1)
            );
            statuses += 1;
        }
    }
    assert_eq!(statuses, 3, "{capture}");
    let verdict = serde_json::json!({"verdict":"pass", "surfaces":3, "red_exit_codes":statuses, "binary":binary});
    std::fs::write(
        std::env::temp_dir().join("agents-summary-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    std::fs::write(
        std::env::temp_dir().join("agents-summary-probe.txt"),
        capture,
    )
    .unwrap();
    println!("{verdict}");
}
