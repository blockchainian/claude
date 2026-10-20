#[path = "support/terminal_handback.rs"]
mod terminal_handback;
#[path = "support/terminal_task_details_borders.rs"]
mod terminal_task_details_borders;

#[path = "support/terminal_output_borders.rs"]
mod terminal_output_borders;

#[path = "support/terminal_golden_layout.rs"]
mod terminal_golden_layout;

#[path = "support/terminal_path_display.rs"]
mod terminal_path_display;

#[path = "support/terminal_prompt_cache.rs"]
mod terminal_prompt_cache;

#[path = "support/terminal_model_command.rs"]
mod terminal_model_command;

#[path = "support/terminal_weekly_usage.rs"]
mod terminal_weekly_usage;

#[path = "support/terminal_message_text.rs"]
mod terminal_message_text;

#[path = "support/terminal_structured.rs"]
mod terminal_structured;

#[path = "support/terminal_task_navigation.rs"]
mod terminal_task_navigation;

#[path = "support/terminal_memory_citation.rs"]
mod terminal_memory_citation;

#[path = "support/terminal_event_search.rs"]
mod terminal_event_search;

use std::path::Path;
use std::process::Command;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

#[path = "support/terminal_agent_messages.rs"]
mod terminal_agent_messages;

#[path = "support/terminal_questions.rs"]
mod terminal_questions;

#[path = "support/terminal_mcp.rs"]
mod terminal_mcp;

#[path = "support/terminal_headings.rs"]
mod terminal_headings;

#[path = "support/terminal_tool_list.rs"]
mod terminal_tool_list;

#[path = "support/terminal_scripts.rs"]
mod terminal_scripts;

#[path = "support/terminal_disabled_prompt.rs"]
mod terminal_disabled_prompt;

#[path = "support/terminal_prompt_mouse.rs"]
mod terminal_prompt_mouse;

#[path = "support/terminal_images.rs"]
mod terminal_images;

#[path = "support/terminal_drafts.rs"]
mod terminal_drafts;

#[path = "support/terminal_task_follow.rs"]
mod terminal_task_follow;

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
        self.capture_when(ready);
    }

    fn capture_when(&self, ready: impl Fn(&str) -> bool) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let capture = self.run(&["capture-pane", "-t", "probe", "-p"]);
            if ready(&capture) {
                return capture;
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
    let result = start_probe_with_transcript(name, include_str!("fixtures/transcript.jsonl"));
    result.1.wait_for(|capture| capture.contains("Stop hook"));
    result
}

fn begin_named_draft(tmux: &Tmux, name: &str) {
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    assert_title_caret(tmux, "New task", 0);
    tmux.run(&["send-keys", "-t", "probe", "-l", name]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    // The wizard opens the repository browser; a prompt-only probe closes it.
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.split('│').any(|part| part.trim() == ".."))
    });
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| capture.contains(&format!("{name} ❯")));
    tmux.run(&["send-keys", "-t", "probe", "C-w"]);
    tmux.run(&["send-keys", "-t", "probe", "j"]);
}

fn delete_named_draft(tmux: &Tmux) {
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| capture.contains("Permanently delete “"));
    tmux.run(&["send-keys", "-t", "probe", "2"]);
    tmux.wait_for(|capture| !capture.contains("Permanently delete “"));
}

// The name editor shares the details rule with the repository and branch labels.
fn title_rule_position(capture: &str, title: &str) -> Option<(usize, usize)> {
    capture.lines().enumerate().find_map(|(row, line)| {
        let byte = line.find(title)?;
        (line.contains('─') && line.contains('╶')).then(|| (line[..byte].width(), row))
    })
}

