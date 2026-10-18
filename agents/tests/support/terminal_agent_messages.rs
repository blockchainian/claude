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
    let tmux = Tmux(format!("agents-message-{}", std::process::id()));
    tmux.run(&[
        "-f",
        "/dev/null",
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
        capture.contains("FINAL_ANSWER from /root/demo_sub") && capture.contains("演示完成")
    });
    assert!(
        plaintext_capture.contains("NEW_TASK from /root"),
        "{plaintext_capture}"
    );
    let (row, line) = plaintext_capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("NEW_TASK from /root"))
        .unwrap();
    let column = line[..line.find("NEW_TASK from /root").unwrap()].width();
    tmux.mouse(0, column, row, true);
    let encrypted_capture = tmux.capture_when(|capture| capture.contains("Content is encrypted."));
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
        let record: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let tool = record["payload"]["name"].as_str().unwrap();
        let label = format!("{tool} {subject}");
        let (row, line) = encrypted_capture
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(&label))
            .unwrap();
        let column = line[..line.find(&label).unwrap()].width();
        tmux.mouse(0, column, row, true);
        let capture = tmux.capture_when(|capture| {
            capture.lines().any(|line| {
                line.contains(subject)
                    && line.contains(if subject == "demo_sub" {
                        "spawn_agent"
                    } else if subject == "ux_probe" {
                        "followup_task"
                    } else {
                        "send_message"
                    })
            }) && capture.contains("Content is encrypted.")
        });
        let record: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let arguments: serde_json::Value =
            serde_json::from_str(record["payload"]["arguments"].as_str().unwrap()).unwrap();
        let token = arguments["message"].as_str().unwrap();
        assert!(!capture.contains(&token[..32]), "{capture}");
        captures.push_str(&format!("\n\n{capture}"));
    }
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
