use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_memory_citation_body() {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("reply.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../fixtures/codex-memory-citation/reply.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?1, name='Memory citation probe', cwd=?2, archived=0 WHERE id='thread-0'",
        rusqlite::params![transcript.to_str().unwrap(), directory.path().to_str().unwrap()],
    )
    .unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux(format!("agents-memory-citation-{}", std::process::id()));
    tmux.run(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "220",
        "-y",
        "100",
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
    let capture = tmux.capture_when(|capture| {
        capture.contains("Update")
            && capture.contains("packages/cache/package.json")
            && capture.contains("负责范围内的旧路径已清理")
    });
    let capture_path = std::env::temp_dir().join("agents-memory-citation-tmux.txt");
    std::fs::write(&capture_path, &capture).unwrap();
    let clean = !capture.contains("oai-mem-citation") && !capture.contains("citation_entries");
    // One copy belongs to Update, one to the log detail, and one to the log list.
    let copies = capture.matches("完成，未提交。").count();
    let complete = capture.contains("270→270")
        && capture.contains("docs/axiom/api-spec.md")
        && capture.contains("未改。");
    let verdict = serde_json::json!({
        "verdict": if clean && complete && copies == 3 { "pass" } else { "fail" },
        "clean": clean,
        "complete": complete,
        "body_copies": copies,
        "binary": binary,
        "capture": capture_path,
    });
    let verdict_path = std::env::temp_dir().join("agents-memory-citation-probe.json");
    std::fs::write(&verdict_path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
    assert_eq!(verdict["verdict"], "pass", "{capture}");
}
