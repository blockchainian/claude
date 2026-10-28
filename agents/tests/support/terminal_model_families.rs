use super::*;

fn model_picker_visible(capture: &str) -> bool {
    capture.lines().any(|line| {
        line.split('│')
            .any(|part| matches!(part.trim(), "Opus" | "Sol"))
    })
}

fn position(capture: &str, text: &str) -> (usize, usize) {
    capture
        .lines()
        .enumerate()
        .find_map(|(row, line)| {
            line.split('│')
                .any(|part| part.trim() == text)
                .then(|| (line[..line.find(text).unwrap()].width(), row))
        })
        .unwrap_or_else(|| panic!("missing {text:?}: {capture}"))
}

fn open_model(tmux: &Tmux, label: &str) {
    let capture = tmux.capture_when(|screen| {
        !model_picker_visible(screen)
            && screen
                .lines()
                .any(|line| line.contains(label) && line.contains("medium"))
    });
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(label) && line.contains("medium"))
        .unwrap();
    tmux.mouse(0, line[..line.find(label).unwrap()].width(), row, true);
    tmux.wait_for(model_picker_visible);
}

#[test]
fn terminal_model_families_keep_headers_out_of_selection() {
    use sha2::{Digest, Sha256};
    let output = std::env::var_os("AGENTS_PROBE_CAPTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-model-families"));
    std::fs::create_dir_all(&output).unwrap();
    let mut checks = Vec::new();
    for (provider, id, current, next, header, groups) in [
        (
            "claude",
            "claude-opus-5",
            "Opus 5",
            "Fable 5.1",
            "Fable",
            vec!["Opus", "Fable", "Sonnet", "Haiku"],
        ),
        (
            "codex",
            "gpt-5.6-sol",
            "GPT-5.6-Sol",
            "GPT-6-Astra",
            "Astra",
            vec!["Sol", "Astra", "Luna", "Terra"],
        ),
    ] {
        let (directory, tmux, _) = start_probe_with_setup(
            &format!("model-families-{provider}"),
            include_str!("../fixtures/transcript.jsonl"),
            |directory| {
                let config = directory.join("claude");
                std::fs::write(
                    config.join(".claude.json"),
                    r#"{"oauthAccount":{"organizationUuid":"org-1","accountUuid":"ACCOUNT-1"}}"#,
                )
                .unwrap();
                let hash = Sha256::digest(b"account-1")
                    .iter()
                    .take(6)
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let cache = config.join("cache/model-catalog");
                std::fs::create_dir_all(&cache).unwrap();
                std::fs::write(
                    cache.join(format!("org-1-{hash}-cc.json")),
                    include_str!("../fixtures/claude-model-families.json"),
                )
                .unwrap();
                std::fs::write(
                    directory.join("codex/models_cache.json"),
                    include_str!("../fixtures/codex-model-families.json"),
                )
                .unwrap();
                std::fs::write(config.join("agents.json"), serde_json::to_vec(&serde_json::json!({
                    "config":{}, "state":{"drafts":[{
                        "id":"draft-1792390000000-1", "provider":provider, "name":"Family probe",
                        "input":{"text":"", "images":[]}, "cwd":directory,
                        "repository_confirmed":false, "branch":"unchosen", "model":id,
                        "effort":"medium", "job":null, "error":null, "updated_at":"2026-10-19T09:00:00Z"
                    }]}
                })).unwrap()).unwrap();
            },
        );
        tmux.run(&["send-keys", "-t", "probe", "Tab"]);
        tmux.wait_for(|screen| screen.contains("Family probe ❯"));
        open_model(&tmux, current);
        let capture = tmux.capture_when(|screen| {
            groups.iter().all(|group| {
                screen
                    .lines()
                    .any(|line| line.split('│').any(|part| part.trim() == *group))
            })
        });
        std::fs::write(output.join(format!("{provider}.txt")), &capture).unwrap();
        let ansi = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        std::fs::write(output.join(format!("{provider}.ansi")), &ansi).unwrap();
        let parser = captured_screen(&ansi, 200, 50);
        let (x, y) = position(&capture, current);
        assert_ne!(
            parser.screen().cell(y as u16, x as u16).unwrap().bgcolor(),
            vt100::Color::Default,
            "current model must start selected: {ansi}"
        );
        let rows: Vec<_> = groups
            .iter()
            .map(|group| position(&capture, group).1)
            .collect();
        assert!(rows.windows(2).all(|pair| pair[0] < pair[1]), "{capture}");
        for group in &groups {
            let (x, y) = position(&capture, group);
            assert_eq!(
                parser.screen().cell(y as u16, x as u16).unwrap().bgcolor(),
                vt100::Color::Default
            );
            let line = ansi.lines().nth(y).unwrap();
            assert!(
                line.contains("[2m") || line.contains(";2m"),
                "family title must be dim: {line:?}"
            );
        }
        let (x, y) = position(&capture, header);
        tmux.mouse(0, x, y, true);
        tmux.wait_for(model_picker_visible);
        tmux.run(&["send-keys", "-t", "probe", "Enter"]);
        tmux.wait_for(|screen| !model_picker_visible(screen) && screen.contains(current));
        let stored: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("claude/agents.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(stored["state"]["drafts"][0]["model"], id);
        open_model(&tmux, current);
        tmux.run(&["send-keys", "-t", "probe", "Down"]);
        tmux.run(&["send-keys", "-t", "probe", "Enter"]);
        tmux.wait_for(|screen| !model_picker_visible(screen) && screen.contains(next));
        open_model(&tmux, next);
        tmux.run(&["send-keys", "-t", "probe", "Up"]);
        tmux.run(&["send-keys", "-t", "probe", "Enter"]);
        tmux.wait_for(|screen| !model_picker_visible(screen) && screen.contains(current));
        let stored: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("claude/agents.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(stored["state"]["drafts"][0]["model"], id);
        println!("{provider} MODEL CAPTURE\n{capture}");
        checks.push(serde_json::json!({"provider":provider,"family_order":groups,"dim_headers":true,"header_click_ignored":true,"current_selected_in_place":true,"up_down_skip_headers":true,"enter_roundtrip_reverted":true}));
    }
    let verdict = serde_json::json!({"verdict":"pass","checks":checks});
    std::fs::write(
        output.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
