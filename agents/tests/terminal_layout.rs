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
    for (width, height) in [
        (200, 50),
        (120, 60),
        (100, 45),
        (149, 50),
        (150, 50),
        (151, 50),
        (200, 50),
    ] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ]);
        std::thread::sleep(Duration::from_secs(1));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        println!("{width} columns:\n{capture}");
        let lines: Vec<_> = capture.lines().collect();
        assert!(
            capture.contains("Message"),
            "message header missing: {capture}"
        );
        assert!(
            !capture.contains("Latest activity"),
            "stale activity header: {capture}"
        );
        assert!(
            !capture.contains("Latest message"),
            "stale message header: {capture}"
        );
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
        let top_end = 3 + lines[3..]
            .iter()
            .take_while(|line| line.matches('│').count() == separators.len())
            .count();
        let prompt = top_end + 1;
        assert!(lines[prompt].contains('›'), "prompt missing: {capture}");
        let log_rule = if width >= 150 {
            top_end + 2
        } else {
            (prompt + 1..lines.len() - 1)
                .find(|&row| lines[row].matches('─').count() > width as usize / 2)
                .expect("separate list/detail border")
        };
        let top_height = top_end - 3;
        let bottom_height = lines.len() - log_rule - 2;
        if width >= 150 {
            assert!(
                top_height == bottom_height || top_height == bottom_height + 1,
                "content heights differ: top {top_height}, bottom {bottom_height}: {capture}"
            );
        } else {
            let list_height = log_rule - prompt - 1;
            let content = height as usize - 7;
            let expected = [content.div_ceil(3), (content + 1) / 3, content / 3];
            assert_eq!(
                [top_height, list_height, bottom_height],
                expected,
                "{capture}"
            );
            println!(
                "{width}x{height} content heights: top={top_height}, list={list_height}, detail={bottom_height}"
            );
        }
        assert!(
            !lines[log_rule + 1].trim().is_empty(),
            "blank row below log rule: {capture}"
        );
        let model = lines
            .iter()
            .find(|line| line.contains("Model"))
            .expect("Model header");
        assert!(
            model.contains("claude-opus-5-5 · medium"),
            "model group missing: {capture}"
        );
        assert!(
            lines.iter().any(|line| {
                line.contains('─') && line.contains("Codex") && line.contains(" · Inactive")
            }),
            "title must precede status on the middle rule: {capture}"
        );
        if width >= 150 {
            let column_width = (width as usize - 8) / 4;
            assert_eq!(separators, [column_width + 2, 2 * column_width + 5]);
            for line in &lines[3..top_end] {
                let log = line.split('│').next_back().unwrap();
                if !log.trim().is_empty() {
                    assert_eq!(log.len() - log.trim_start().len(), 1, "log left: {line}");
                    assert_eq!(
                        line.trim_end().width(),
                        width as usize - 1,
                        "log time right: {line}"
                    );
                }
            }
            assert!(
                lines[top_end + 1].contains('›'),
                "prompt missing: {capture}"
            );
            let middle: Vec<_> = lines[top_end].chars().collect();
            let details: String = middle[separators[0] + 2..separators[1] - 1]
                .iter()
                .collect();
            assert!(
                details.contains(" · Inactive"),
                "status missing from middle rule: {capture}"
            );
            for line in &lines[3..top_end] {
                let log = line.split('│').next_back().unwrap();
                assert!(!log.contains("────"), "extra log rule: {capture}");
                assert!(!line.contains(" · Inactive"), "extra status row: {capture}");
            }
            assert!(lines[log_rule].trim().chars().all(|c| c == '─'));
            assert!(
                lines[log_rule + 4..lines.len() - 1]
                    .iter()
                    .any(|line| line.width() > width as usize * 2 / 3),
                "detail does not span the terminal: {capture}"
            );
            assert!(
                lines[3..top_end].iter().any(|line| {
                    line.split('│')
                        .next_back()
                        .is_some_and(|log| log.contains("Stop hook"))
                }),
                "event list missing from top row: {capture}"
            );
        } else {
            let list = lines[prompt + 1..log_rule].join("\n");
            assert!(
                list.contains("Stop hook"),
                "event list missing below prompt: {capture}"
            );
            assert!(
                lines[log_rule + 1..lines.len() - 1]
                    .join("\n")
                    .contains("decision"),
                "newest event detail missing below list: {capture}"
            );
            let user_row = (prompt + 1..log_rule)
                .find(|&row| lines[row].contains("⏺"))
                .expect("real user event row");
            // Feed the same SGR mouse press/release received by the terminal loop.
            tmux.run(&[
                "send-keys",
                "-t",
                "probe",
                "-l",
                &format!("\x1b[<0;2;{}M\x1b[<0;2;{}m", user_row + 1, user_row + 1),
            ]);
            std::thread::sleep(Duration::from_secs(1));
            let opened = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            let opened_lines: Vec<_> = opened.lines().collect();
            assert_eq!(
                opened_lines[prompt + 1..log_rule].join("\n"),
                list,
                "event list hidden after click: {opened}"
            );
            assert!(
                opened_lines[log_rule + 1..opened_lines.len() - 1]
                    .join("\n")
                    .contains("run_in_background"),
                "clicked user event missing from detail: {opened}"
            );
            tmux.run(&["send-keys", "-t", "probe", "C-r"]);
            tmux.run(&["send-keys", "-t", "probe", "-l", "await"]);
            std::thread::sleep(Duration::from_millis(200));
            let searched = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            assert!(
                searched
                    .lines()
                    .skip(log_rule + 1)
                    .any(|line| line.contains("Search › await")),
                "detail search missing: {searched}"
            );
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
            // Select the real newest event so each resize starts with the same detail.
            std::thread::sleep(Duration::from_millis(200));
            let assistant_row = prompt + 1;
            tmux.run(&[
                "send-keys",
                "-t",
                "probe",
                "-l",
                &format!(
                    "\x1b[<0;2;{}M\x1b[<0;2;{}m",
                    assistant_row + 1,
                    assistant_row + 1
                ),
            ]);
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}
