use super::*;

#[test]
fn terminal_model_command_refreshes_without_an_assistant_reply() {
    let fixture = include_str!("../fixtures/claude-model-command.jsonl");
    let (assistant, commands) = fixture.split_once('\n').unwrap();
    let before = format!(
        "{}{assistant}\n",
        include_str!("../fixtures/transcript.jsonl")
    );
    let (_directory, tmux, transcript) = start_probe_with_transcript("model-command", &before);
    let model_rule = |screen: &str, model: &str| {
        screen
            .lines()
            .any(|line| line.starts_with(&format!("─╴{model} ")) && line.contains(" medium╶"))
    };
    tmux.wait_for(|screen| model_rule(screen, "claude-fable-5-1"));
    std::fs::write(&transcript, format!("{before}{commands}")).unwrap();
    let after = tmux.capture_when(|screen| model_rule(screen, "claude-opus-5-5"));
    assert!(!after.contains("claude-fable-5-1"), "{after}");
    // Restore the transcript and check the same watched session again.
    std::fs::write(&transcript, &before).unwrap();
    tmux.wait_for(|screen| model_rule(screen, "claude-fable-5-1"));
    let verdict = serde_json::json!({
        "verdict": "pass",
        "surface": "model and effort rule",
        "model_before": "claude-fable-5-1",
        "model_after": "claude-opus-5-5",
        "assistant_reply": false,
        "reverted": true,
    });
    let path = std::env::temp_dir().join("agents-model-command-verdict.json");
    std::fs::write(path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
}
