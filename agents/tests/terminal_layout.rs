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

    fn mouse(&self, button: u16, column: usize, row: usize, click: bool) {
        let press = format!("\x1b[<{button};{};{}M", column + 1, row + 1);
        let release = if click {
            format!("\x1b[<{button};{};{}m", column + 1, row + 1)
        } else {
            String::new()
        };
        self.run(&[
            "send-keys",
            "-t",
            "probe",
            "-l",
            &format!("{press}{release}"),
        ]);
        std::thread::sleep(Duration::from_millis(200));
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output();
    }
}

fn start_probe(name: &str) -> (tempfile::TempDir, Tmux, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&transcript, include_str!("fixtures/transcript.jsonl")).unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux(format!("agents-{name}-{}", std::process::id()));
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
    (directory, tmux, transcript)
}

// Runs the executable's terminal loop, provider discovery and log loading, rather
// than constructing a View. AGENTS_PROBE_BIN also checks the installed artifact.
#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_layout_tracks_width() {
    let (_directory, tmux, _transcript) = start_probe("layout");
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
            capture.contains("Latest update"),
            "latest update header missing: {capture}"
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

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_log_selection_tracks_wheel_click_and_tail() {
    use std::io::Write;

    for width in [120, 180] {
        let (_directory, tmux, transcript) = start_probe("log-selection");
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "45",
        ]);
        std::thread::sleep(Duration::from_millis(200));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = capture.lines().collect();
        let prompt = rows
            .iter()
            .rposition(|row| row.contains('›'))
            .expect("prompt");
        let (column, row) = if width >= 150 {
            let separator = rows[3].rfind('│').expect("log column separator");
            (rows[3][..separator].width() + 2, 3)
        } else {
            (1, prompt + 1)
        };
        let detail = |capture: &str| {
            let rows: Vec<_> = capture.lines().collect();
            let rule = rows
                .iter()
                .rposition(|line| line.matches('─').count() > width / 2)
                .expect("log detail rule");
            rows[rule + 1..rows.len() - 1].join("\n")
        };
        assert!(detail(&capture).contains("decision"), "{capture}");
        tmux.mouse(65, column, row, false);
        let older = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(detail(&older).contains("run_in_background"), "{older}");
        assert!(older.contains("↑ Latest"), "{older}");
        tmux.mouse(64, column, row, false);
        let newest = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(detail(&newest).contains("decision"), "{newest}");
        assert!(!newest.contains("↑ Latest"), "{newest}");
        tmux.mouse(0, column, row + 1, true);
        let selected = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            detail(&selected).contains("run_in_background"),
            "{selected}"
        );
        let mut arrival: serde_json::Value = serde_json::from_str(
            include_str!("fixtures/transcript.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        arrival["uuid"] = "terminal-tail-arrival".into();
        arrival["timestamp"] = "2026-10-06T16:41:00Z".into();
        arrival["message"]["content"] = "terminal-tail-proof".into();
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .unwrap(),
            "{arrival}"
        )
        .unwrap();
        std::thread::sleep(Duration::from_secs(1));
        let grown = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(detail(&grown).contains("run_in_background"), "{grown}");
        let (jump_row, jump_line) = grown
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("↑ 1 New"))
            .expect("new event jump");
        let jump_column = jump_line[..jump_line.find("↑ 1 New").unwrap()].width();
        tmux.mouse(0, jump_column, jump_row, true);
        let following = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            detail(&following).contains("terminal-tail-proof"),
            "{following}"
        );
        assert!(!following.contains("↑ 1 New"), "{following}");
        println!("{width} columns: wheel, click, pinned arrival and tail jump passed");
    }
}
