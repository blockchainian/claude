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
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    (directory, tmux, transcript)
}

#[test]
#[ignore = "requires tmux and NO_COLOR unset; run with --ignored --nocapture"]
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
        let mut parser = vt100::Parser::new(50, width, 0);
        parser.process(capture.replace('\n', "\r\n").as_bytes());
        let screen = parser.screen();
        let (x, y) = (0..50)
            .find_map(|y| {
                (0..width / 3)
                    .find_map(|x| (screen.cell(y, x).unwrap().contents() == "›").then_some((x, y)))
            })
            .expect("selected task marker");
        let marker = screen.cell(y, x).unwrap();
        let status = screen.cell(y, x + 2).unwrap();
        let title = screen.cell(y, x + 4).unwrap();
        assert!(marker.bold());
        // vt100 models bold and dim as mutually exclusive intensity levels.
        assert!(status.dim());
        assert!(title.bold());
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
    let separators: Vec<_> = lines[3]
        .match_indices('│')
        .map(|(index, _)| lines[3][..index].width())
        .collect();
    let top_end = 3 + lines[3..]
        .iter()
        .take_while(|line| line.matches('│').count() == separators.len())
        .count();
    let details_start = separators[0] + 2;
    let details_end = if width >= 150 {
        separators[1] - 1
    } else {
        width as usize - 1
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
    assert!(details.chars().take(start).all(|c| c == '─'), "{capture}");
    assert!(
        details.chars().skip(start + status.len()).all(|c| c == '─'),
        "{capture}"
    );
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let mut parser = vt100::Parser::new(height, width, 0);
    parser.process(
        colored
            .trim_end_matches('\n')
            .replace('\n', "\r\n")
            .as_bytes(),
    );
    let cell = parser
        .screen()
        .cell(top_end as u16, (details_start + start + 1) as u16)
        .unwrap();
    assert!(cell.bold(), "{colored}");
    assert!(!cell.dim(), "{colored}");
    assert_eq!(cell.fgcolor(), vt100::Color::Default);

    let top: String = (details_start..details_end)
        .map(|x| parser.screen().cell(2, x as u16).unwrap().contents())
        .collect();
    let elapsed = top.rsplit('─').next().unwrap();
    assert!(elapsed.starts_with(' '), "{capture}");
    assert!(!elapsed.trim().is_empty(), "{capture}");
    let elapsed_start = details_end - elapsed.width();
    for x in elapsed_start + 1..details_end {
        let cell = parser.screen().cell(2, x as u16).unwrap();
        assert!(cell.dim(), "{colored}");
        assert!(!cell.bold(), "{colored}");
    }
    let title = top.trim_end_matches(elapsed).trim_matches('─');
    assert!(title.starts_with(' ') && title.ends_with(' '), "{capture}");
    let start = (elapsed_start - details_start - title.width()) / 2;
    assert_eq!(top.find(title), Some(start * "─".len()), "{capture}");
    println!("{{\"details_borders\":\"pass\",\"width\":{width}}}");
}

#[test]
#[ignore = "requires tmux and NO_COLOR unset; run with --ignored --nocapture"]
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
            !lines[2].contains("Inactive"),
            "status must stay on the bottom rule: {capture}"
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
        assert_details_borders(&tmux, width, height, &capture);
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
                details.contains(" Inactive "),
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
    let list = rows[3..rule].join("\n");
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
                (x > update_x + 45 && y < 20).then_some((y, x))
            })
        })
        .expect("user log row");
    tmux.mouse(0, user_x, user_y, true);
    let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(
        before.lines().skip(22).any(|line| line.contains("User")),
        "{before}"
    );
    tmux.mouse(0, update_x, update_y + 1, true);
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
fn terminal_task_notification_shows_summary_and_fields() {
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
    std::thread::sleep(Duration::from_secs(1));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    println!("{capture}");
    let rows: Vec<_> = capture.lines().collect();
    let rule = rows
        .iter()
        .rposition(|line| line.matches('─').count() > 100)
        .expect("log detail rule");
    let detail = rows[rule + 1..rows.len() - 1].join("\n");
    let summary = "Background command \"等 tab 线程\" completed (exit code 0)";
    assert_eq!(detail.matches(summary).count(), 1, "{capture}");
    for field in [
        "task-id: b9wi31ww0",
        "status: completed",
        "output-file: /tmp/agents-notification/tasks/b9wi31ww0.output",
    ] {
        assert!(detail.contains(field), "{capture}");
    }
    assert!(!detail.contains("tool-use-id"), "{capture}");
    println!("{{\"task_notification\":\"pass\",\"summary_once\":true,\"fields\":true}}");
}

#[test]
#[ignore = "requires tmux and NO_COLOR unset; run with --ignored --nocapture"]
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
    let colored_rows: Vec<_> = colored.lines().collect();
    // Parse just the JSON rows: wide glyphs in the task list must not shift the
    // parser's screen rows before the output under test.
    let mut first = vt100::Parser::new(1, 200, 0);
    first.process(colored_rows[first_row].as_bytes());
    let mut next = vt100::Parser::new(1, 200, 0);
    next.process(colored_rows[first_row + 2].as_bytes());
    let string = first.screen().cell(0, first_column as u16).unwrap();
    let continuation = next.screen().cell(0, first_column as u16).unwrap();
    let key = first
        .screen()
        .cell(0, rows[first_row][..key_byte].width() as u16 + 1)
        .unwrap();
    assert_ne!(key.fgcolor(), vt100::Color::Default);
    assert_ne!(string.fgcolor(), vt100::Color::Default);
    assert_ne!(key.fgcolor(), string.fgcolor());
    assert_eq!(string.fgcolor(), continuation.fgcolor());
    println!("{{\"json_multiline\":\"pass\",\"alignment\":\"pass\",\"colors\":\"pass\"}}");
}
