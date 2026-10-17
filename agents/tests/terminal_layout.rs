use std::path::Path;
use std::process::Command;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

#[path = "support/terminal_questions.rs"]
mod terminal_questions;

#[path = "support/terminal_headings.rs"]
mod terminal_headings;

struct Tmux(String);

impl Tmux {
    fn run(&self, args: &[&str]) -> String {
        // These probes assert ANSI colors and intensity in a controlled terminal.
        let output = Command::new("tmux")
            .env_remove("NO_COLOR")
            .args(["-L", &self.0])
            .args(args)
            .output()
            .expect("tmux must be installed for the terminal probe");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn wait_for(&self, ready: impl Fn(&str) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let capture = self.run(&["capture-pane", "-t", "probe", "-p"]);
            if ready(&capture) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "terminal did not become ready: {capture}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
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

// tmux carries SGR state across capture rows. Position each row explicitly so
// terminal-width wrapping cannot shift later rows, while preserving that state.
fn captured_screen(capture: &str, width: u16, height: u16) -> vt100::Parser {
    let mut parser = vt100::Parser::new(height, width, 0);
    for (row, text) in capture.lines().enumerate() {
        parser.process(format!("\x1b[{};1H{text}", row + 1).as_bytes());
    }
    parser
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
    let tmux_config = directory.path().join("tmux.conf");
    std::fs::write(&tmux_config, "set -g default-terminal tmux-256color\n").unwrap();
    let tmux = Tmux(format!("agents-{name}-{}", std::process::id()));
    tmux.run(&[
        "-f",
        tmux_config.to_str().unwrap(),
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
    // Wait for provider discovery before sending input, including on busy hosts.
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|tabs| tabs.contains("Inactive 1"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    tmux.wait_for(|capture| capture.contains("Stop hook") && capture.contains("Update"));
    (directory, tmux, transcript)
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_mcp_display_names() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("mcp-names");
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "45"]);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    // Keep the two parallel calls and their recorded result from the real transcript.
    for line in include_str!("fixtures/claude-tool-calls.jsonl")
        .lines()
        .skip(16)
        .take(3)
    {
        writeln!(file, "{line}").unwrap();
    }
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    tmux.wait_for(|capture| capture.contains("codex-manager start"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let path = std::env::temp_dir().join("agents-mcp-names-200x45.txt");
    std::fs::write(&path, &capture).unwrap();
    assert_eq!(capture.lines().count(), 45);
    assert!(
        !capture.contains("mcp__plugin_codex_codex-manager__start"),
        "{capture}"
    );
    assert!(
        capture.matches("codex-manager start").count() >= 3,
        "{capture}"
    );
    assert!(
        capture.contains("codex-manager start · Implement workstream A"),
        "{capture}"
    );
    println!(
        "{{\"verdict\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_dashboard_tabs_and_grouping_hint() {
    let (_directory, tmux, _transcript) = start_probe("header-footer");
    for width in [200, 120] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "45",
        ]);
        tmux.wait_for(|capture| {
            // The clipped pre-resize frame can already have the expected height and footer.
            capture.lines().count() == 45
                && capture.contains("g group by project")
                && capture
                    .lines()
                    .next()
                    .is_some_and(|row| row.contains("Inactive 1"))
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = capture.lines().collect();
        assert!(rows[0].contains("Active 0"), "{capture}");
        assert!(rows[0].contains("Inactive 1"), "{capture}");
        assert!(rows[1].contains("──"), "{capture}");
        assert!(!capture.contains("Agent command center"), "{capture}");
        assert!(rows[44].contains("g group by project"), "{capture}");
        let path = std::env::temp_dir().join(format!("agents-dashboard-{width}x45.txt"));
        std::fs::write(&path, &capture).unwrap();
        println!(
            "{{\"dashboard_header_footer\":\"pass\",\"width\":{width},\"capture\":\"{}\"}}",
            path.display()
        );
        for grouping in ["status", "model", "project"] {
            tmux.run(&["send-keys", "-t", "probe", "g"]);
            tmux.wait_for(|capture| {
                capture
                    .lines()
                    .last()
                    .is_some_and(|footer| footer.contains(&format!("g group by {grouping}")))
            });
        }
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_cursor_keeps_cell_colors() {
    let (_directory, tmux, _transcript) = start_probe("task-cursor");
    for width in [120, 200] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        std::thread::sleep(Duration::from_millis(300));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let parser = captured_screen(&capture, width, 50);
        let screen = parser.screen();
        let tabs = screen.rows(0, width).next().unwrap();
        assert_eq!(
            tabs.split_whitespace().collect::<Vec<_>>(),
            ["Active", "0", "Inactive", "1"]
        );
        assert_eq!(tabs.find("Active"), Some(2));
        assert_eq!(
            tabs[..tabs.find("Inactive").unwrap()].width(),
            usize::from(width) - 12
        );
        for column in [2, width - 12] {
            let tab = screen.cell(0, column).unwrap();
            assert!(tab.bold(), "{capture}");
            assert!(!tab.dim(), "{capture}");
            assert_ne!(tab.fgcolor(), vt100::Color::Default, "{capture}");
        }
        assert_eq!(screen.cell(0, 2).unwrap().bgcolor(), vt100::Color::Default);
        assert_ne!(
            screen.cell(0, width - 12).unwrap().bgcolor(),
            vt100::Color::Default
        );
        let (x, y) = (0..50)
            .find_map(|y| {
                (0..width / 3).find_map(|x| {
                    (screen.cell(y, x).unwrap().contents() == "○"
                        && x > 0
                        && screen.cell(y, x - 1).unwrap().bold())
                    .then_some((x, y))
                })
            })
            .expect("selected task marker");
        assert!(
            !(0..width / 3).any(|column| screen.cell(y, column).unwrap().contents() == "›"),
            "{capture}"
        );
        let marker = screen.cell(y, x).unwrap();
        let status = screen.cell(y, x).unwrap();
        let title = screen.cell(y, x + 2).unwrap();
        assert!(screen.cell(y, x - 1).unwrap().bold());
        // vt100 models bold and dim as mutually exclusive intensity levels.
        assert!(status.dim());
        assert!(title.bold(), "title at ({}, {y}): {capture}", x + 2);
        assert_eq!(marker.fgcolor(), vt100::Color::Default);
        assert_ne!(title.fgcolor(), marker.fgcolor());
        assert_eq!(status.fgcolor(), vt100::Color::Default);
        assert_eq!(title.bgcolor(), marker.bgcolor());
        assert_eq!(status.bgcolor(), marker.bgcolor());
        assert_ne!(marker.bgcolor(), vt100::Color::Default);
    }
    println!("{{\"task_cursor\":\"pass\",\"widths\":[120,200]}}");
}

fn assert_details_borders(tmux: &Tmux, width: u16, height: u16, capture: &str) {
    let lines: Vec<_> = capture.lines().collect();
    let separators: Vec<_> = lines[2]
        .match_indices('│')
        .map(|(index, _)| lines[2][..index].width())
        .collect();
    let top_end = 2 + lines[2..]
        .iter()
        .take_while(|line| line.matches('│').count() == separators.len())
        .count();
    let details_start = separators[0];
    let details_end = if width >= 150 {
        separators[1] + 1
    } else {
        width as usize
    };
    let middle: Vec<_> = lines[top_end].chars().collect();
    let details: String = middle[details_start..details_end].iter().collect();
    let status = " Inactive ";
    let start = (details_end - details_start - status.len()) / 2;
    assert_eq!(
        details
            .chars()
            .skip(start)
            .take(status.len())
            .collect::<String>(),
        status,
        "{capture}"
    );
    assert!(details.starts_with('┴'), "{capture}");
    assert!(
        details.ends_with(if width >= 150 {
            " claude╶┴"
        } else {
            " claude╶─"
        }),
        "{capture}"
    );
    assert!(
        details.chars().skip(1).take(start - 1).all(|c| c == '─'),
        "{capture}"
    );
    let folder_start = details.chars().count() - " claude╶─".chars().count();
    assert!(
        details
            .chars()
            .skip(start + status.len())
            .take(folder_start - start - status.len())
            .all(|c| c == '─'),
        "{capture}"
    );
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, width, height);
    let cell = parser
        .screen()
        .cell(top_end as u16, (details_start + start + 1) as u16)
        .unwrap();
    assert!(cell.bold(), "{colored}");
    assert!(!cell.dim(), "{colored}");
    assert_eq!(cell.fgcolor(), vt100::Color::Default);

    let top: String = (details_start..details_end)
        .map(|x| parser.screen().cell(1, x as u16).unwrap().contents())
        .collect();
    let elapsed = top
        .rsplit('─')
        .map(|part| part.trim_matches('┬'))
        .find(|part| !part.trim_matches([' ', '┬']).is_empty())
        .unwrap();
    assert!(
        elapsed.starts_with(' ') && elapsed.ends_with('╶'),
        "{capture}"
    );
    assert!(!elapsed.trim().is_empty(), "{capture}");
    let elapsed_start = details_end - 1 - elapsed.width();
    for x in elapsed_start + 1..details_end - 2 {
        let cell = parser.screen().cell(1, x as u16).unwrap();
        assert!(cell.dim(), "{colored}");
        assert!(!cell.bold(), "{colored}");
    }
    let title = top
        .split('─')
        .find(|part| !part.trim_matches([' ', '┬']).is_empty())
        .unwrap();
    assert!(title.starts_with(' ') && title.ends_with(' '), "{capture}");
    let start = (details_end - details_start - title.width()) / 2;
    assert_eq!(top[..top.find(title).unwrap()].width(), start, "{capture}");
    for x in details_start + start + 1..details_start + start + title.width() - 1 {
        let cell = parser.screen().cell(1, x as u16).unwrap();
        if !cell.contents().is_empty() {
            assert!(cell.bold(), "{colored}");
            assert!(!cell.dim(), "{colored}");
        }
    }
    println!("{{\"details_borders\":\"pass\",\"width\":{width}}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_details_borders_keep_status_and_elapsed_separate() {
    let (_directory, tmux, _transcript) = start_probe("details-borders");
    for width in [112, 200] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        std::thread::sleep(Duration::from_millis(300));
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        println!("{width} columns:\n{capture}");
        assert_details_borders(&tmux, width, 50, &capture);
    }
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
            capture.contains("Update"),
            "update header missing: {capture}"
        );
        assert!(
            !capture.contains("Latest update"),
            "stale update header: {capture}"
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
            !lines[1].contains("Inactive"),
            "status must stay on the bottom rule: {capture}"
        );
        let top = lines[2];
        assert_eq!(
            top.matches('│').count(),
            if width >= 150 { 2 } else { 1 },
            "{capture}"
        );
        let separators: Vec<_> = top
            .match_indices('│')
            .map(|(index, _)| top[..index].width())
            .collect();
        let top_end = 2 + lines[2..]
            .iter()
            .take_while(|line| line.matches('│').count() == separators.len())
            .count();
        let prompt = top_end + 1;
        assert!(lines[prompt].contains('›'), "prompt missing: {capture}");
        let log_rule = if width >= 150 {
            top_end + 2
        } else {
            (prompt + 2..lines.len() - 1)
                .find(|&row| lines[row].matches('─').count() > width as usize / 2)
                .expect("separate list/detail border")
        };
        let top_height = top_end - 2;
        let bottom_height = lines.len() - log_rule - 2;
        if width >= 150 {
            assert!(
                top_height == bottom_height || top_height == bottom_height + 1,
                "content heights differ: top {top_height}, bottom {bottom_height}: {capture}"
            );
        } else {
            let list_height = log_rule - prompt - 2;
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
            .find(|line| line.contains("claude-opus-5-5"))
            .expect("model row");
        assert!(
            model.contains("claude-opus-5-5") && model.contains("medium"),
            "model group missing: {capture}"
        );
        assert_details_borders(&tmux, width, height, &capture);
        if width >= 150 {
            let column_width = (width as usize - 8) / 4;
            assert_eq!(separators, [column_width + 2, 2 * column_width + 5]);
            for line in &lines[2..top_end] {
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
                details.contains(" Inactive "),
                "status missing from middle rule: {capture}"
            );
            for line in &lines[2..top_end] {
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
                lines[2..top_end].iter().any(|line| {
                    line.split('│')
                        .next_back()
                        .is_some_and(|log| log.contains("Stop hook"))
                }),
                "event list missing from top row: {capture}"
            );
        } else {
            let list = lines[prompt + 2..log_rule].join("\n");
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
                opened_lines[prompt + 2..log_rule].join("\n"),
                list,
                "event list hidden after click: {opened}"
            );
            assert!(opened_lines[prompt + 1].contains("↑ Latest"), "{opened}");
            assert!(
                opened_lines[log_rule + 1..opened_lines.len() - 1]
                    .join("\n")
                    .contains("run_in_background"),
                "clicked user event missing from detail: {opened}"
            );
            // Search follows the last mouse panel, independently of the selected event.
            tmux.mouse(0, 1, log_rule + 1, true);
            tmux.run(&["send-keys", "-t", "probe", "/"]);
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
            let assistant_row = prompt + 2;
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
fn terminal_compaction_summary_keeps_row_and_labels_detail() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("compaction");
    std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap()
        .write_all(include_bytes!("fixtures/claude-compaction.jsonl"))
        .unwrap();
    std::thread::sleep(Duration::from_secs(1));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let rows: Vec<_> = capture.lines().collect();
    let rule = rows
        .iter()
        .rposition(|line| line.matches('─').count() > 100)
        .expect("log detail rule");
    let list = rows[2..rule].join("\n");
    let detail = rows[rule + 1..rows.len() - 1].join("\n");
    assert!(
        list.contains("This session is being continued"),
        "{capture}"
    );
    assert!(detail.contains("Compaction"), "{capture}");
    assert!(
        detail.contains("This session is being continued"),
        "{capture}"
    );
    assert!(!detail.contains("User"), "{capture}");
    println!("{{\"compaction_list\":\"pass\",\"compaction_detail\":\"pass\"}}");
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
            let separator = rows[2].rfind('│').expect("log column separator");
            (rows[2][..separator].width() + 2, 2)
        } else {
            (1, prompt + 2)
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

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_update_click_selects_source_row() {
    let (_directory, tmux, _transcript) = start_probe("details-source");
    std::thread::sleep(Duration::from_secs(1));
    // Select the older user row before clicking the Update body.
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let lines: Vec<_> = capture.lines().collect();
    let (update_y, update_x) = lines
        .iter()
        .enumerate()
        .find_map(|(y, line)| line.find("Update").map(|byte| (y, line[..byte].width())))
        .expect("Update section");
    let (user_y, user_x) = lines
        .iter()
        .enumerate()
        .find_map(|(y, line)| {
            line.rfind("Stop hook").and_then(|byte| {
                let x = line[..byte].width();
                (x > update_x && y < 20).then_some((y, x))
            })
        })
        .expect("user log row");
    tmux.mouse(0, user_x, user_y, true);
    let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(
        before.lines().skip(22).any(|line| line.contains("User")),
        "{before}"
    );
    let body_x = lines[update_y + 1]
        .find('│')
        .map(|byte| lines[update_y + 1][..byte].width())
        .unwrap()
        + 2;
    tmux.mouse(0, body_x, update_y + 1, true);
    std::thread::sleep(Duration::from_secs(1));
    let after = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(
        after
            .lines()
            .skip(22)
            .any(|line| line.contains("Assistant")),
        "{after}"
    );
    assert!(
        after.lines().skip(22).any(|line| line.contains("Stop")),
        "{after}"
    );
    println!("Update click selected the real transcript's assistant row in tmux.");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_notification_shows_summary_id_and_missing_output_path() {
    use std::io::Write;
    let (_directory, tmux, transcript) = start_probe("task-notification");
    let record = include_str!("fixtures/claude-task-notifications.jsonl")
        .lines()
        .next()
        .unwrap();
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(transcript)
            .unwrap(),
        "{record}"
    )
    .unwrap();
    let summary = "Background command \"等 tab 线程\" completed (exit code 0)";
    tmux.wait_for(|capture| capture.contains(summary));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    println!("{capture}");
    let rows: Vec<_> = capture.lines().collect();
    let rule = rows
        .iter()
        .rposition(|line| line.matches('─').count() > 100)
        .expect("log detail rule");
    let detail = rows[rule + 1..rows.len() - 1].join("\n");
    assert_eq!(detail.matches(summary).count(), 1, "{capture}");
    assert!(rows[rule + 1].starts_with(" Task b9wi31ww0 "), "{capture}");
    assert!(
        detail.contains("output-file: /tmp/agents-notification/tasks/b9wi31ww0.output"),
        "{capture}"
    );
    assert!(!detail.contains("task-id:"), "{capture}");
    assert!(!detail.contains("status: completed"), "{capture}");
    assert!(!detail.contains("tool-use-id"), "{capture}");
    println!("{{\"task_notification\":\"pass\",\"summary_once\":true,\"output_path\":true}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_json_strings_keep_multiline_alignment_and_colors() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("json-strings");
    // This await output was supplied from a real Codex manager completion.
    let output = include_str!("fixtures/codex-manager-await.jsonl").trim_end();
    let completed: serde_json::Value = serde_json::from_str(output).unwrap();
    let message = completed["lastMessage"].as_str().unwrap();
    let paragraphs: Vec<_> = message.split('\n').collect();
    assert_eq!(paragraphs.len(), 4);
    let tool = serde_json::json!({
        "type": "assistant", "uuid": "json-await-call",
        "timestamp": "2026-10-16T21:12:48Z",
        "message": {"id": "json-await-call", "role": "assistant", "content": [{
            "type": "tool_use", "id": "json-await", "name": "Bash",
            "input": {"command": "codex-manager await", "description": "Read completed turn"}
        }]}
    });
    let result = serde_json::json!({
        "type": "user", "uuid": "json-await-result",
        "timestamp": "2026-10-16T21:12:49Z",
        "message": {"role": "user", "content": [{
            "type": "tool_result", "tool_use_id": "json-await", "content": output
        }]}
    });
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    writeln!(file, "{tool}\n{result}").unwrap();
    // Wake the terminal loop after the fixture arrives, including on hosts whose
    // filesystem watcher does not notify for the temporary transcript directory.
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    std::thread::sleep(Duration::from_secs(1));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    println!("{capture}");
    let rows: Vec<_> = capture.lines().collect();
    let (first_row, first_column) = rows
        .iter()
        .enumerate()
        .find_map(|(row, text)| {
            text.find(paragraphs[0])
                .map(|byte| (row, text[..byte].width()))
        })
        .expect("await lastMessage first line");
    let key = "\"lastMessage\": \"";
    let key_byte = rows[first_row].find(key).expect("lastMessage JSON key");
    assert_eq!(
        rows[first_row][..key_byte].width() + key.width(),
        first_column,
        "string must start immediately after its opening quote: {capture}"
    );
    assert!(rows[first_row + 1].trim().is_empty(), "{capture}");
    let second_byte = rows[first_row + 2]
        .find(paragraphs[2])
        .expect("await lastMessage continuation line");
    assert_eq!(
        rows[first_row + 2][..second_byte].width(),
        first_column,
        "continuation must align under the first string character: {capture}"
    );
    assert_eq!(
        rows[first_row + 3].trim().trim_end_matches(','),
        "\"",
        "{capture}"
    );
    assert_eq!(
        rows[first_row + 3].find('"').unwrap(),
        first_column,
        "the trailing newline must precede the closing quote: {capture}"
    );
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    let screen = parser.screen();
    let string = screen.cell(first_row as u16, first_column as u16).unwrap();
    let continuation = screen
        .cell((first_row + 2) as u16, first_column as u16)
        .unwrap();
    let key = screen
        .cell(
            first_row as u16,
            rows[first_row][..key_byte].width() as u16 + 1,
        )
        .unwrap();
    assert_ne!(key.fgcolor(), vt100::Color::Default);
    assert_ne!(string.fgcolor(), vt100::Color::Default);
    assert_ne!(key.fgcolor(), string.fgcolor());
    assert_eq!(string.fgcolor(), continuation.fgcolor());
    println!("{{\"json_multiline\":\"pass\",\"alignment\":\"pass\",\"colors\":\"pass\"}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_running_tool_animates_without_input() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("running-tool");
    // Retain the upstream tool-call envelope while running this probe's actual command.
    let mut call: serde_json::Value = serde_json::from_str(
        include_str!("fixtures/claude-tool-details.jsonl")
            .lines()
            .nth(7)
            .unwrap(),
    )
    .unwrap();
    call["isSidechain"] = false.into();
    call["timestamp"] = chrono::Utc::now().to_rfc3339().into();
    call["message"]["content"][0]["input"] = serde_json::json!({"command": "sleep 5"});
    let mut tool = Command::new("sleep").arg("5").spawn().unwrap();
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(transcript)
            .unwrap(),
        "{call}"
    )
    .unwrap();
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    std::thread::sleep(Duration::from_secs(1));
    let frames = ["•··", "·•·", "··•"];
    let mut seen = std::collections::BTreeSet::new();
    let mut position = None;
    for _ in 0..10 {
        assert!(
            tool.try_wait().unwrap().is_none(),
            "tool must still be running"
        );
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let (y, raw) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| frames.iter().any(|frame| line.contains(frame)))
            .unwrap_or_else(|| panic!("running output animation is visible: {capture}"));
        let parser = captured_screen(&capture, 200, 50);
        let screen = parser.screen();
        let contents = screen.rows(0, 200).nth(y).unwrap();
        let (x, frame) = frames
            .iter()
            .find_map(|frame| {
                contents
                    .find(frame)
                    .map(|byte| (contents[..byte].width(), *frame))
            })
            .unwrap();
        if let Some(previous) = position {
            assert_eq!((x, y), previous, "animation must stay centered");
        }
        position = Some((x, y));
        assert_eq!(x, 1 + (198 - 3) / 2);
        for offset in 0..3 {
            let cell = screen.cell(y as u16, (x + offset) as u16).unwrap();
            assert!(cell.dim(), "running output stays dim: x={x}, raw={raw:?}");
            assert!(!cell.is_wide(), "each dot occupies one terminal cell");
            assert_eq!(
                cell.contents(),
                frame.chars().nth(offset).unwrap().to_string()
            );
        }
        seen.insert(frame);
        println!("frame: {contents}");
        // No key, resize or transcript write occurs during frame sampling.
        std::thread::sleep(Duration::from_millis(130));
    }
    assert_eq!(seen.len(), 3);
    assert!(tool.wait().unwrap().success());
    println!(
        "{{\"running_animation\":\"pass\",\"frames\":3,\"single_width\":true,\"dim\":true,\"centered\":true}}"
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_details_section_positions() {
    let (_directory, tmux, transcript) = start_probe("details-sections");
    let original = format!(
        "{}\n{}",
        std::fs::read_to_string(&transcript).unwrap(),
        include_str!("fixtures/claude-todos.jsonl")
    );
    for has_pr in [false, true, false] {
        let content = if has_pr {
            format!("{original}\n{}", include_str!("fixtures/pr-link.jsonl"))
        } else {
            original.clone()
        };
        std::fs::write(&transcript, content).unwrap();
        tmux.wait_for(|capture| capture.contains("#763") == has_pr && capture.contains("Task 1"));
        for width in [112, 200] {
            tmux.run(&[
                "resize-window",
                "-t",
                "probe",
                "-x",
                &width.to_string(),
                "-y",
                "70",
            ]);
            std::thread::sleep(Duration::from_millis(500));
            let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            println!("{width} columns, PR {has_pr}:\n{capture}");
            let lines: Vec<_> = capture.lines().collect();
            let separators: Vec<_> = lines[2]
                .match_indices('│')
                .map(|(byte, _)| lines[2][..byte].width())
                .collect();
            let left = separators[0];
            let right = if width >= 150 {
                separators[1] + 1
            } else {
                width as usize
            };
            let panel_right = right - if width >= 150 { 2 } else { 1 };
            let panel_width = panel_right - left - 2;
            let bottom = 2 + lines[2..]
                .iter()
                .take_while(|line| line.matches('│').count() == separators.len())
                .count();
            let parser = captured_screen(&capture, width, 70);
            let panel: Vec<String> = (2..bottom)
                .map(|y| {
                    (left + 2..panel_right)
                        .map(|x| parser.screen().cell(y as u16, x as u16).unwrap().contents())
                        .collect()
                })
                .collect();
            let positions = ["Update", "Prompt", "Tasks"]
                .map(|label| panel.iter().position(|row| row.contains(label)).unwrap());
            assert!(positions.is_sorted(), "{panel:?}");
            for label in ["Update", "Prompt", "Tasks"] {
                let row = panel.iter().find(|row| row.contains(label)).unwrap();
                let byte = row.find(label).unwrap();
                assert_eq!(
                    row[..byte].width(),
                    (panel_width - label.len()) / 2,
                    "{row}"
                );
                assert!(!row.contains("messages"));
            }
            assert!(
                panel
                    .iter()
                    .all(|row| row.trim() != "Project" && row.trim() != "Model")
            );
            assert!(
                panel
                    .iter()
                    .all(|row| !row.contains("claude-opus-5-5") && !row.contains("medium"))
            );
            let status: String = (left..right)
                .map(|x| {
                    parser
                        .screen()
                        .cell(bottom as u16, x as u16)
                        .unwrap()
                        .contents()
                })
                .collect();
            assert!(
                status.ends_with(if width >= 150 {
                    " claude╶┴"
                } else {
                    " claude╶─"
                }),
                "{status}"
            );
            assert!(status.contains(" Inactive "), "{status}");
            assert!(!status.contains("main"), "{status}");
            if has_pr {
                assert!(status.starts_with("┴╴#763 ─"), "{status}");
            } else {
                assert!(!status.contains('#'), "{status}");
                assert!(status.starts_with("┴────"), "{status}");
            }
            let model_y = if width >= 150 { bottom } else { bottom + 2 };
            let model_left = if width >= 150 { separators[1] } else { 0 };
            let model_right = width as usize;
            let model: String = (model_left..model_right)
                .map(|x| {
                    parser
                        .screen()
                        .cell(model_y as u16, x as u16)
                        .unwrap()
                        .contents()
                })
                .collect();
            assert!(
                model.starts_with(if width >= 150 {
                    "┴╴claude-opus-5-5 "
                } else {
                    "─╴claude-opus-5-5 "
                }),
                "{model}"
            );
            assert!(model.ends_with(" medium╶─"), "{model}");
            assert!(!model.contains("Model"));
            assert!(!model.contains('·'));
            let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
            let colored = captured_screen(&colored, width, 70);
            for (start, length) in [
                (model_left + 2, "claude-opus-5-5".len()),
                (model_right - 2 - "medium".len(), "medium".len()),
            ] {
                for x in start..start + length {
                    let cell = colored.screen().cell(model_y as u16, x as u16).unwrap();
                    assert!(cell.dim(), "{capture}");
                    assert!(!cell.bold(), "{capture}");
                }
            }
            println!(
                "{{\"details_sections\":\"pass\",\"folder_on_status_rule\":\"pass\",\"model_on_log_rule\":\"pass\",\"metadata_dim\":\"pass\",\"width\":{width},\"pr\":{has_pr}}}"
            );
        }
    }
}

#[path = "support/terminal_codex_exit.rs"]
mod terminal_codex_exit;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_log_border_double_clicks() {
    let (_directory, tmux, _transcript) = start_probe("log-borders");
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "45"]);
    tmux.wait_for(|capture| capture.lines().count() == 45 && capture.contains("对，必须"));
    let capture = |name: &str| {
        let text = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-log-border-{name}-200x45.txt"));
        std::fs::write(&path, &text).unwrap();
        println!("capture: {}", path.display());
        text
    };
    let before = capture("before");
    let rows: Vec<_> = before.lines().collect();
    let bottom = rows
        .iter()
        .position(|line| line.contains("claude-opus") || line.contains("Opus"))
        .unwrap();
    let double_click = |row: usize| {
        let sequence = format!("\x1b[<0;135;{row}M\x1b[<0;135;{row}m").repeat(2);
        tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
    };
    double_click(bottom + 1);
    tmux.wait_for(|capture| capture.contains("↑ Latest"));
    let oldest = capture("oldest");
    assert!(oldest.contains("Codex Manager 在一个 turn"), "{oldest}");
    double_click(2);
    tmux.wait_for(|capture| !capture.contains("↑ Latest") && capture.contains("对，必须"));
    let latest = capture("latest");
    assert!(latest.contains("对，必须"));
    println!("{{\"log_border_double_clicks\":\"pass\",\"width\":200,\"height\":45}}");
}

#[test]
#[ignore = "requires tmux and macOS clipboard; run with --ignored --nocapture"]
fn terminal_raw_transcript_copy_note() {
    let (_directory, tmux, _transcript) = start_probe("raw-copy");
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "45"]);
    tmux.wait_for(|capture| capture.lines().count() == 45 && capture.contains("对，必须"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (column, row) = capture
        .lines()
        .enumerate()
        .find_map(|(row, line)| {
            let byte = line.rfind("对，必须")?;
            let column = line[..byte].width();
            (row > 25).then_some((column, row))
        })
        .expect("assistant body in log detail");
    tmux.mouse(0, column, row, true);
    tmux.wait_for(|capture| capture.contains("\"apiBlockIndex\""));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (column, row) = capture
        .lines()
        .enumerate()
        .find_map(|(row, line)| {
            let byte = line.rfind('{')?;
            let column = line[..byte].width();
            (row > 25).then_some((column, row))
        })
        .expect("raw JSON in log detail");
    let sequence = format!(
        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
        column + 1,
        row + 1,
        column + 1,
        row + 1
    )
    .repeat(2);
    tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
    tmux.wait_for(|capture| capture.contains("Copied transcript"));
    let copied = Command::new("pbpaste").output().unwrap();
    assert!(copied.status.success());
    let expected: serde_json::Value = serde_json::from_str(
        include_str!("fixtures/transcript.jsonl")
            .lines()
            .nth(1)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(copied.stdout).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap()
    );
    println!(
        "{{\"raw_transcript_copy\":\"pass\",\"note\":\"Copied transcript\",\"width\":200,\"height\":45,\"pbpaste\":\"pretty JSON matches\"}}"
    );
}
