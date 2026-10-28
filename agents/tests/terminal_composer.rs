use std::{
    path::Path,
    time::{Duration, Instant},
};

#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;
impl Tmux {
    fn wait(&self, ready: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let screen = self.run(&["capture-pane", "-t", "probe", "-p"]);
            if ready(&screen) {
                return screen;
            }
            assert!(Instant::now() < deadline, "terminal timeout: {screen}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn literal(&self, text: &str) {
        self.run(&["send-keys", "-t", "probe", "-l", "--", text]);
    }
    fn key(&self, key: &str) {
        self.run(&["send-keys", "-t", "probe", key]);
    }
}
#[test]
#[ignore = "requires tmux"]
fn enhanced_keys_newlines_and_vertical_caret_in_tmux() {
    probe("csi-u");
}

#[test]
#[ignore = "requires tmux"]
fn xterm_tmux_keeps_fallbacks_and_dashboard_keys() {
    probe("xterm");
}

/// Writes one Codex task named Composer into `directory` and returns its Codex and Claude homes.
fn composer_session(directory: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let codex = directory.join("codex");
    let claude = directory.join("claude");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(&claude).unwrap();
    let transcript = codex.join("rollout.jsonl");
    std::fs::write(
        &transcript,
        include_str!("fixtures/codex-command-failed.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?1, cwd=?2, archived=0, name='Composer'",
        rusqlite::params![transcript.to_str().unwrap(), directory.to_str().unwrap()],
    )
    .unwrap();
    (codex, claude)
}

fn probe(format: &str) {
    let directory = tempfile::tempdir().unwrap();
    let (codex, claude) = composer_session(directory.path());
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let output = directory.path().join("terminal.raw");
    let tmux = Tmux::new(format!("agents-composer-{format}-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "100",
        "-y",
        "40",
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        "sh",
        "-c",
        "read go; exec \"$1\"",
        "sh",
        binary.to_str().unwrap(),
    ]);
    tmux.run(&["set-option", "-g", "remain-on-exit", "on"]);
    tmux.run(&["set-option", "-s", "extended-keys", "on"]);
    tmux.run(&["set-option", "-s", "extended-keys-format", format]);
    tmux.run(&[
        "pipe-pane",
        "-t",
        "probe",
        &format!(
            "cat > '{}'",
            output.to_str().unwrap().replace('\'', "'\\''")
        ),
    ]);
    tmux.key("Enter");
    tmux.wait(|s| s.contains("Inactive 1"));
    tmux.key("Tab");
    tmux.wait(|s| s.contains("Composer ❯"));
    let raw = std::fs::read_to_string(&output).unwrap();
    assert_eq!(raw.contains("\x1b[>4;2m"), format == "csi-u");
    assert!(
        raw.contains("\x1b[>1u"),
        "keyboard enhancement was not pushed"
    );
    tmux.literal("\x1b[9;2u");
    tmux.wait(|s| s.contains("No task ❮"));
    tmux.literal("\x1b[9u");
    tmux.wait(|s| s.contains("Composer ❯"));
    tmux.literal("?");
    tmux.wait(|s| s.contains("Navigate") && s.contains("Tasks"));
    tmux.key("Escape");
    tmux.wait(|s| s.contains("Composer ❯"));
    // Focus via a real SGR mouse report on the prompt row.
    let screen = tmux.wait(|s| s.contains("Composer ❯"));
    let row = screen
        .lines()
        .position(|line| line.contains("Composer ❯"))
        .unwrap();
    tmux.literal(&format!("\x1b[<0;15;{}M\x1b[<0;15;{}m", row + 1, row + 1));
    tmux.literal("alpha");
    tmux.wait(|s| s.contains("❯ alpha"));
    if format == "csi-u" {
        tmux.key("S-Enter"); // tmux encodes the modified key using the negotiated mode.
    } else {
        tmux.literal("\x1b\r"); // Legacy tmux uses Alt+Enter as its newline fallback.
    }
    tmux.literal("beta");
    tmux.wait(|s| s.contains("❯ alpha") && s.lines().any(|l| l.trim() == "beta"));
    tmux.key("Up");
    tmux.literal("X");
    tmux.wait(|s| s.contains("❯ alphXa"));
    tmux.key("Down");
    tmux.literal("Y");
    tmux.wait(|s| s.lines().any(|l| l.trim() == "betaY"));
    tmux.literal("\x1b\r"); // Alt+Enter fallback.
    tmux.literal("gamma");
    tmux.wait(|s| s.lines().any(|l| l.trim() == "gamma"));
    tmux.key("C-j"); // Ctrl+J fallback.
    tmux.literal("delta");
    tmux.wait(|s| s.lines().any(|l| l.trim() == "delta"));
    tmux.key("C-c"); // tmux reports the control code under modifyOtherKeys.
    tmux.wait(|s| !s.contains("alphXa"));
    tmux.key("C-c");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let raw = std::fs::read_to_string(&output).unwrap();
        if raw.contains("\x1b[<1u") && raw.contains("\x1b[>4;0m") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "keyboard enhancement was not popped"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    println!(
        "{{\"verdict\":\"pass\",\"format\":\"{}\",\"shift_enter\":{},\"alt_enter\":true,\"ctrl_j\":true,\"vertical_caret\":true,\"keyboard_stack\":true}}",
        format,
        format == "csi-u"
    );
}

#[test]
#[ignore = "requires tmux"]
fn readline_words_match_in_composer() {
    let directory = tempfile::tempdir().unwrap();
    let (codex, claude) = composer_session(directory.path());
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-readline-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "100",
        "-y",
        "40",
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.wait(|s| s.contains("Inactive 1"));
    tmux.key("Tab");
    let screen = tmux.wait(|s| s.contains("Composer ❯"));
    let row = screen
        .lines()
        .position(|line| line.contains("Composer ❯"))
        .unwrap();
    tmux.literal(&format!("\x1b[<0;15;{}M\x1b[<0;15;{}m", row + 1, row + 1));
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/readline-words.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let key = case["key"].as_str().unwrap();
        tmux.literal(input);
        tmux.wait(|s| s.contains(&format!("❯ {input}")));
        if matches!(key, "Alt+F" | "Alt+D") {
            tmux.key("Home");
        }
        match key {
            "Alt+B" => tmux.literal("\x1bb"),
            "Alt+F" => tmux.literal("\x1bf"),
            "Alt+D" => tmux.literal("\x1bd"),
            "Alt+Backspace" => tmux.literal("\x1b\x7f"),
            "Ctrl+W" => tmux.key("C-w"),
            _ => panic!("unknown readline key: {key}"),
        }
        // Insert a visible caret marker to verify the actual terminal input path.
        tmux.literal("|");
        let mut expected = case["result"].as_str().unwrap().to_owned();
        let point = case["point"].as_u64().unwrap() as usize;
        let byte = expected.chars().take(point).map(char::len_utf8).sum();
        expected.insert(byte, '|');
        tmux.wait(|s| {
            s.lines()
                .any(|line| line.trim().ends_with(&format!("❯ {expected}")))
        });
        tmux.key("End");
        tmux.key("C-u");
        tmux.wait(|s| s.lines().any(|line| line.trim().ends_with("❯")));
    }
}
