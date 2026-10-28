use super::*;

#[test]
fn terminal_pending_questions_wrap_in_full() {
    let (directory, tmux, transcript) = start_probe("pending-question-wrap");
    let parent = transcript.file_stem().unwrap().to_str().unwrap();
    let rollout = directory.path().join("codex/pending.jsonl");
    std::fs::write(
        &rollout,
        include_str!("../fixtures/codex-command-failed.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(directory.path().join("codex/state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute("UPDATE threads SET rollout_path=?1, name='Pending question probe', cwd=?2, archived=0 WHERE id='thread-0'",
        rusqlite::params![rollout.to_str().unwrap(), directory.path().to_str().unwrap()]).unwrap();
    let manager = directory.path().join(parent);
    std::fs::create_dir(&manager).unwrap();
    std::fs::create_dir(directory.path().join("threads")).unwrap();
    let mut state: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/codex-manager/attached-state.json"
    ))
    .unwrap();
    state["threads"][0]["id"] = "thread-0".into();
    std::fs::write(
        manager.join("state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let mut supervisor: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/codex-manager/attached-supervisor.json"
    ))
    .unwrap();
    supervisor["sessionId"] = parent.into();
    std::fs::write(
        directory.path().join("threads/thread-0.json"),
        serde_json::to_vec(&supervisor).unwrap(),
    )
    .unwrap();
    let ask_path = manager.join("thread-0.ask.pending.json");
    std::fs::write(&ask_path, include_str!("../fixtures/supervision-ask.json")).unwrap();
    tmux.run(&["resize-window", "-t", "probe", "-x", "120", "-y", "80"]);
    tmux.capture_when(|capture| capture.lines().next().unwrap().contains("Active 2"));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let selected = tmux.capture_when(|capture| capture.contains("Pending question probe"));
    let (row, line) = selected
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Pending question probe"))
        .unwrap();
    let column = line[..line.find("Pending question probe").unwrap()].width();
    tmux.mouse(0, column, row, true);
    let ask_capture = tmux.capture_when(|capture| capture.contains("Needs input"));
    let ask: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/supervision-ask.json")).unwrap();
    let full_text = |capture: &str| {
        let panel = capture
            .lines()
            .filter_map(|line| line.split('│').nth(1))
            .collect::<Vec<_>>()
            .join("\n");
        panel
            .split_once("Needs input")
            .unwrap()
            .1
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '`')
            .collect::<String>()
    };
    let expected: String = ask["text"]
        .as_str()
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(full_text(&ask_capture).contains(&expected), "{ask_capture}");
    assert!(
        !ask_capture.contains("Open task to review."),
        "{ask_capture}"
    );
    let record: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/claude-questions-pending.jsonl")).unwrap();
    let mut long_ask = ask;
    long_ask["text"] = record["message"]["content"][0]["input"]["questions"][0]["question"].clone();
    std::fs::write(&ask_path, serde_json::to_vec(&long_ask).unwrap()).unwrap();
    let long_capture = tmux.capture_when(|capture| capture.contains("怎么判断"));
    let expected: String = long_ask["text"]
        .as_str()
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '`')
        .collect();
    assert!(
        full_text(&long_capture).contains(&expected),
        "{long_capture}"
    );
    let evidence = serde_json::json!({"verdict": "pass", "ask_capture": ask_capture, "long_capture": long_capture, "width": 120});
    std::fs::write(
        std::env::temp_dir().join("agents-pending-question-wrap.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    println!("{evidence}");
}
