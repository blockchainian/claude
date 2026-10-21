use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_agent_message_bodies() {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("messages.jsonl");
    let encrypted = include_str!("../fixtures/codex-agent-messages/new-task.jsonl");
    let plaintext = include_str!("../fixtures/codex-agent-messages/final-answer.jsonl");
    let collaboration = [
        (
            include_str!("../fixtures/codex-collaboration/spawn_agent.jsonl"),
            "demo_sub",
        ),
        (
            include_str!("../fixtures/codex-collaboration/send_message.jsonl"),
            "rename_gating",
        ),
        (
            include_str!("../fixtures/codex-collaboration/followup_task.jsonl"),
            "ux_probe",
        ),
    ];
    let calls: String = collaboration.iter().map(|(fixture, _)| *fixture).collect();
    std::fs::write(&transcript, format!("{calls}{encrypted}{plaintext}")).unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?1, name='Agent message probe', cwd=?2, archived=0 WHERE id='thread-0'",
        rusqlite::params![transcript.to_str().unwrap(), directory.path().to_str().unwrap()],
    ).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-message-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "160",
        "-y",
        "45",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|tabs| tabs.contains("Inactive 1"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let plaintext_capture = tmux.capture_when(|capture| {
        capture.contains("demo_sub → 演示完成") && capture.contains("演示完成")
    });
    assert!(
        plaintext_capture.contains("demo_sub ← new task"),
        "{plaintext_capture}"
    );
    let (row, line) = plaintext_capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("demo_sub ← new task"))
        .unwrap();
    let column = line[..line.find("demo_sub ← new task").unwrap()].width();
    tmux.mouse(0, column, row, true);
    let encrypted_capture = tmux.capture_when(|capture| capture.contains("(encrypted)"));
    let record: serde_json::Value = serde_json::from_str(encrypted).unwrap();
    let ciphertext = record["payload"]["content"][1]["encrypted_content"]
        .as_str()
        .unwrap();
    assert!(
        !encrypted_capture.contains(&ciphertext[..32]),
        "{encrypted_capture}"
    );
    let mut captures = format!("{plaintext_capture}\n\n{encrypted_capture}");
    for (fixture, subject) in collaboration {
        let label = match subject {
            "demo_sub" => format!("Spawn agent {subject}"),
            "ux_probe" => format!("{subject} ← followup task"),
            _ => format!("{subject} ← message"),
        };
        let (row, line) = encrypted_capture
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(&label))
            .unwrap();
        let column = line[..line.find(&label).unwrap()].width();
        tmux.mouse(0, column, row, true);
        let event_detail_title = if subject == "demo_sub" {
            subject.to_owned()
        } else {
            format!("← {subject}")
        };
        let capture = tmux.capture_when(|capture| {
            capture.lines().any(|line| {
                line.contains(&event_detail_title)
                    && line.contains(if subject == "demo_sub" {
                        "Spawn agent"
                    } else if subject == "ux_probe" {
                        "Followup task"
                    } else {
                        "Message"
                    })
            }) && capture.contains("(encrypted)")
        });
        let record: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let arguments: serde_json::Value =
            serde_json::from_str(record["payload"]["arguments"].as_str().unwrap()).unwrap();
        let token = arguments["message"].as_str().unwrap();
        assert!(!capture.contains(&token[..32]), "{capture}");
        captures.push_str(&format!("\n\n{capture}"));
    }
    std::fs::write(
        &transcript,
        include_str!("../fixtures/codex-collaboration/spawn_agent.jsonl"),
    )
    .unwrap();
    let activity_capture = tmux.capture_when(|capture| {
        capture.contains("Last activity") && capture.matches("Spawn agent demo_sub").count() >= 2
    });
    captures.push_str(&format!("\n\n{activity_capture}"));
    let capture = std::env::temp_dir().join("agents-message-tmux.txt");
    std::fs::write(&capture, captures).unwrap();
    let verdict =
        serde_json::json!({"verdict":"pass","events":5,"binary":binary,"capture":capture});
    std::fs::write(
        std::env::temp_dir().join("agents-message-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_collaboration_navigation() {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let parent_records = include_str!("../fixtures/codex-interaction/parent.jsonl");
    let child_records = include_str!("../fixtures/codex-interaction/child.jsonl");
    let metadata = |records: &str| -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(records.lines().next().unwrap()).unwrap()["payload"].clone()
    };
    let parent = metadata(parent_records);
    let child = metadata(child_records);
    let parent_path = codex.join("parent.jsonl");
    let child_path = codex.join("child.jsonl");
    std::fs::write(&parent_path, parent_records).unwrap();
    std::fs::write(&child_path, child_records).unwrap();
    std::fs::write(
        codex.join("models_cache.json"),
        include_str!("../fixtures/codex-models-cache.json"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute(
        "DELETE FROM threads WHERE id NOT IN ('thread-0', 'thread-1')",
        [],
    )
    .unwrap();
    for (original, meta, path, name) in [
        ("thread-0", &parent, &parent_path, "Collaboration parent"),
        ("thread-1", &child, &child_path, "Child probe"),
    ] {
        db.execute("UPDATE threads SET id=?1, rollout_path=?2, source=?3, cwd=?4, name=?5, archived=0 WHERE id=?6", rusqlite::params![
            meta["id"].as_str().unwrap(), path.to_str().unwrap(),
            if let Some(source) = meta["source"].as_str() { source.to_owned() } else { meta["source"].to_string() },
            directory.path().to_str().unwrap(), name, original,
        ]).unwrap();
    }
    db.execute("DELETE FROM thread_spawn_edges", []).unwrap();
    db.execute(
        "INSERT INTO thread_spawn_edges VALUES (?1, ?2, 'completed')",
        rusqlite::params![
            parent["id"].as_str().unwrap(),
            child["id"].as_str().unwrap()
        ],
    )
    .unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-collaboration-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "180",
        "-y",
        "48",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.capture_when(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|tabs| tabs.contains("Inactive 2"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let capture = tmux.capture_when(|capture| capture.contains("Collaboration parent"));
    let find = |capture: &str, needle: &str| -> (usize, usize) {
        let (row, line) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(needle))
            .unwrap();
        (line[..line.find(needle).unwrap()].width(), row)
    };
    let (column, row) = find(&capture, "Collaboration parent");
    tmux.mouse(0, column, row, true);
    let capture = tmux.capture_when(|capture| capture.contains("demo_sub → 演示完成"));
    let (column, row) = find(&capture, "Spawn agent demo_sub");
    tmux.mouse(0, column, row, true);
    let spawn = tmux.capture_when(|capture| {
        capture.contains("Spawn agent") && capture.contains("demo_sub · GPT-6.1-Sol · low")
    });
    let event_surface = spawn
        .lines()
        .map(|line| line.chars().skip(90).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!event_surface.contains("SubAgentActivity"), "{spawn}");
    assert!(!event_surface.contains("CollabAgentToolCall"), "{spawn}");
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let jumped = tmux.capture_when(|capture| {
        capture.contains("root → new task") && !capture.contains("demo_sub → 演示完成")
    });
    let (column, row) = find(&jumped, "root → new task");
    tmux.mouse(0, column, row, true);
    let jumped = tmux.capture_when(|capture| capture.contains("New task"));
    assert!(jumped.contains("root → demo_sub"), "{jumped}");
    let (column, row) = find(&jumped, "Collaboration parent");
    tmux.mouse(0, column, row, true);
    let capture = tmux.capture_when(|capture| capture.contains("Wait agent"));
    let (column, row) = find(&capture, "Wait agent");
    tmux.mouse(0, column, row, true);
    let waited = tmux
        .capture_when(|capture| capture.contains("70000") && capture.contains("Wait completed."));
    assert!(waited.contains("GPT-6.1-Sol"), "{waited}");
    let output = std::env::temp_dir().join("agents-collaboration-tmux.txt");
    std::fs::write(&output, format!("{spawn}\n\n{jumped}\n\n{waited}")).unwrap();
    let verdict = serde_json::json!({"verdict":"pass", "binary":binary, "capture":output, "checks":["spawn metadata", "model label", "child selection", "wait result"]});
    std::fs::write(
        std::env::temp_dir().join("agents-collaboration-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_claude_agent_exchange() {
    let mut records = format!(
        "{}\n{}",
        include_str!("../fixtures/transcript.jsonl"),
        include_str!("../fixtures/claude-agent-names.jsonl")
    );
    for line in include_str!("../fixtures/claude-tool-details.jsonl").lines() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        if record["message"]["content"][0]["name"] == "SendMessage" {
            records.push_str(line);
            records.push('\n');
        }
    }
    let peer = include_str!("../fixtures/claude-peer-prompts.jsonl")
        .lines()
        .next()
        .unwrap();
    records.push_str(&format!("{peer}\n{peer}\n"));
    let (_directory, tmux, _) = start_probe_with_transcript("claude-agent-exchange", &records);
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "70"]);
    let capture = tmux.capture_when(|capture| {
        capture.contains("Spawn agent reviewer")
            && capture.contains("hide-idle-teammates ← message")
            && capture.contains("translate-pdf → 2 messages")
    });
    let mut captures = capture.clone();
    for (label, body) in [
        ("Spawn agent reviewer", "Review files"),
        ("hide-idle-teammates ← message", "Is the uncommitted edit"),
        ("translate-pdf → 2 messages", "translate-pdf:"),
    ] {
        let (row, line) = capture
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(label))
            .last()
            .unwrap();
        let column = line[..line.find(label).unwrap()].width();
        tmux.mouse(0, column, row, true);
        let detail = tmux.capture_when(|capture| {
            capture.contains(body)
                && (label != "translate-pdf → 2 messages"
                    || capture
                        .lines()
                        .filter(|line| {
                            line.trim_start().starts_with("Message")
                                && line.contains("translate-pdf →")
                        })
                        .count()
                        == 2)
        });
        if label == "hide-idle-teammates ← message" {
            assert!(
                detail.lines().any(|line| {
                    line.trim_start().starts_with("Message")
                        && line.contains("← hide-idle-teammates")
                }),
                "{detail}"
            );
        }
        captures.push_str(&format!("\n\n{detail}"));
    }
    let verdict = serde_json::json!({"verdict":"pass","provider":"claude","spawn":true,"outgoing":true,"incoming_merged":true,"details":true});
    std::fs::write(
        std::env::temp_dir().join("agents-claude-exchange-probe.txt"),
        captures,
    )
    .unwrap();
    std::fs::write(
        std::env::temp_dir().join("agents-claude-exchange-probe.json"),
        verdict.to_string(),
    )
    .unwrap();
    println!("{verdict}");
}
