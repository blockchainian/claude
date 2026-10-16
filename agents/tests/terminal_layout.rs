use std::path::Path;
use std::process::Command;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

struct Tmux(String);

impl Tmux {
    fn run(&self, args: &[&str]) -> String {
        let output = Command::new("tmux")
            .args(["-L", &self.0])
            .args(args)
            .output()
            .expect("tmux must be installed for the terminal probe");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
    }
}

// Runs the executable's terminal loop, provider discovery and log loading, rather
// than constructing a View. AGENTS_PROBE_BIN also checks the installed artifact.
#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_layout_tracks_width() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        include_str!("fixtures/transcript.jsonl"),
    )
    .unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux(format!("agents-layout-{}", std::process::id()));
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
        "50",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", config.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    // Let provider discovery finish, then select the real archived transcript.
    std::thread::sleep(Duration::from_secs(2));
    tmux.run(&["send-keys", "-t", "probe", "Tab", "Tab", "Tab", "Tab"]);
    for width in [200, 149, 150, 200] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        std::thread::sleep(Duration::from_secs(1));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        println!("{width} columns:\n{capture}");
        let lines: Vec<_> = capture.lines().collect();
        assert!(
            capture.contains("4165651d") || capture.contains("Stop hook"),
            "fixture not selected: {capture}"
        );
        assert!(
            lines[2].trim().chars().all(|c| c == '─'),
            "top rule is not clean: {capture}"
        );
        let top = lines[3];
        assert_eq!(
            top.matches('│').count(),
            if width >= 150 { 2 } else { 1 },
            "{capture}"
        );
        let separators: Vec<_> = top
            .match_indices('│')
            .map(|(index, _)| top[..index].width())
            .collect();
        if width >= 150 {
            assert_eq!(separators[1], width as usize - width as usize / 3 - 4);
            assert!(lines[30].contains('›'), "prompt missing: {capture}");
            let middle: Vec<_> = lines[29].chars().collect();
            let details: String = middle[separators[0] + 2..separators[1] - 1]
                .iter()
                .collect();
            assert!(
                details.contains("Inactive ·"),
                "status missing from middle rule: {capture}"
            );
            for line in &lines[3..29] {
                let log = line.split('│').next_back().unwrap();
                assert!(!log.contains("────"), "extra log rule: {capture}");
                assert!(!line.contains("Inactive ·"), "extra status row: {capture}");
            }
            assert!(lines[31].trim().chars().all(|c| c == '─'));
            assert!(
                lines[35..48]
                    .iter()
                    .any(|line| line.width() > width as usize * 2 / 3),
                "detail does not span the terminal: {capture}"
            );
            assert!(
                lines[3..29].iter().any(|line| {
                    line.split('│').next_back().is_some_and(|log| {
                        log.contains("Tool") || log.contains("User") || log.contains("Assistant")
                    })
                }),
                "event list missing from top row: {capture}"
            );
        } else {
            assert!(
                lines[33..48].iter().any(|line| line.contains("Tool")
                    || line.contains("User")
                    || line.contains("Assistant")),
                "event list missing below prompt: {capture}"
            );
        }
    }
}