fn assert_title_caret(tmux: &Tmux, title: &str, offset: usize) -> String {
    tmux.capture_when(|capture| {
        assert!(!capture.contains("Rename ❯"), "{capture}");
        let Some((column, row)) = title_rule_position(capture, title) else {
            return false;
        };
        let cursor = tmux.run(&[
            "display-message",
            "-t",
            "probe",
            "-p",
            "#{cursor_x},#{cursor_y}",
        ]);
        cursor.trim() == format!("{},{row}", column + offset)
    })
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_background_event_has_no_marker_and_keeps_its_time() {
    let records = include_str!("fixtures/background/two-awaits.jsonl");
    let contents = |count| {
        format!(
            "{}{}\n",
            include_str!("fixtures/transcript.jsonl"),
            records.lines().take(count).collect::<Vec<_>>().join("\n")
        )
    };
    let (_directory, tmux, transcript) =
        start_probe_with_transcript("background-log-marker", &contents(3));
    for (count, time) in [(3, "3m 13s"), (4, "3m 15s")] {
        if count == 4 {
            std::fs::write(&transcript, contents(count)).unwrap();
        }
        let capture = tmux.capture_when(|capture| {
            capture
                .lines()
                .any(|row| row.contains("等用量图颜色线程") && row.trim_end().ends_with(time))
        });
        let row = capture
            .lines()
            .find(|row| row.contains("等用量图颜色线程") && row.trim_end().ends_with(time))
            .unwrap();
        assert!(!row.contains(" *"), "{row}");
    }
    println!(
        "{{\"verdict\":\"pass\",\"background_marker\":false,\"pending_time\":\"3m 13s\",\"completed_time\":\"3m 15s\"}}"
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_queued_task_details_wrap_and_separate_messages() {
    let mut enqueue: serde_json::Value = include_str!("fixtures/claude-main-model-history.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["type"] == "queue-operation" && record["operation"] == "enqueue")
        .unwrap();
    let mut records = include_str!("fixtures/transcript.jsonl").to_owned();
    for text in [
        "Queued first message with enough words to wrap across the details panel and keep its queue_tail_one",
        "Queued second message with enough words to wrap across the details panel and keep its queue_tail_two",
    ] {
        enqueue["content"] = text.into();
        records.push('\n');
        records.push_str(&enqueue.to_string());
    }
    records.push('\n');
    let (_directory, tmux, _transcript) = start_probe_with_transcript("queued-details", &records);
    tmux.run(&["resize-window", "-t", "probe", "-x", "112", "-y", "90"]);
    let capture = tmux.capture_when(|capture| {
        capture.contains("queue_tail_one") && capture.contains("queue_tail_two")
    });
    println!("{capture}");
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 112, 90);
    let screen = parser.screen();
    let lines: Vec<_> = capture.lines().collect();
    let mut previous_tail = None;
    for (first, tail) in [
        ("Queued first", "queue_tail_one"),
        ("Queued second", "queue_tail_two"),
    ] {
        let y = lines.iter().position(|line| line.contains(first)).unwrap();
        let x = lines[y][..lines[y].find(first).unwrap()].width();
        let end = lines.iter().position(|line| line.contains(tail)).unwrap();
        assert!(end > y, "queued message must wrap: {capture}");
        if let Some(previous) = previous_tail {
            assert_eq!(y, previous + 2, "messages need one blank row: {capture}");
        }
        for row in y..=end {
            assert!(
                !screen.cell(row as u16, x as u16).unwrap().bold(),
                "{colored}"
            );
        }
        for column in x..x + first.len() {
            assert_eq!(
                screen
                    .cell((y - 1) as u16, column as u16)
                    .unwrap()
                    .contents()
                    .trim(),
                ""
            );
        }
        previous_tail = Some(end);
    }
    println!(
        "{{\"queued_task_details\":\"pass\",\"wrap\":true,\"bold\":false,\"blank_rows\":true}}"
    );
}

fn start_probe_with_transcript(
    name: &str,
    contents: &str,
) -> (tempfile::TempDir, Tmux, std::path::PathBuf) {
    start_probe_with_setup(name, contents, |_| {})
}

fn start_probe_with_setup(
    name: &str,
    contents: &str,
    setup: impl FnOnce(&Path),
) -> (tempfile::TempDir, Tmux, std::path::PathBuf) {
    start_probe_with_command(name, contents, setup, &[])
}

fn start_probe_with_command(
    name: &str,
    contents: &str,
    setup: impl FnOnce(&Path),
    command_prefix: &[&str],
) -> (tempfile::TempDir, Tmux, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&transcript, contents).unwrap();
    std::fs::create_dir(directory.path().join("clipboard")).unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    setup(directory.path());
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux_config = directory.path().join("tmux.conf");
    std::fs::write(&tmux_config, "set -g default-terminal tmux-256color\n").unwrap();
    let tmux = Tmux(format!("agents-{name}-{}", std::process::id()));
    tmux.run(
        &[
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
            &format!("CLAUDE_CODE_TMPDIR={}", directory.path().display()),
            "-e",
            &format!("CODEX_HOME={}", codex.display()),
            "-e",
            &format!(
                "AGENTS_CLIPBOARD_DIR={}",
                directory.path().join("clipboard").display()
            ),
            "-e",
            &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        ]
        .into_iter()
        .chain(command_prefix.iter().copied())
        .chain(std::iter::once(binary.to_str().unwrap()))
        .collect::<Vec<_>>(),
    );
    // Wait for provider discovery before sending input, including on busy hosts.
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|tabs| tabs.contains("Inactive 1"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    tmux.wait_for(|capture| capture.contains("Update"));
    (directory, tmux, transcript)
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_show_more_keeps_label_while_loading() {
    let (directory, tmux, _transcript) = start_probe("show-more");
    let project = directory.path().join("claude/projects/project");
    for index in 1..12 {
        let id = format!("00000000-0000-0000-0000-000000000{index:03}");
        let records = include_str!("fixtures/transcript.jsonl")
            .lines()
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                record["sessionId"] = id.clone().into();
                record.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(project.join(format!("{id}.jsonl")), records).unwrap();
    }
    let capture = tmux.capture_when(|capture| capture.contains("Inactive 10"));
    let tabs = capture.lines().next().unwrap();
    let inactive = tabs[..tabs.find("Inactive 10").unwrap()].width();
    tmux.mouse(0, inactive, 0, true);
    tmux.wait_for(|capture| capture.contains("Show more"));
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let capture = tmux.capture_when(|capture| capture.contains("Show more"));
    assert!(!capture.contains("Loading more"));
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| capture.contains("Inactive 12"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(!capture.contains("Loading more"));
    println!("{{\"show_more\":\"pass\",\"transient_label\":\"absent\"}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_branch_name_edits_in_task_details_and_preserves_input() {
    let repository = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Probe",
            "-c",
            "user.email=probe@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "probe",
        ],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(repository.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let transcript = include_str!("fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["cwd"] = repository.path().to_str().unwrap().into();
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (_directory, tmux, _transcript) = start_probe_with_transcript("branch-prompt", &transcript);
    tmux.wait_for(|capture| capture.contains("Stop hook"));
    begin_named_draft(&tmux, "Branch probe");
    tmux.run(&["send-keys", "-t", "probe", "-l", "unfinished task"]);
    tmux.wait_for(|capture| capture.contains("Branch probe ❯ unfinished task"));
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.split('│').any(|part| part.trim() == ".."))
    });
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let capture = tmux.capture_when(|capture| capture.contains("Create new branch"));
    assert!(capture.contains("Branch probe ❯"), "{capture}");
    assert!(
        capture.contains("Branch probe ❯ unfinished task"),
        "{capture}"
    );
    assert!(!capture.contains("New branch ❯"), "{capture}");
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Create new branch"))
        .unwrap();
    let create_column = line[..line.find("Create new branch").unwrap()].width();
    let main_row = capture
        .lines()
        .position(|line| line.split('│').any(|part| part.trim() == "main"))
        .unwrap();
    assert!(main_row < row, "{capture}");
    // The checked-out branch is highlighted; select the create row explicitly.
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let input = tmux.capture_when(|capture| capture.contains("New branch ❯"));
    assert!(!input.contains("Create new branch"), "{input}");
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| capture.contains("New branch ❯"));
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    let input_row = input
        .lines()
        .position(|line| line.contains("New branch ❯"))
        .unwrap();
    assert_eq!(
        row, input_row,
        "button and editor share the bottom row: {input}"
    );
    println!(
        "--- branch button screen ---\n{capture}--- branch editor screen ---\n{input}--- branch screens end ---"
    );
    let input_line = input.lines().nth(input_row).unwrap();
    let input_column = input_line[..input_line.find("New branch ❯").unwrap()].width();
    for column in input_column..input_column + "New branch ❯ ".width() {
        assert_eq!(
            parser
                .screen()
                .cell(input_row as u16, column as u16)
                .unwrap()
                .bgcolor(),
            vt100::Color::Default,
            "{colored}"
        );
    }
    tmux.run(&["send-keys", "-t", "probe", "-l", "feature/discard"]);
    tmux.wait_for(|capture| capture.contains("New branch ❯ feature/discard"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    let button = tmux.capture_when(|capture| capture.contains("Create new branch"));
    assert!(!button.contains("feature/discard"), "{button}");
    assert!(button.contains("Branch probe ❯"), "{button}");
    // A click also enters the editor, with the discarded text gone.
    tmux.mouse(0, create_column, row, true);
    let empty = tmux.capture_when(|capture| capture.contains("New branch ❯"));
    assert!(!empty.contains("feature/discard"), "{empty}");
    tmux.run(&["send-keys", "-t", "probe", "-l", "feature/probe"]);
    tmux.run(&["send-keys", "-t", "probe", "Left"]);
    tmux.run(&["send-keys", "-t", "probe", "BSpace"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "b"]);
    tmux.wait_for(|capture| capture.contains("New branch ❯ feature/probe"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| capture.contains("Create new branch"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    let cancelled = tmux.capture_when(|capture| !capture.contains("Create new branch"));
    assert!(
        cancelled.contains("Branch probe ❯ unfinished task"),
        "{cancelled}"
    );
    delete_named_draft(&tmux);
    // Explicit deletion removes both the name and the prompt.
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    let naming = assert_title_caret(&tmux, "New task", 0);
    assert!(!naming.contains("Branch probe"), "{naming}");
    assert!(!naming.contains("unfinished task"), "{naming}");
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    let output = Command::new("git")
        .args(["branch", "--list", "feature/probe", "feature/discard"])
        .current_dir(repository.path())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    println!(
        "{{\"branch_prompt\":\"pass\",\"empty_prompt\":\"pass\",\"branch_edit\":\"pass\",\"cancel\":\"pass\"}}"
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_prompt_newline_entry() {
    let (_directory, tmux, _transcript) = start_probe("prompt-newlines");
    begin_named_draft(&tmux, "Prompt probe");
    for (name, entry) in [
        ("shift-enter", "first line\x1b[13;2usecond line"),
        ("option-enter", "first line\x1b\rsecond line"),
        ("control-j", "first line\nsecond line"),
        ("paste-cr", "\x1b[200~first line\rsecond line\x1b[201~"),
        ("paste-crlf", "\x1b[200~first line\r\nsecond line\x1b[201~"),
        ("paste-lf", "\x1b[200~first line\nsecond line\x1b[201~"),
    ] {
        tmux.run(&["send-keys", "-t", "probe", "-l", entry]);
        tmux.wait_for(|capture| {
            let rows: Vec<_> = capture.lines().collect();
            rows.windows(2).any(|pair| {
                pair[0].contains("Prompt probe ❯ first line") && pair[1].trim() == "second line"
            })
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-prompt-{name}.txt"));
        std::fs::write(&path, capture).unwrap();
        println!(
            "{{\"prompt_newline\":\"pass\",\"entry\":\"{name}\",\"capture\":\"{}\"}}",
            path.display()
        );
        tmux.run(&["send-keys", "-t", "probe", "End", "C-u"]);
        tmux.wait_for(|capture| {
            capture.contains("Prompt probe ❯") && !capture.contains("first line")
        });
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_prompt_marker_caret_and_mouse_alignment() {
    let (_directory, tmux, _transcript) = start_probe("prompt-marker");
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
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
        tmux.wait_for(|capture| capture.contains("Search ❯ Stop"));
        let assert_caret = |needle: &str, offset: usize| {
            tmux.wait_for(|capture| {
                let Some((row, line)) = capture
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains(needle))
                else {
                    return false;
                };
                let column = line[..line.find(needle).unwrap()].width() + offset;
                let cursor = tmux.run(&[
                    "display-message",
                    "-t",
                    "probe",
                    "-p",
                    "#{cursor_x},#{cursor_y}",
                ]);
                cursor.trim() == format!("{column},{row}")
            });
        };
        assert_caret("Search ❯ Stop", 13);
        tmux.run(&["send-keys", "-t", "probe", "Left"]);
        assert_caret("Search ❯ Stop", 12);
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.run(&["send-keys", "-t", "probe", "r"]);
        tmux.run(&["send-keys", "-t", "probe", "Home"]);
        assert_title_caret(&tmux, "Codex manager", 0);
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    }
    begin_named_draft(&tmux, "Prompt probe");
    delete_named_draft(&tmux);
    tmux.wait_for(|capture| {
        // Confirmed deletion returns to the task chooser.
        capture.contains("No task ❮ Select a task to type") && !capture.contains("Prompt probe ❯")
    });
    begin_named_draft(&tmux, "Prompt probe");
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Prompt probe ❯"))
        .unwrap();
    let marker = line[..line.find('❯').unwrap()].width();
    tmux.mouse(0, marker, row, true);
    tmux.run(&["send-keys", "-t", "probe", "-l", "你好"]);
    tmux.wait_for(|capture| capture.contains("Prompt probe ❯ 你好"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let row = capture
        .lines()
        .position(|line| line.contains("Prompt probe ❯ 你好"))
        .unwrap();
    let cursor = tmux.run(&[
        "display-message",
        "-t",
        "probe",
        "-p",
        "#{cursor_x},#{cursor_y}",
    ]);
    assert_eq!(cursor.trim(), format!("{},{row}", marker + 6));
    println!(
        "{{\"prompt_marker\":\"pass\",\"widths\":[120,200],\"caret\":\"pass\",\"mouse\":\"pass\"}}"
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_last_activity_heading_stays_centered() {
    let (_directory, tmux, transcript) = start_probe("activity-heading");
    // Append a recorded tool call so the provider supplies a real activity section.
    let mut call: serde_json::Value = serde_json::from_str(
        include_str!("fixtures/claude-tool-details.jsonl")
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    // Exercise the recorded subagent call in this foreground session.
    call["isSidechain"] = false.into();
    let original = std::fs::read_to_string(&transcript).unwrap();
    std::fs::write(&transcript, format!("{original}\n{call}\n")).unwrap();
    tmux.wait_for(|capture| capture.contains("Last activity"));
    for width in [120, 200] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "70",
        ]);
        tmux.wait_for(|capture| {
            capture.lines().count() == 70
                && capture
                    .lines()
                    .nth(1)
                    .is_some_and(|line| line.width() == width)
                && capture.contains("Last activity")
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let lines: Vec<_> = capture.lines().collect();
        let separators: Vec<_> = lines[2]
            .match_indices('│')
            .map(|(byte, _)| lines[2][..byte].width())
            .collect();
        let left = separators[0] + 2;
        let right = if width >= 150 {
            separators[1] - 1
        } else {
            width - 1
        };
        let heading = lines
            .iter()
            .find(|line| line.contains("Last activity"))
            .unwrap();
        let column = heading[..heading.find("Last activity").unwrap()].width();
        assert_eq!(
            column,
            left + (right - left - "Last activity".len()) / 2,
            "{capture}"
        );
        assert!(!capture.contains("Latest activity"), "{capture}");
        if width >= 150 {
            let summary = "Bash mkdir, cd, ls, wc, head";
            assert!(
                capture.matches(summary).count() >= 2,
                "activity and log must share the summary: {capture}"
            );
        }
        let path = std::env::temp_dir().join(format!("agents-activity-heading-{width}.txt"));
        std::fs::write(&path, &capture).unwrap();
        println!(
            "{{\"activity_heading\":\"pass\",\"width\":{width},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_inline_code_follows_neovim_accent() {
    let (_directory, tmux, _transcript) = start_probe("text-accent");
    for (mode, notification, index) in [
        ("light", "\x1b[?997;2n", 18),
        ("dark", "\x1b[?997;1n", 24),
        ("light", "\x1b[?997;2n", 18),
    ] {
        tmux.run(&["send-keys", "-t", "probe", "-l", notification]);
        tmux.wait_for(|_| {
            let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
            let parser = captured_screen(&capture, 200, 50);
            let screen = parser.screen();
            // The recorded assistant event contains the inline code `start`.
            (25..49).any(|row| {
                (0..195).any(|column| {
                    "start".chars().enumerate().all(|(offset, ch)| {
                        let cell = screen.cell(row, column + offset as u16).unwrap();
                        cell.contents() == ch.to_string()
                            && cell.fgcolor() == vt100::Color::Idx(index)
                    })
                })
            })
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let path = std::env::temp_dir().join(format!("agents-accent-{mode}.ansi"));
        std::fs::write(&path, &capture).unwrap();
        println!(
            "{{\"text_accent\":\"pass\",\"mode\":\"{mode}\",\"index\":{index},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_leaving_list_search_preserves_event_detail() {
    for mouse_exit in [false, true] {
        let (_directory, tmux, _transcript) = start_probe(if mouse_exit {
            "search-exit-mouse"
        } else {
            "search-exit-escape"
        });
        tmux.run(&["resize-window", "-t", "probe", "-x", "120", "-y", "50"]);
        // The wide screen also has long rules; wait for the narrow panel divider
        // before deriving mouse coordinates from the rendered prompt and detail.
        tmux.wait_for(|capture| {
            capture.lines().nth(1).is_some_and(|line| {
                line.find('┬')
                    .is_some_and(|column| line[..column].width() == 60)
            })
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = capture.lines().collect();
        let rule = rows
            .iter()
            .rposition(|line| line.matches('─').count() > 100)
            .unwrap();
        let prompt = rows[..rule]
            .iter()
            .rposition(|line| line.contains('❯'))
            .unwrap();
        tmux.mouse(65, 2, prompt + 2, false);
        tmux.mouse(0, 2, prompt + 4, true);
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
        tmux.wait_for(|capture| capture.contains("Search ❯ Stop"));
        let detail = |capture: &str| {
            capture
                .lines()
                .skip(rule + 1)
                .collect::<Vec<_>>()
                .join("\n")
        };
        let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(detail(&before).contains("manager.mjs:125"), "{before}");
        if !mouse_exit {
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        }
        tmux.mouse(64, 2, rule + 3, false);
        tmux.mouse(0, 2, rule + 3, true);
        tmux.wait_for(|capture| !capture.contains("Search ❯ Stop"));
        let after = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(!after.contains("Search ❯ Stop"), "{after}");
        assert!(
            detail(&after).contains("manager.mjs:125"),
            "detail lost after search exit: {after}"
        );
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
        tmux.wait_for(|capture| detail(capture).contains("Search ❯ Stop"));
        let searched = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(detail(&searched).contains("manager.mjs:125"), "{searched}");
        let path = std::env::temp_dir().join(format!("agents-search-exit-{mouse_exit}.txt"));
        std::fs::write(&path, &searched).unwrap();
        println!(
            "{{\"verdict\":\"pass\",\"capture\":\"{}\"}}",
            path.display()
        );
    }
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
    tmux.run(&["send-keys", "-t", "probe", "End"]);
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
        capture.contains("codex-manager start") && capture.contains("Claude 图片链接"),
        "{capture}"
    );
    assert!(capture.contains("Implement workstream A"), "{capture}");
    println!(
        "{{\"verdict\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_dashboard_tabs_and_footer() {
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
                && capture.contains("? help  esc back  ↑/↓ move  enter open  n new")
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
        assert_eq!(
            rows[44].trim(),
            "? help  esc back  ↑/↓ move  enter open  n new",
            "{capture}"
        );
        let path = std::env::temp_dir().join(format!("agents-dashboard-{width}x45.txt"));
        std::fs::write(&path, &capture).unwrap();
        println!(
            "{{\"dashboard_header_footer\":\"pass\",\"width\":{width},\"capture\":\"{}\"}}",
            path.display()
        );
        tmux.run(&["send-keys", "-t", "probe", "?"]);
        tmux.wait_for(|capture| capture.contains("Task shortcuts"));
        let help = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            help.lines()
                .any(|line| line.contains('g') && line.contains("Group")),
            "{help}"
        );
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| capture.contains("? help"));
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
            !(0..width / 3).any(|column| screen.cell(y, column).unwrap().contents() == "❯"),
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

fn assert_task_details_borders(tmux: &Tmux, width: u16, height: u16, capture: &str) {
    let lines: Vec<_> = capture.lines().collect();
    let separators: Vec<_> = lines[2]
        .match_indices('│')
        .map(|(index, _)| lines[2][..index].width())
        .collect();
    let top_end = 2 + lines[2..]
        .iter()
        .take_while(|line| line.matches('│').count() == separators.len())
        .count();
    let task_details_start = separators[0];
    let task_details_end = if width >= 150 {
        separators[1] + 1
    } else {
        width as usize
    };
    let middle: Vec<_> = lines[top_end].chars().collect();
    let task_details: String = middle[task_details_start..task_details_end]
        .iter()
        .collect();
    let status = " Inactive ";
    let start = (task_details_end - task_details_start - status.len()) / 2;
    assert_eq!(
        task_details
            .chars()
            .skip(start)
            .take(status.len())
            .collect::<String>(),
        status,
        "{capture}"
    );
    assert!(task_details.starts_with('┴'), "{capture}");
    assert!(
        task_details.ends_with(if width >= 150 {
            " claude╶┴"
        } else {
            " claude╶─"
        }),
        "{capture}"
    );
    assert!(
        task_details
            .chars()
            .skip(1)
            .take(start - 1)
            .all(|c| c == '─'),
        "{capture}"
    );
    let folder_start = task_details.chars().count() - " claude╶─".chars().count();
    assert!(
        task_details
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
        .cell(top_end as u16, (task_details_start + start + 1) as u16)
        .unwrap();
    assert!(cell.bold(), "{colored}");
    assert!(!cell.dim(), "{colored}");
    assert_eq!(cell.fgcolor(), vt100::Color::Default);

    let top: String = (task_details_start..task_details_end)
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
    let elapsed_start = task_details_end - 1 - elapsed.width();
    for x in elapsed_start + 1..task_details_end - 2 {
        let cell = parser.screen().cell(1, x as u16).unwrap();
        assert!(cell.dim(), "{colored}");
        assert!(!cell.bold(), "{colored}");
    }
    let title = top
        .split('─')
        .find(|part| !part.trim_matches([' ', '┬']).is_empty())
        .unwrap();
    assert!(title.starts_with(' ') && title.ends_with(' '), "{capture}");
    let start = (task_details_end - task_details_start - title.width()) / 2;
    assert_eq!(top[..top.find(title).unwrap()].width(), start, "{capture}");
    for x in task_details_start + start + 1..task_details_start + start + title.width() - 1 {
        let cell = parser.screen().cell(1, x as u16).unwrap();
        if !cell.contents().is_empty() {
            assert!(cell.bold(), "{colored}");
            assert!(!cell.dim(), "{colored}");
        }
    }
    println!("{{\"task_details_borders\":\"pass\",\"width\":{width}}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_details_borders_keep_status_and_elapsed_separate() {
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
        assert_task_details_borders(&tmux, width, 50, &capture);
    }
}

// Runs the executable's terminal loop, provider discovery and event loading, rather
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
            !capture.contains("Last activity"),
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
        assert!(lines[prompt].contains('❯'), "prompt missing: {capture}");
        let event_rule = if width >= 150 {
            top_end + 2
        } else {
            (prompt + 2..lines.len() - 1)
                .find(|&row| lines[row].matches('─').count() > width as usize / 2)
                .expect("separate list/detail border")
        };
        let top_height = top_end - 2;
        let bottom_height = lines.len() - event_rule - 2;
        if width >= 150 {
            let content = height as usize - 6;
            assert_eq!(top_height, content * 618 / 1000, "{capture}");
            assert_eq!(top_height + bottom_height, content, "{capture}");
        } else {
            let list_height = event_rule - prompt - 2;
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
            !lines[event_rule + 1].trim().is_empty(),
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
        assert_task_details_borders(&tmux, width, height, &capture);
        if width >= 150 {
            let column_width = (width as usize - 8) / 4;
            assert_eq!(separators, [column_width + 2, 2 * column_width + 5]);
            for line in &lines[2..top_end] {
                let event_panel = line.split('│').next_back().unwrap();
                if !event_panel.trim().is_empty() {
                    assert_eq!(
                        event_panel.len() - event_panel.trim_start().len(),
                        1,
                        "log left: {line}"
                    );
                    assert_eq!(
                        line.trim_end().width(),
                        width as usize - 1,
                        "log time right: {line}"
                    );
                }
            }
            assert!(
                lines[top_end + 1].contains('❯'),
                "prompt missing: {capture}"
            );
            let middle: Vec<_> = lines[top_end].chars().collect();
            let task_details: String = middle[separators[0] + 2..separators[1] - 1]
                .iter()
                .collect();
            assert!(
                task_details.contains(" Inactive "),
                "status missing from middle rule: {capture}"
            );
            for line in &lines[2..top_end] {
                let event_panel = line.split('│').next_back().unwrap();
                assert!(!event_panel.contains("────"), "extra log rule: {capture}");
                assert!(!line.contains(" · Inactive"), "extra status row: {capture}");
            }
            assert!(lines[event_rule].trim().chars().all(|c| c == '─'));
            assert!(
                lines[event_rule + 4..lines.len() - 1]
                    .iter()
                    .any(|line| line.width() > width as usize * 2 / 3),
                "detail does not span the terminal: {capture}"
            );
            assert!(
                lines[2..top_end].iter().any(|line| {
                    line.split('│')
                        .next_back()
                        .is_some_and(|event_panel| event_panel.contains("Stop hook"))
                }),
                "event list missing from top row: {capture}"
            );
        } else {
            let list = lines[prompt + 2..event_rule].join("\n");
            assert!(
                list.contains("Stop hook"),
                "event list missing below prompt: {capture}"
            );
            assert!(
                lines[event_rule + 1..lines.len() - 1]
                    .join("\n")
                    .contains("decision"),
                "newest event detail missing below list: {capture}"
            );
            let user_row = (prompt + 1..event_rule)
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
                opened_lines[prompt + 2..event_rule].join("\n"),
                list,
                "event list hidden after click: {opened}"
            );
            assert!(!opened_lines[event_rule].contains("↓ Latest"), "{opened}");
            assert!(
                opened_lines[event_rule + 1..opened_lines.len() - 1]
                    .join("\n")
                    .contains("run_in_background"),
                "clicked user event missing from detail: {opened}"
            );
            // Search follows the last mouse panel, independently of the selected event.
            tmux.mouse(0, 1, event_rule + 1, true);
            tmux.run(&["send-keys", "-t", "probe", "/"]);
            tmux.run(&["send-keys", "-t", "probe", "-l", "await"]);
            std::thread::sleep(Duration::from_millis(200));
            let searched = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            assert!(
                searched
                    .lines()
                    .skip(event_rule + 1)
                    .any(|line| line.contains("Search ❯ await")),
                "detail search missing: {searched}"
            );
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
            // Select the real newest event so each resize starts with the same detail.
            std::thread::sleep(Duration::from_millis(200));
            let assistant_row = (prompt + 2..event_rule)
                .find(|&row| opened_lines[row].contains("对，必须"))
                .expect("newest assistant row");
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
fn terminal_compaction_summary_keeps_row_and_labels_event_detail() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("compaction");
    std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap()
        .write_all(include_bytes!("fixtures/claude-compaction.jsonl"))
        .unwrap();
    let capture = tmux.capture_when(|capture| {
        capture.contains("This session is being continued") && capture.contains("Compaction")
    });
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
    println!("{{\"compaction_list\":\"pass\",\"compaction_event_detail\":\"pass\"}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_event_selection_tracks_wheel_click_and_tail() {
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
            .rposition(|row| row.contains('❯'))
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
        tmux.mouse(64, column, row, false);
        let older = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(detail(&older).contains("run_in_background"), "{older}");
        assert!(!older.contains("↓ Latest"), "{older}");
        tmux.mouse(65, column, row, false);
        let newest = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(detail(&newest).contains("decision"), "{newest}");
        assert!(!newest.contains("↓ Latest"), "{newest}");
        tmux.mouse(0, column, row, true);
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
            .find(|(_, line)| line.contains("↓ 1 New"))
            .expect("new event jump");
        let jump_column = jump_line[..jump_line.find("↓ 1 New").unwrap()].width();
        tmux.mouse(0, jump_column, jump_row, true);
        let following = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            detail(&following).contains("terminal-tail-proof"),
            "{following}"
        );
        assert!(!following.contains("↓ 1 New"), "{following}");
        arrival["uuid"] = "terminal-pinned-arrival".into();
        arrival["timestamp"] = "2026-10-06T16:42:00Z".into();
        arrival["message"]["content"] = "terminal-pinned-proof".into();
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .unwrap(),
            "{arrival}"
        )
        .unwrap();
        tmux.wait_for(|capture| detail(capture).contains("terminal-pinned-proof"));
        let pinned = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            !pinned.contains("↓ Latest") && !pinned.contains("↓ 1 New"),
            "{pinned}"
        );
        let list: String = pinned
            .lines()
            .skip(row)
            .take(if width >= 150 {
                prompt - row - 1
            } else {
                pinned
                    .lines()
                    .enumerate()
                    .filter_map(|(index, line)| {
                        (line.matches('─').count() > width / 2).then_some(index)
                    })
                    .last()
                    .unwrap()
                    - row
            })
            .map(|line| line.split('│').next_back().unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let first = list.find("terminal-tail-proof").unwrap();
        let last = list.find("terminal-pinned-proof").unwrap();
        assert!(first < last, "arrivals must append downwards: {pinned}");
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
fn terminal_task_notification_shows_summary_without_id_and_missing_file() {
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
    assert!(!detail.contains("Task b9wi31ww0"), "{capture}");
    assert!(detail.contains("File no longer exists"), "{capture}");
    assert!(!detail.contains("output-file:"), "{capture}");
    let raw = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&raw, 200, 50);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("File no longer exists"))
        .unwrap();
    let column = line[..line.find("File no longer exists").unwrap()].width();
    assert!(
        parser
            .screen()
            .cell(row as u16, column as u16)
            .unwrap()
            .dim(),
        "{raw}"
    );
    assert!(!detail.contains("task-id:"), "{capture}");
    assert!(!detail.contains("status: completed"), "{capture}");
    assert!(!detail.contains("tool-use-id"), "{capture}");
    println!("{{\"task_notification\":\"pass\",\"summary_once\":true,\"missing_file\":true}}");
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
    tmux.run(&["send-keys", "-t", "probe", "End"]);
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
        rows[first_row][..key_byte].width(),
        "continuation must use the field's structural indent: {capture}"
    );
    assert_eq!(
        rows[first_row + 3].trim().trim_end_matches(','),
        "\"",
        "{capture}"
    );
    assert_eq!(
        rows[first_row + 3].find('"').unwrap(),
        rows[first_row][..key_byte].width(),
        "the trailing newline must precede the closing quote: {capture}"
    );
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    let screen = parser.screen();
    let string = screen.cell(first_row as u16, first_column as u16).unwrap();
    let continuation = screen
        .cell(
            (first_row + 2) as u16,
            rows[first_row][..key_byte].width() as u16,
        )
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
    tmux.run(&["send-keys", "-t", "probe", "End"]);
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
        assert_eq!(x, terminal_event_detail_width::event_detail_center(200, 3));
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

fn background_text_probe(name: &str, text: &str) -> (tempfile::TempDir, Tmux, std::path::PathBuf) {
    let (directory, tmux, transcript) = start_probe(name);
    let background = include_str!("fixtures/background/records.jsonl");
    let record: serde_json::Value =
        serde_json::from_str(background.lines().next().unwrap()).unwrap();
    let slug: String = record["cwd"]
        .as_str()
        .unwrap()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let outputs = directory
        .path()
        .join(format!("claude-{}", unsafe { libc::getuid() }))
        .join(slug)
        .join(transcript.file_stem().unwrap())
        .join("tasks");
    std::fs::create_dir_all(&outputs).unwrap();
    let result: serde_json::Value =
        serde_json::from_str(background.lines().nth(1).unwrap()).unwrap();
    let output = outputs.join(format!(
        "{}.output",
        result["toolUseResult"]["backgroundTaskId"]
            .as_str()
            .unwrap()
    ));
    std::fs::write(&output, text).unwrap();
    tmux.run(&[
        "new-window",
        "-d",
        "-n",
        "writer",
        &format!("sleep 300 >> '{}'", output.display()),
    ]);
    (directory, tmux, transcript)
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_details_section_positions() {
    let (_directory, tmux, transcript) =
        background_text_probe("details-sections", "Running checks\n");
    let background = include_str!("fixtures/background/records.jsonl");
    let original = format!(
        "{}\n{background}\n{}",
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
        tmux.wait_for(|capture| {
            capture.contains("#763") == has_pr
                && capture.contains("Task 1")
                && capture.contains("Background")
                && capture.contains("Running checks")
        });
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
            let positions = ["Background", "Last activity", "Update", "Prompt", "Tasks"]
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
            assert!(model.contains("claude-opus-5-5 · medium"), "{model}");
            assert!(!model.contains("Model"));
            assert!(model.contains('·'));
            let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
            let colored = captured_screen(&colored, width, 70);
            for (start, length) in [
                (model_left + 2, "claude-opus-5-5".len()),
                (
                    model_left + 2 + "claude-opus-5-5 · ".width(),
                    "medium".len(),
                ),
            ] {
                for x in start..start + length {
                    let cell = colored.screen().cell(model_y as u16, x as u16).unwrap();
                    assert!(cell.dim(), "{capture}");
                    assert!(!cell.bold(), "{capture}");
                }
            }
            println!(
                "{{\"task_details_sections\":\"pass\",\"folder_on_status_rule\":\"pass\",\"model_on_event_rule\":\"pass\",\"metadata_dim\":\"pass\",\"width\":{width},\"pr\":{has_pr}}}"
            );
        }
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_details_background_before_activity() {
    let (_directory, tmux, transcript) =
        background_text_probe("background-before-activity", "Running checks\n");
    let content = format!(
        "{}\n{}\n{}",
        std::fs::read_to_string(&transcript).unwrap(),
        include_str!("fixtures/background/records.jsonl"),
        include_str!("fixtures/claude-todos.jsonl")
    );
    std::fs::write(&transcript, content).unwrap();
    tmux.run(&["resize-window", "-t", "probe", "-x", "112", "-y", "70"]);
    let capture = tmux.capture_when(|capture| {
        let positions = ["Background", "Last activity", "Update", "Prompt", "Tasks"]
            .map(|label| capture.lines().position(|row| row.contains(label)));
        positions.iter().all(Option::is_some) && positions.is_sorted()
    });
    println!("{capture}");
    println!("{{\"task_details_background_before_activity\":\"pass\"}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_details_needs_input_follows_prompt_before_tasks() {
    let (directory, tmux, transcript) = start_probe("details-needs-input");
    let job = directory.path().join("claude/jobs/details-needs-input");
    std::fs::create_dir_all(&job).unwrap();
    // Safe fields from the real Claude Code background state payload.
    let mut state: serde_json::Value = serde_json::from_str(
        r#"{"state":"done","template":"bg","sessionId":"e772d00b-c424-4632-a8df-74bc9c7b1629","respawnFlags":[],"backend":"daemon"}"#,
    )
    .unwrap();
    state["sessionId"] = transcript.file_stem().unwrap().to_str().unwrap().into();
    state["needs"] = "send a prompt to start".into();
    std::fs::write(job.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    let original = std::fs::read_to_string(&transcript).unwrap();
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "70"]);
    std::fs::write(
        &transcript,
        format!(
            "{original}\n{}",
            include_str!("fixtures/claude-todos.jsonl")
        ),
    )
    .unwrap();
    let capture = tmux.capture_when(|capture| {
        capture.lines().any(|line| {
            line.split('│')
                .nth(1)
                .is_some_and(|panel| panel.trim() == "Needs input")
        })
    });
    let panel: Vec<_> = capture
        .lines()
        .filter_map(|line| line.split('│').nth(1))
        .collect();
    let positions = ["Prompt", "Needs input", "Tasks"].map(|label| {
        panel
            .iter()
            .position(|line| line.trim_start().starts_with(label))
            .unwrap()
    });
    assert!(positions.is_sorted(), "{capture}");
    assert!(!capture.contains("Needs attention"), "{capture}");
    assert!(capture.contains("send a prompt to start"), "{capture}");
    println!("{{\"needs_input_after_prompt_before_tasks\":\"pass\"}}");
}

#[path = "support/terminal_codex_exit.rs"]
mod terminal_codex_exit;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_event_metadata_aligns_new_with_clock() {
    for width in [80, 200] {
        let (_directory, tmux, transcript) = start_probe("log-latest-border");
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        tmux.wait_for(|capture| {
            capture
                .lines()
                .any(|row| row.contains("medium") && row.width() == width)
        });
        let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = before.lines().collect();
        let model = rows
            .iter()
            .position(|row| row.contains("claude-opus-5-5") || row.contains("Opus 5.5"))
            .unwrap_or_else(|| panic!("model border missing: {before}"));
        let (column, top, bottom) = if width < 150 {
            let bottom = (model + 1..rows.len())
                .find(|&row| rows[row].matches('─').count() > width / 2)
                .unwrap();
            (1, model, bottom)
        } else {
            let separator = rows[2].rfind('│').unwrap();
            (rows[2][..separator].width() + 2, 1, model)
        };
        tmux.mouse(64, column, top + 1, false);
        tmux.wait_for(|capture| capture.contains("User"));
        let without_arrivals = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(!without_arrivals.contains("↓ Latest"), "{without_arrivals}");
        let panel_width = width - column - 1;
        let former = column + panel_width / 2;
        tmux.mouse(0, former, bottom, true);
        let clicked = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(clicked.contains("User"), "{clicked}");
        let mut arrival: serde_json::Value = serde_json::from_str(
            include_str!("fixtures/transcript.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        arrival["uuid"] = "terminal-new-jump".into();
        arrival["timestamp"] = "2026-10-06T16:41:00Z".into();
        arrival["message"]["content"] = "terminal-new-jump-proof".into();
        use std::io::Write;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .unwrap(),
            "{arrival}"
        )
        .unwrap();
        tmux.wait_for(|capture| capture.contains("↓ 1 New"));
        let older = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = older.lines().collect();
        assert!(
            rows[model].contains("↓ 1 New"),
            "New missing from model border: {older}"
        );
        assert!(rows[model].contains(" · medium"), "{older}");
        assert!(
            (rows[model].contains("claude-opus-5-5") || rows[model].contains("Opus 5.5"))
                && rows[model].contains("medium"),
            "{older}"
        );
        let byte = rows[model].find("↓ 1 New").unwrap();
        let jump = rows[model][..byte].width();
        assert_eq!(jump + "↓ 1 New".width(), width - 1);
        println!("{older}");
        if width < 150 {
            tmux.mouse(65, 2, bottom + 3, false);
            let scrolled = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            assert_ne!(scrolled, older);
            assert!(!scrolled.contains("↑ Top"), "{scrolled}");
            assert!(
                scrolled.lines().nth(model).unwrap().contains("↓ 1 New"),
                "{scrolled}"
            );
        }
        tmux.mouse(0, jump, model, true);
        tmux.wait_for(|capture| {
            !capture.contains("↓ 1 New") && capture.contains("terminal-new-jump-proof")
        });
        let double_click = |column: usize, row: usize| {
            let column = column + 1;
            let row = row + 1;
            let sequence = format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").repeat(2);
            tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
        };
        let border_column = column + (width - column) / 3;
        double_click(border_column, top);
        tmux.wait_for(|capture| !capture.contains("↓ 1 New") && capture.contains("User"));
        double_click(border_column, bottom);
        if width < 150 {
            let shared = tmux.capture_when(|capture| {
                capture
                    .lines()
                    .skip(bottom + 1)
                    .take(48 - bottom - 1)
                    .any(|line| line.contains("User"))
            });
            assert!(
                !shared
                    .lines()
                    .skip(bottom + 1)
                    .take(48 - bottom - 1)
                    .any(|line| line.contains("terminal-new-jump-proof")),
                "the shared rule scrolls output without selecting the newest event: {shared}"
            );
            let latest = shared
                .lines()
                .enumerate()
                .skip(top + 1)
                .take(bottom - top - 1)
                .find(|(_, line)| line.contains("terminal-new-jump-proof"))
                .expect("newest event remains in the log list")
                .0;
            tmux.mouse(0, column + 2, latest, true);
            tmux.wait_for(|capture| {
                capture
                    .lines()
                    .skip(bottom + 1)
                    .take(48 - bottom - 1)
                    .any(|line| line.contains("terminal-new-jump-proof"))
            });
        } else {
            tmux.wait_for(|capture| {
                !capture.contains("↓ 1 New") && capture.contains("terminal-new-jump-proof")
            });
        }
        tmux.mouse(64, column, top + 1, false);
        tmux.wait_for(|capture| capture.contains("decision"));
        double_click(jump, model);
        tmux.wait_for(|capture| {
            !capture.contains("↓ 1 New") && capture.contains("terminal-new-jump-proof")
        });
        let path = std::env::temp_dir().join(format!("agents-latest-border-{width}x50.txt"));
        std::fs::write(&path, older).unwrap();
        println!(
            "{{\"new_clock_alignment\":\"pass\",\"width\":{width},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_event_border_double_clicks() {
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
    double_click(2);
    tmux.wait_for(|capture| {
        capture
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains("Codex Manager 在一个 turn")
    });
    let oldest = capture("oldest");
    assert!(
        oldest
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains("Codex Manager 在一个 turn"),
        "{oldest}"
    );
    double_click(bottom + 1);
    tmux.wait_for(|capture| !capture.contains("↓ Latest") && capture.contains("对，必须"));
    let latest = capture("latest");
    assert!(latest.contains("对，必须"));
    println!("{{\"log_border_double_clicks\":\"pass\",\"width\":200,\"height\":45}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_session_search_overlays_chart_bottom() {
    // Move the real transcript's usage into the visible time window.
    let records: Vec<_> = include_str!("fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["timestamp"] = chrono::Utc::now().to_rfc3339().into();
            serde_json::to_string(&record).unwrap()
        })
        .collect();
    let (_directory, tmux, _transcript) =
        start_probe_with_transcript("session-search", &(records.join("\n") + "\n"));
    let braille = |text: &str| text.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c));
    for (width, height) in [(200, 50), (120, 45), (80, 12)] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ]);
        tmux.wait_for(|capture| {
            capture.lines().count() == height && capture.lines().take(height / 2).any(&braille)
        });
        std::thread::sleep(Duration::from_millis(200));
        // Compare within one fixed pair of five-second usage bins so the time axis
        // cannot advance between the baseline and search captures.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let (before, after) = loop {
            let pair = chrono::Utc::now().timestamp().div_euclid(10);
            let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            tmux.run(&["send-keys", "-t", "probe", "/"]);
            tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
            tmux.wait_for(|capture| capture.contains("Search ❯ Stop"));
            let after = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            if pair == chrono::Utc::now().timestamp().div_euclid(10) {
                break (before, after);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "capture crossed usage bins"
            );
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
            tmux.wait_for(|capture| !capture.contains("Search ❯"));
        };
        let list_width = before
            .lines()
            .nth(2)
            .unwrap()
            .chars()
            .position(|c| c == '│')
            .unwrap();
        let left = |line: &str| line.chars().take(list_width).collect::<String>();
        let rows: Vec<_> = before.lines().map(left).collect();
        let panel_bottom = rows.iter().rposition(|line| braille(line)).unwrap();
        let searched: Vec<_> = after.lines().map(left).collect();
        let input = searched
            .iter()
            .position(|line| line.contains("Search ❯ Stop"))
            .unwrap();
        assert_eq!(input, panel_bottom, "{width}x{height}: {after}");
        assert!(
            !braille(&searched[input]),
            "chart dots in search row: {after}"
        );
        assert!(
            !searched[2].contains("Search ❯")
                && (searched[2].contains("claude") || searched[2].contains("Codex manager")),
            "top row must remain the list: {after}"
        );
        for (y, row) in rows
            .iter()
            .enumerate()
            .filter(|(y, line)| *y < input && braille(line))
        {
            assert_eq!(&searched[y], row, "chart changed at row {y}: {after}");
        }
        let path = std::env::temp_dir().join(format!("agents-session-search-{width}x{height}.txt"));
        std::fs::write(&path, &after).unwrap();
        println!(
            "{{\"verdict\":\"pass\",\"capture\":\"{}\"}}",
            path.display()
        );
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| !capture.contains("Search ❯"));
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_detail_reading_column_preserves_original_copy() {
    use std::io::Write;

    let calls: Vec<_> = include_str!("fixtures/claude-tool-calls.jsonl")
        .lines()
        .collect();
    let agents: Vec<_> = include_str!("fixtures/claude-agent-display.jsonl")
        .lines()
        .collect();
    let user = include_str!("fixtures/claude-raw-detail.jsonl")
        .lines()
        .find(|line| {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["type"] == "user"
                && record["message"]["content"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("cctail"))
        })
        .expect("recorded user message");
    // Keep recorded upstream bodies, field names and command line breaks intact.
    for (kind, records, needle, prose) in [
        ("assistant", String::new(), "对，必须", true),
        // Declared prompt inputs use the reading column.
        (
            "send",
            include_str!("fixtures/reading-column-send.jsonl").to_owned(),
            "Additional scope",
            true,
        ),
        ("user", format!("{user}\n"), "cctail", true),
        (
            "compaction",
            include_str!("fixtures/claude-compaction.jsonl").to_owned(),
            "This session is being continued",
            true,
        ),
        (
            "queued",
            format!(
                "{}\n",
                include_str!("fixtures/claude-agent-names.jsonl")
                    .lines()
                    .nth(2)
                    .unwrap()
            ),
            "Checks passed.",
            true,
        ),
        (
            "question",
            include_str!("fixtures/claude-questions-pending.jsonl").to_owned(),
            "• 怎么判断",
            false,
        ),
        (
            "agent-prompt",
            format!("{}\n", agents[2]),
            "You are the fork",
            false,
        ),
        (
            "tool-prompt",
            include_str!("fixtures/claude-manager-start.jsonl").to_owned(),
            "Implement workstream A",
            true,
        ),
        (
            "command",
            format!("{}\n", calls[0]),
            "pkill -f slide_boxes",
            false,
        ),
        (
            "output",
            format!("{}\n{}\n", calls[2], calls[3]),
            "default-terminal",
            false,
        ),
    ] {
        let (directory, tmux, transcript) = start_probe(&format!("reading-{kind}"));
        if !records.is_empty() {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .unwrap()
                .write_all(records.as_bytes())
                .unwrap();
            tmux.wait_for(|capture| capture.lines().skip(25).any(|line| line.contains(needle)));
        }
        for width in [200_u16, 120] {
            tmux.run(&[
                "resize-window",
                "-t",
                "probe",
                "-x",
                &width.to_string(),
                "-y",
                "50",
            ]);
            tmux.wait_for(|capture| {
                capture.lines().count() == 50
                    && capture.lines().skip(25).any(|line| line.contains(needle))
            });
            std::thread::sleep(Duration::from_millis(200));
            let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            let path = std::env::temp_dir().join(format!("agents-reading-{kind}-{width}.txt"));
            std::fs::write(&path, &capture).unwrap();
            let rows: Vec<_> = capture.lines().collect();
            let rule = rows
                .iter()
                .rposition(|line| line.matches('─').count() > usize::from(width / 2))
                .expect("detail header rule");
            assert!(
                rows[rule + 2].trim_end_matches('│').trim().is_empty(),
                "{capture}"
            );
            let (row, line) = rows
                .iter()
                .enumerate()
                .skip(rule + 1)
                .find(|(_, line)| line.contains(needle))
                .expect("recorded body in event detail");
            let left = if kind == "question" {
                line.len() - line.trim_start().len()
            } else {
                line[..line.find(needle).unwrap()].width()
            };
            let group_start = rows
                .iter()
                .enumerate()
                .skip(rule + 3)
                .take(row.saturating_sub(rule + 3))
                .filter(|(_, line)| line.trim() == "↓")
                .map(|(index, _)| index + 1)
                .next_back()
                .unwrap_or(rule + 3);
            let group: Vec<_> = rows
                .iter()
                .skip(group_start)
                .take_while(|line| line.trim() != "↓" && !line.starts_with(" ? help"))
                .map(|line| line.trim_end().trim_end_matches('│').trim_end())
                .filter(|line| !line.trim().is_empty())
                .collect();
            let group_left = group
                .iter()
                .map(|line| line.width() - line.trim_start().width())
                .min()
                .expect("detail body group");
            let group_right = group.iter().map(|line| line.width()).max().unwrap();
            let cap = terminal_event_detail_width::message_width(&capture, usize::from(width));
            if prose {
                assert!(
                    group_right - group_left <= cap,
                    "{kind} exceeds actual message column {cap}: {capture}"
                );
            }
            // A group can continue below the viewport: its widest source row
            // need not be among the captured rows used for these bounds.
            let maximum_width = if prose {
                cap
            } else {
                terminal_event_detail_width::content_width(usize::from(width))
            };
            let leftmost =
                terminal_event_detail_width::event_detail_center(usize::from(width), maximum_width);
            let rightmost = terminal_event_detail_width::event_detail_center(
                usize::from(width),
                group_right - group_left,
            );
            assert!(
                (leftmost..=rightmost).contains(&group_left),
                "{kind} group center outside {leftmost}..={rightmost} at {width}: {capture}"
            );
            assert!(
                group_right <= usize::from(width) - 11,
                "{kind} enters the time column at {width}: {capture}"
            );
            if kind == "send" {
                assert!(
                    rows[rule + 1]
                        .trim_start()
                        .starts_with("codex-manager send"),
                    "tool title missing from centered header: {capture}"
                );
                assert!(
                    rows[rule + 1].contains("00000000-0000-4000-8000-000000000000"),
                    "{capture}"
                );
                assert!(group[0].contains("Additional scope"), "{capture}");
                assert!(
                    !group.iter().any(|row| row.contains("\"prompt\":")),
                    "{capture}"
                );
                // Copy the input block, including bytes hidden by visual truncation.
                let clipboard = directory.path().join("clipboard/text");
                if clipboard.exists() {
                    std::fs::remove_file(&clipboard).unwrap();
                }
                let click = format!(
                    "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                    left + 1,
                    row + 1,
                    left + 1,
                    row + 1
                )
                .repeat(2);
                tmux.run(&["send-keys", "-t", "probe", "-l", &click]);
                tmux.wait_for(|_| clipboard.exists());
                let copied = std::fs::read_to_string(&clipboard).unwrap();
                let record: serde_json::Value = serde_json::from_str(records.trim()).unwrap();
                let input = &record["message"]["content"][0]["input"];
                assert_eq!(copied, input["prompt"].as_str().unwrap());
                assert!(
                    !copied.contains(input["threadId"].as_str().unwrap()),
                    "{copied}"
                );
            }
            if kind == "tool-prompt" {
                let header = rows[rule + 1];
                assert!(
                    header.trim_start().starts_with("codex-manager start"),
                    "{capture}"
                );
                assert!(header.contains("Claude 图片链接"), "{capture}");
                assert!(group[0].contains("Implement workstream A"), "{capture}");
                assert!(
                    !group.iter().any(|line| line.contains("\"cwd\":")),
                    "{capture}"
                );
            }
            if kind == "assistant" {
                // Drag in screen coordinates to the next character's boundary.
                tmux.mouse(0, left, row, false);
                tmux.mouse(32, left + "对，必须".width(), row, false);
                let release = format!("\x1b[<0;{};{}m", left + "对，必须".width() + 1, row + 1);
                tmux.run(&["send-keys", "-t", "probe", "-l", &release]);
                // A previous copy can leave the same toast visible. Wait for this
                // gesture's clipboard value before reading it.
                tmux.wait_for(|capture| {
                    capture.contains("Copied")
                        && std::fs::read_to_string(directory.path().join("clipboard/text"))
                            .is_ok_and(|text| text == "对，必须")
                });
                let selected =
                    std::fs::read_to_string(directory.path().join("clipboard/text")).unwrap();
                assert_eq!(selected, "对，必须");
                std::thread::sleep(Duration::from_millis(350));
                // Visual wrapping must not change the Markdown or source paragraphs copied.
                let click = format!(
                    "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                    left + 1,
                    row + 1,
                    left + 1,
                    row + 1
                )
                .repeat(2);
                tmux.run(&["send-keys", "-t", "probe", "-l", &click]);
                let source: serde_json::Value = serde_json::from_str(
                    include_str!("fixtures/transcript.jsonl")
                        .lines()
                        .nth(1)
                        .unwrap(),
                )
                .unwrap();
                let expected = source["message"]["content"][0]["text"].as_str().unwrap();
                tmux.wait_for(|capture| {
                    capture.contains("Copied message")
                        && std::fs::read_to_string(directory.path().join("clipboard/text"))
                            .is_ok_and(|text| text == expected)
                });
                let copied =
                    std::fs::read_to_string(directory.path().join("clipboard/text")).unwrap();
                assert_eq!(copied, expected);
            }
            println!(
                "{{\"reading_column\":\"pass\",\"kind\":\"{kind}\",\"width\":{width},\"capture\":\"{}\"}}",
                path.display()
            );
        }
    }
}

#[path = "support/terminal_chronological.rs"]
mod terminal_chronological;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_manager_detach_title() {
    use std::io::Write;
    let (_directory, tmux, transcript) = start_probe("manager-detach-title");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    write!(
        file,
        "{}",
        include_str!("fixtures/claude-manager-detach.jsonl")
    )
    .unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.wait_for(|capture| {
        capture.contains("codex-manager detach") && capture.contains("archived")
    });
    for width in [200_u16, 120, 150, 149] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        tmux.wait_for(|capture| {
            capture.lines().count() == 50
                && capture.lines().nth(1).is_some_and(|line| {
                    line.matches('┬').count() == if width >= 150 { 2 } else { 1 }
                })
                && capture.lines().any(|line| {
                    line.width() == usize::from(width)
                        && line.matches('─').count() > usize::from(width / 2)
                })
                && capture.contains("codex-manager detach")
                && capture.contains("archived")
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = capture.lines().collect();
        let rule = rows
            .iter()
            .rposition(|line| line.matches('─').count() > usize::from(width / 2))
            .unwrap();
        let title = rows[rule + 1];
        assert!(
            rows[rule + 2].trim_end_matches('│').trim().is_empty(),
            "{capture}"
        );
        assert!(
            title.trim_start().starts_with("codex-manager detach"),
            "{capture}"
        );
        let record: serde_json::Value = serde_json::from_str(
            include_str!("fixtures/claude-manager-detach.jsonl")
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        let subject = record["message"]["content"][0]["input"]["threadId"]
            .as_str()
            .unwrap();
        assert!(title.contains(subject), "{capture}");
        assert_eq!(
            title[..title.find(subject).unwrap()].width(),
            terminal_event_detail_width::event_detail_center(usize::from(width), subject.width()),
            "thread title stays on the panel midline: {capture}"
        );
        assert_eq!(rows[rule + 3].trim(), "↓", "{capture}");
        assert_eq!(
            rows[rule + 3][..rows[rule + 3].find('↓').unwrap()].width(),
            terminal_event_detail_width::event_detail_center(usize::from(width), 1),
            "empty input arrow stays on the panel midline: {capture}"
        );
        assert!(
            capture.contains("\"archived\": true") && capture.contains("\"detached\": true"),
            "{capture}"
        );
        let path = std::env::temp_dir().join(format!("agents-manager-detach-title-{width}.txt"));
        std::fs::write(&path, &capture).unwrap();
        println!(
            "{{\"verdict\":\"pass\",\"width\":{width},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

#[path = "support/terminal_event_detail_width.rs"]
mod terminal_event_detail_width;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_start_tool_name_header() {
    use std::io::Write;
    let (_directory, tmux, transcript) = start_probe("start-tool-name");
    std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap()
        .write_all(include_str!("fixtures/claude-manager-start.jsonl").as_bytes())
        .unwrap();
    for width in [200_u16, 90] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        tmux.wait_for(|capture| {
            capture.lines().count() == 50
                && capture.lines().any(|line| {
                    line.trim_start().starts_with("codex-manager start")
                        && line.contains("Claude 图片链接")
                        && line.contains("codex-manager start")
                })
                && capture.contains("Implement workstream A")
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-start-tool-name-live-{width}.txt"));
        std::fs::write(&path, &capture).unwrap();
        let rows: Vec<_> = capture.lines().collect();
        let header = rows
            .iter()
            .rposition(|line| {
                line.contains("Claude 图片链接") && line.contains("codex-manager start")
            })
            .expect("tool name title bar");
        let bar = rows[header];
        let clock = |line: &str| -> String {
            line.chars()
                .scan(0, |column, ch| {
                    let start = *column;
                    *column += ch.to_string().width();
                    Some((start, ch))
                })
                .filter(|(column, _)| {
                    *column >= usize::from(width) - 9 && *column < usize::from(width) - 1
                })
                .map(|(_, ch)| ch)
                .collect::<String>()
                .trim()
                .to_owned()
        };
        let list_start = if width >= 150 {
            2
        } else {
            rows[..header]
                .iter()
                .rposition(|line| line.contains("claude-opus-5-5") || line.contains("Opus 5.5"))
                .expect("log list model border")
                + 1
        };
        let list_row = rows[list_start..header]
            .iter()
            .find(|line| {
                if width >= 150 {
                    line.rsplit_once('│')
                        .is_some_and(|(_, list)| list.contains("Claude 图片链接"))
                } else {
                    line.contains("Claude 图片链接")
                }
            })
            .expect("selected tool list row");
        assert_eq!(
            clock(bar),
            clock(list_row),
            "event time stays at the panel edge: {capture}"
        );
        assert!(
            bar.trim_end_matches('│').trim_end().width() < usize::from(width),
            "{capture}"
        );
        assert_eq!(
            bar[..bar.find("codex-manager start").unwrap()].width(),
            1,
            "tool name stays in the left segment: {capture}"
        );
        assert_eq!(
            bar[..bar.find("Claude 图片链接").unwrap()].width(),
            terminal_event_detail_width::event_detail_center(
                usize::from(width),
                "Claude 图片链接".width()
            ),
            "tool name stays on the panel midline: {capture}"
        );
        assert!(bar.contains("Claude 图片链接"), "{capture}");
        assert!(
            rows[header + 2].contains("Implement workstream A"),
            "{capture}"
        );
        assert!(
            !rows
                .iter()
                .skip(header + 2)
                .any(|line| line.contains("\"cwd\":")),
            "{capture}"
        );
        println!(
            "{{\"verdict\":\"pass\",\"width\":{width},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_notification_json_preserves_lines_wide_and_narrow() {
    use std::io::Write;
    let (directory, tmux, transcript) = start_probe("notification-preformatted");
    let output = directory.path().join("completion.output");
    std::fs::write(
        &output,
        include_str!("fixtures/claude-task-notification-output.txt"),
    )
    .unwrap();
    let notifications = include_str!("fixtures/claude-task-notifications.jsonl").replace(
        "/tmp/agents-notification/tasks/b9wi31ww0.output",
        output.to_str().unwrap(),
    );
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(transcript)
            .unwrap(),
        "{notifications}"
    )
    .unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    for width in [200, 120] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "90",
        ]);
        tmux.wait_for(|capture| {
            capture.lines().count() == 90
                && capture.contains("181h 38m")
                && capture.contains("Claude provider:")
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = capture.lines().collect();
        let start = rows
            .iter()
            .position(|line| line.contains("\"lastMessage\":"))
            .unwrap();
        let code = rows
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, line)| line.contains("Claude 图片链接") && line.contains("181h 38m"))
            .unwrap();
        let prompt = rows
            .iter()
            .enumerate()
            .skip(code.0)
            .find(|(_, line)| {
                line.contains("Implement workstream A") && line.contains("Claude provider:")
            })
            .unwrap();
        assert_eq!(
            code.1.find("Claude 图片链接"),
            prompt.1.find("Implement workstream A"),
            "{capture}"
        );
        assert!(
            rows[code.0 + 1]
                .trim()
                .trim_end_matches('│')
                .trim()
                .is_empty(),
            "no wrapped duration: {capture}"
        );
        let file =
            std::env::temp_dir().join(format!("agents-notification-preformatted-{width}.txt"));
        std::fs::write(&file, &capture).unwrap();
        println!(
            "{width} columns: {}\n{}\n{}",
            file.display(),
            code.1,
            prompt.1
        );
    }
    println!("{{\"notification_preformatted\":\"pass\"}}");
}

#[path = "support/terminal_notification.rs"]
mod terminal_notification;

#[path = "support/terminal_paths.rs"]
mod paths;

#[path = "support/terminal_event_scroll.rs"]
mod terminal_event_scroll;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_manager_detach_result_name() {
    use std::io::Write;
    let (_directory, tmux, transcript) = start_probe("manager-detach-result-name");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap()
        .write_all(include_bytes!("fixtures/claude-manager-detach-name.jsonl"))
        .unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .filter(|line| line.contains("codex-manager detach") && line.contains("模型和new挪位"))
            .count()
            == 2
    });
    let path = std::env::temp_dir().join("agents-detach-result-name.txt");
    std::fs::write(&path, capture).unwrap();
    println!("{}", serde_json::json!({"verdict":"pass", "capture":path}));
}

#[test]
#[ignore = "requires tmux and local Claude account authentication; reads live weekly usage"]
fn terminal_weekly_usage_on_task_header() {
    let (_directory, tmux, _transcript) = start_probe("weekly-usage");
    for width in [80, 200] {
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        let capture = tmux.capture_when(|capture| {
            capture.lines().any(|line| line.width() == width as usize)
                && capture.lines().any(|line| {
                    line.contains('%') && line.contains("Active ") && line.contains("Inactive ")
                })
        });
        let row = capture
            .lines()
            .find(|line| {
                line.contains('%') && line.contains("Active ") && line.contains("Inactive ")
            })
            .unwrap();
        assert!(
            !row.contains("Model:")
                && !row.contains("Weekly")
                && !row.contains("Claude")
                && !row.contains("Codex"),
            "{row}"
        );
        assert!(
            row.contains("Active ") && row.contains("Inactive "),
            "{row}"
        );
        println!(
            "{}",
            serde_json::json!({
                "verdict": "passed", "width": width, "line": row.trim(),
            })
        );
    }
}

#[test]
#[ignore = "requires tmux; runs the real app with isolated preferences"]
fn terminal_context_usage_on_task_details_border() {
    let (_claude_directory, claude, _transcript) = start_probe("claude-context-border");
    let capture = claude.capture_when(|capture| capture.contains("Update"));
    let claude_border = capture.lines().nth(1).unwrap().to_owned();
    assert!(claude_border.contains("╴5% "), "{capture}");
    assert!(claude_border.contains("Inactive"), "{capture}");

    let mut claude_cases = Vec::new();
    for (name, fixture, percent) in [
        (
            "native",
            include_str!("fixtures/claude-context/opus.jsonl"),
            Some(24),
        ),
        (
            "200k",
            include_str!("fixtures/claude-context/old.jsonl"),
            Some(12),
        ),
        (
            "unknown",
            include_str!("fixtures/claude-context/unknown.jsonl"),
            None,
        ),
    ] {
        let contents = format!("{}{fixture}", include_str!("fixtures/transcript.jsonl"));
        let (_directory, tmux, _) =
            start_probe_with_transcript(&format!("claude-context-{name}"), &contents);
        let capture = tmux.capture_when(|capture| {
            capture.lines().nth(1).is_some_and(|border| match percent {
                Some(percent) => border.contains(&format!("╴{percent}% ")),
                None => !border.contains('%'),
            })
        });
        let border = capture.lines().nth(1).unwrap();
        assert!(border.contains("Inactive"), "{capture}");
        if let Some(percent) = percent {
            assert!(percent <= 100);
            assert!(
                border.find('%').unwrap() < border.find("Inactive").unwrap(),
                "{capture}"
            );
        }
        claude_cases.push(serde_json::json!({"case":name,"percent":percent,"border":border}));
    }

    let (_codex_directory, codex) = terminal_codex_exit::start_codex_probe(
        "codex-context-border",
        include_str!("fixtures/codex-latest-activity.jsonl"),
    );
    let capture = codex.capture_when(|capture| {
        capture
            .lines()
            .nth(1)
            .is_some_and(|row| row.contains("╴9% "))
    });
    let codex_border = capture.lines().nth(1).unwrap();
    assert!(codex_border.contains("Inactive"), "{capture}");
    assert!(
        codex_border.find("9%").unwrap() < codex_border.find("Inactive").unwrap(),
        "{capture}"
    );
    let result = serde_json::json!({"verdict":"passed", "claude":claude_border, "claude_cases":claude_cases, "codex":codex_border});
    if let Some(path) = std::env::var_os("AGENTS_CONTEXT_TERMINAL_RESULT") {
        std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    }
    println!("{result}");
}

#[path = "support/terminal_summary.rs"]
mod terminal_summary;

#[path = "support/terminal_await_output.rs"]
mod terminal_await_output;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_duration_sort_jump_position() {
    let mut captures = Vec::new();
    for width in [80, 200] {
        let (_directory, tmux, _transcript) = start_probe("duration-border");
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        let before = tmux.capture_when(|capture| {
            capture
                .lines()
                .any(|row| row.contains("medium") && row.width() == width)
        });
        let rows: Vec<_> = before.lines().collect();
        let (column, top) = if width < 150 {
            (
                1,
                rows.iter().position(|row| row.contains("medium")).unwrap(),
            )
        } else {
            let separator = rows[2].rfind('│').unwrap();
            (rows[2][..separator].width() + 2, 1)
        };
        tmux.mouse(0, column, top + 1, true);
        tmux.run(&["send-keys", "-t", "probe", "t"]);
        let capture = tmux.capture_when(|capture| capture.contains("duration"));
        let border = capture
            .lines()
            .find(|row| row.contains("duration"))
            .unwrap();
        assert!(!border.contains("Sort · "), "{capture}");
        // tmux omits trailing blank cells, including the label's final space.
        assert!(border.ends_with(" duration"), "{capture}");
        assert_eq!(border.width(), width - 1, "{capture}");
        if width < 150 {
            assert!(border.contains("medium"), "{capture}");
        }
        let path = std::env::temp_dir().join(format!("agents-duration-border-{width}.txt"));
        std::fs::write(&path, &capture).unwrap();
        captures.push(path);
    }
    let verdict = serde_json::json!({"verdict": "pass", "captures": captures});
    let path = std::env::temp_dir().join("agents-duration-border-verdict.json");
    std::fs::write(path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{verdict}");
}

#[path = "support/terminal_link_overlays.rs"]
mod terminal_link_overlays;

#[path = "support/terminal_model_families.rs"]
mod terminal_model_families;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_finished_claude_subagents_remain_under_inactive_parent() {
    let (_directory, tmux, transcript) = start_probe("claude-inactive-subagents");
    let children = transcript.with_extension("").join("subagents");
    std::fs::create_dir_all(&children).unwrap();
    let records = include_str!("fixtures/subagent.jsonl");
    std::fs::write(children.join("agent-done.jsonl"), records).unwrap();
    std::fs::write(
        children.join("agent-done.meta.json"),
        r#"{"description":"Completed review"}"#,
    )
    .unwrap();
    std::fs::write(
        children.join("agent-pending.jsonl"),
        records.lines().next().unwrap(),
    )
    .unwrap();
    std::fs::write(
        children.join("agent-pending.meta.json"),
        r#"{"description":"Pending check"}"#,
    )
    .unwrap();
    let inactive = tmux.capture_when(|capture| {
        capture.contains("Inactive 3")
            && capture.contains("Completed review")
            && capture.contains("Pending check")
    });
    for title in ["Completed review", "Pending check"] {
        let row = inactive.lines().find(|line| line.contains(title)).unwrap();
        assert!(row.contains('├') || row.contains('└'), "{inactive}");
    }
    tmux.run(&["send-keys", "-t", "probe", "BTab"]);
    let active = tmux.capture_when(|capture| {
        !capture.contains("Completed review") && !capture.contains("Pending check")
    });
    assert!(!active.contains("Completed review"), "{active}");
    assert!(!active.contains("Pending check"), "{active}");
    let verdict = serde_json::json!({
        "passed": true,
        "inactive": inactive,
        "active": active,
        "finished_child_nested": true,
        "dead_parent_pending_child_nested": true,
        "active_excludes_children": true,
    });
    if let Some(path) = std::env::var_os("AGENTS_SUBAGENTS_VERDICT") {
        std::fs::write(path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    }
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_inactive_elapsed_stays_at_last_record() {
    let (_directory, tmux, _transcript) = start_probe("inactive-elapsed");
    let header = |capture: &str| {
        capture
            .lines()
            .find(|line| line.contains("Inactive") && line.contains('─'))
            .expect("inactive details header")
            .trim_end()
            .to_owned()
    };
    let first = tmux.capture_when(|capture| {
        capture
            .lines()
            .any(|line| line.contains("Inactive") && line.contains("16s"))
    });
    std::thread::sleep(Duration::from_secs(2));
    // Force a redraw after wall time advances, without changing the session.
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    let second = tmux.capture_when(|capture| {
        capture
            .lines()
            .any(|line| line.contains("Inactive") && line.contains("16s"))
    });
    assert_eq!(header(&first), header(&second));
    std::fs::write(
        std::env::temp_dir().join("agents-inactive-elapsed-verdict.json"),
        serde_json::json!({"inactive_elapsed": "pass", "elapsed": "16s", "redraw_stable": true})
            .to_string(),
    )
    .unwrap();
}

#[path = "support/terminal_window_navigation.rs"]
mod terminal_window_navigation;
