use super::*;

fn save_mcp_capture(tmux: &Tmux, provider: &str, case: &str) -> String {
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let path = std::env::temp_dir().join(format!("agents-mcp-{provider}-{case}.ansi"));
    std::fs::write(&path, &capture).unwrap();
    let verdict = serde_json::json!({
        "verdict": "pass", "provider": provider, "case": case, "capture": path,
    });
    let verdict_path = std::env::temp_dir().join(format!("agents-mcp-{provider}-{case}.json"));
    std::fs::write(&verdict_path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
    capture
}

fn token_color(capture: &str, token: &str, width: u16, height: u16) -> vt100::Color {
    let parser = captured_screen(capture, width, height);
    let screen = parser.screen();
    let (row, text) = screen
        .rows(0, width)
        .enumerate()
        .find(|(_, text)| text.contains(token))
        .unwrap_or_else(|| panic!("missing token {token}: {capture}"));
    let column = text[..text.find(token).unwrap()].width();
    screen.cell(row as u16, column as u16).unwrap().fgcolor()
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_claude_mcp_generic_event_detail() {
    use std::io::Write;
    let (_directory, tmux, transcript) = start_probe("mcp-claude-generic");
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "150"]);
    for (case, fixture, needles) in [
        (
            "nested",
            include_str!("../fixtures/mcp-claude/nested.jsonl"),
            vec![
                "\"query\": {",
                "\"dry\": false",
                "\"limit\": 10",
                "\"datasets\": [",
            ],
        ),
        (
            "image",
            include_str!("../fixtures/mcp-claude/image.jsonl"),
            vec!["\"actions\": [", "\"duration\": 5", "[image image/jpeg]"],
        ),
        (
            "multiline",
            include_str!("../fixtures/claude-manager-start.jsonl"),
            vec!["Implement workstream A", "Rules:", "Claude 图片链接"],
        ),
    ] {
        std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap()
            .write_all(fixture.as_bytes())
            .unwrap();
        tmux.run(&["send-keys", "-t", "probe", "End"]);
        let plain =
            tmux.capture_when(|capture| needles.iter().all(|needle| capture.contains(needle)));
        assert!(
            !plain.contains("\\n"),
            "escaped newlines in {case}: {plain}"
        );
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        if case == "nested" {
            let key = token_color(&capture, "dry", 200, 150);
            let boolean = token_color(&capture, "false", 200, 150);
            let number = token_color(&capture, "10,", 200, 150);
            let string = token_color(&capture, "events", 200, 150);
            assert_ne!(key, string, "JSON keys and strings have separate colors");
            for color in [key, string, number, boolean] {
                assert_ne!(
                    color,
                    vt100::Color::Default,
                    "JSON tokens have theme colors"
                );
            }
        }
        if case == "image" {
            let parser = captured_screen(&capture, 200, 150);
            let screen = parser.screen();
            let (row, text) = screen
                .rows(0, 200)
                .enumerate()
                .find(|(_, text)| text.contains("[image image/jpeg]"))
                .unwrap();
            let column = text[..text.find("[image image/jpeg]").unwrap()].width();
            assert!(screen.cell(row as u16, column as u16).unwrap().dim());
            assert!(
                !plain.contains("[redacted image data]"),
                "image bytes must stay hidden"
            );
        }
        save_mcp_capture(&tmux, "claude", case);
    }
}

fn start_codex_mcp_probe(fixture: &str) -> (tempfile::TempDir, Tmux) {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("mcp.jsonl");
    std::fs::write(&transcript, fixture).unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?1, name='MCP details probe', cwd=?2, archived=0 WHERE id='thread-0'",
        rusqlite::params![transcript.to_str().unwrap(), directory.path().to_str().unwrap()],
    ).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux(format!("agents-mcp-codex-{}", std::process::id()));
    tmux.run(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "200",
        "-y",
        "150",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|tabs| tabs.contains("Inactive 1"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    (directory, tmux)
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_mcp_generic_event_detail() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/mcp-codex/structured.json")).unwrap();
    let (directory, tmux) = start_codex_mcp_probe(&format!("{fixture}\n"));
    let structured = &fixture["payload"]["item"]["result"]["structuredContent"];
    let keys: Vec<_> = structured
        .as_object()
        .unwrap()
        .keys()
        .map(|key| format!("\"{key}\":"))
        .collect();
    let plain = tmux.capture_when(|capture| {
        capture.contains("Action completed.") && keys.iter().all(|key| capture.contains(key))
    });
    let action_row = plain
        .lines()
        .position(|line| line.contains("Action completed."))
        .unwrap();
    let structured_row = plain
        .lines()
        .position(|line| line.contains(&keys[0]))
        .unwrap();
    assert!(
        action_row < structured_row,
        "structured result follows text: {plain}"
    );
    assert!(!plain.contains("\\n"), "escaped newlines: {plain}");
    let capture = save_mcp_capture(&tmux, "codex", "structured");
    assert!(capture.contains("Action completed."));
    drop(directory);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_codex_claude_client_text() {
    for (tool, fixture, input_marker, reply_marker) in [
        (
            "ask_claude",
            include_str!("../fixtures/claude-client/ask_claude.jsonl"),
            "状态线程正在",
            "按函数拆补丁",
        ),
        (
            "notify_claude",
            include_str!("../fixtures/claude-client/notify_claude.jsonl"),
            "Step 4 provider/App/UI",
            "Delivered to Claude.",
        ),
    ] {
        let (_directory, tmux) = start_codex_mcp_probe(fixture);
        let capture = tmux.capture_when(|capture| {
            capture.contains(input_marker) && capture.contains(reply_marker)
        });
        assert!(!capture.contains("\"text\":"), "{capture}");
        assert!(
            capture.matches(input_marker).count() >= 2,
            "list and detail: {capture}"
        );
        save_mcp_capture(&tmux, "codex", tool);
    }
}
