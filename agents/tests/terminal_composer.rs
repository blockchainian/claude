use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

struct Tmux(String);
impl Tmux {
    fn run(&self, args: &[&str]) -> String {
        let output = Command::new("tmux")
            .env_remove("NO_COLOR")
            .args(["-L", &self.0])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
    fn wait(&self, ready: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(30);
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
        self.run(&["send-keys", "-t", "probe", "-l", text]);
    }
    fn key(&self, key: &str) {
        self.run(&["send-keys", "-t", "probe", key]);
    }
}
impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
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

fn probe(format: &str) {
    let directory = tempfile::tempdir().unwrap();
    let codex = directory.path().join("codex");
    let claude = directory.path().join("claude");
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
        rusqlite::params![
            transcript.to_str().unwrap(),
            directory.path().to_str().unwrap()
        ],
    )
    .unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let output = directory.path().join("terminal.raw");
    let tmux = Tmux(format!("agents-composer-{format}-{}", std::process::id()));
    tmux.run(&[
        "-f",
        "/dev/null",
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
