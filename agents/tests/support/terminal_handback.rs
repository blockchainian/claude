use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_subagent_handback_list_and_details() {
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/subagent-handback");
    std::fs::create_dir_all(&evidence).unwrap();
    let source = include_str!("../fixtures/claude-subagent-handback.jsonl");
    let mut records: Vec<serde_json::Value> = source
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let message = records[0]["message"]["content"][0]["input"]["message"]
        .as_str()
        .unwrap()
        .to_owned();
    // Display the recorded child call in the probe's single selected session.
    for record in &mut records {
        record["isSidechain"] = false.into();
    }
    let transcript = format!(
        "{}{}",
        include_str!("../fixtures/transcript.jsonl"),
        records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>()
    );
    let (_directory, tmux, _) = start_probe_with_transcript("subagent-handback", &transcript);
    tmux.run(&["resize-window", "-t", "probe", "-x", "240", "-y", "150"]);
    let label = message.split('.').next().unwrap();
    let capture = tmux.capture_when(|capture| {
        capture.lines().any(|line| {
            line.split('│')
                .nth(2)
                .is_some_and(|list| list.contains(label))
        })
    });
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| {
            line.split('│')
                .nth(2)
                .is_some_and(|list| list.contains(label))
        })
        .unwrap();
    assert!(!line.split('│').nth(2).unwrap().contains("SubagentHandback"));
    std::fs::write(evidence.join("list.txt"), &capture).unwrap();
    tmux.mouse(0, line[..line.rfind(label).unwrap()].width(), row, true);
    let capture = tmux.capture_when(|capture| {
        capture.contains("SubagentHandback")
            && capture.contains("Report delivered to your caller.")
            && capture.contains("Cause:")
    });
    assert!(!capture.contains("\"message\":"), "{capture}");
    assert!(!capture.contains("**Cause:**"), "{capture}");
    assert!(!capture.contains("\"success\":"), "{capture}");
    std::fs::write(evidence.join("details.txt"), capture).unwrap();
    std::fs::write(
        evidence.join("verdict.json"),
        serde_json::json!({"verdict":"pass", "list_content":true,
            "markdown_input":true,"message_output":true,"detail_title":true})
        .to_string(),
    )
    .unwrap();
    // Tmux::drop kills this probe's server, including on assertion failure.
}
