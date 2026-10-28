#[path = "support/terminal_handback.rs"]
mod terminal_handback;

#[path = "support/terminal_path_display.rs"]
mod terminal_path_display;

#[path = "support/terminal_weekly_usage.rs"]
mod terminal_weekly_usage;

#[path = "support/terminal_message_text.rs"]
mod terminal_message_text;


use std::path::Path;
use std::process::Command;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;


#[path = "support/terminal_mcp.rs"]
mod terminal_mcp;


#[path = "support/terminal_event_detail.rs"]
mod terminal_event_detail;
use terminal_event_detail::event_detail_rows;

#[path = "support/terminal_scripts.rs"]
mod terminal_scripts;

#[path = "support/terminal_heredoc_format.rs"]
mod terminal_heredoc_format;

#[path = "support/terminal_drafts.rs"]
mod terminal_drafts;

#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;

impl Tmux {
    fn wait_for(&self, ready: impl Fn(&str) -> bool) {
        self.capture_when(ready);
    }

    fn capture_when(&self, ready: impl Fn(&str) -> bool) -> String {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
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

fn wait_for_size(tmux: &Tmux, width: u16, height: u16) -> String {
    tmux.capture_when(|capture| {
        capture.lines().count() == usize::from(height)
            && capture.lines().nth(1).is_some_and(|line| line.width() == usize::from(width))
            && capture.lines().nth(2).is_some_and(|line| {
                line.matches('│').count() == if width >= 150 { 2 } else { 1 }
            })
    })
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
    launch_probe(name, contents, setup, command_prefix, true)
}

fn launch_probe(
    name: &str,
    contents: &str,
    setup: impl FnOnce(&Path),
    command_prefix: &[&str],
    select_task: bool,
) -> (tempfile::TempDir, Tmux, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    // The default fixture's project path must exist when a probe creates a draft.
    // Keep explicitly supplied transcripts and repository paths intact.
    let contents = if contents == include_str!("fixtures/transcript.jsonl") {
        contents
            .lines()
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                if record.get("cwd").is_some() {
                    record["cwd"] = directory.path().to_str().unwrap().into();
                }
                record.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        contents.to_owned()
    };
    std::fs::write(&transcript, contents).unwrap();
    std::fs::create_dir(directory.path().join("clipboard")).unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    setup(directory.path());
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-{name}-{}", std::process::id()));
    tmux.run(
        &[
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
    if select_task {
        tmux.run(&["send-keys", "-t", "probe", "Tab"]);
        tmux.wait_for(|capture| capture.contains("Update") || capture.contains("Last activity"));
    }
    (directory, tmux, transcript)
}

#[test]
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
        wait_for_size(&tmux, width, 50);
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
        .take_while(|line| {
            separators.iter().all(|column| {
                line.match_indices('│')
                    .any(|(index, _)| line[..index].width() == *column)
            })
        })
        .count();
    let task_details_start = separators[0];
    let task_details_end = if width >= 150 {
        separators[1] + 1
    } else {
        width as usize
    };
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, width, height);
    let edge = |y| {
        (task_details_start..task_details_end)
            .map(|x| parser.screen().cell(y, x as u16).unwrap().contents())
            .collect::<String>()
    };
    let top = edge(1);
    let status = " Inactive ⏸\u{fe0e} ";
    let status_start = top.find(status).expect("status on the top rule");
    assert_eq!(
        top[..status_start].width(),
        (task_details_end - task_details_start - status.width()) / 2,
        "{capture}"
    );
    for x in task_details_start + top[..status_start].width() + 1
        ..task_details_start + top[..status_start].width() + status.width() - 1
    {
        let cell = parser.screen().cell(1, x as u16).unwrap();
        assert!(cell.bold(), "{colored}");
        assert!(!cell.dim(), "{colored}");
        assert_eq!(cell.fgcolor(), vt100::Color::Default);
    }
    let elapsed = " 16s╶";
    assert!(
        top.ends_with(&format!(
            "{elapsed}{}",
            if width >= 150 { '┬' } else { '─' }
        )),
        "{capture}"
    );
    let elapsed_start = task_details_end - 1 - elapsed.width();
    for x in elapsed_start + 1..task_details_end - 2 {
        let cell = parser.screen().cell(1, x as u16).unwrap();
        assert!(cell.dim(), "{colored}");
        assert!(!cell.bold(), "{colored}");
    }

    let bottom = edge(top_end as u16);
    let title = match width {
        150 | 151 => " Codex manager 停止时… ",
        _ => " Codex manager 停止时线程处理 ",
    };
    let title_start = bottom.find(title).expect("task name on the bottom rule");
    assert_eq!(
        bottom[..title_start].width(),
        (task_details_end - task_details_start - title.width()) / 2,
        "{capture}"
    );
    assert!(top.starts_with('┬'), "{capture}");
    let folder = match width {
        150 | 151 => "┴╴cla… ",
        _ => "┴╴claude ",
    };
    assert!(bottom.starts_with(folder), "{capture}");
    assert!(
        bottom[folder.len()..title_start].chars().all(|c| c == '─'),
        "{capture}"
    );
    let branch_start = bottom.rfind(" main╶").expect("branch on the bottom rule");
    assert!(
        bottom[title_start + title.len()..branch_start]
            .chars()
            .all(|c| c == '─'),
        "{capture}"
    );
    assert!(
        bottom.ends_with(if width >= 150 {
            " main╶┴"
        } else {
            " main╶─"
        }),
        "{capture}"
    );
    for x in task_details_start + bottom[..title_start].width() + 1
        ..task_details_start + bottom[..title_start].width() + title.width() - 1
    {
        let cell = parser.screen().cell(top_end as u16, x as u16).unwrap();
        if !cell.contents().is_empty() {
            assert!(cell.bold(), "{colored}");
            assert!(!cell.dim(), "{colored}");
        }
    }
    println!("{{\"task_details_borders\":\"pass\",\"width\":{width}}}");
}

// Runs the executable's terminal loop, provider discovery and event loading, rather
// than constructing a View. AGENTS_PROBE_BIN also checks the installed artifact.
#[test]
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
        wait_for_size(&tmux, width, height);
        let capture = tmux.capture_when(|capture| {
            let rows: Vec<_> = capture.lines().collect();
            let Some(header) = rows.iter().rposition(|line| line.contains("Assistant")) else { return false; };
            if header + 3 >= rows.len() { return false; }
            if width >= 150 {
                rows[header + 3..rows.len() - 1].iter().any(|line| line.width() > width as usize * 2 / 3)
            } else {
                rows[header + 1..].join("\n").contains("decision")
            }
        });
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
            lines[1].contains(" Inactive ⏸\u{fe0e} "),
            "status must stay on the top rule: {capture}"
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
            .take_while(|line| {
                separators.iter().all(|column| {
                    line.match_indices('│')
                        .any(|(index, _)| line[..index].width() == *column)
                })
            })
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
                let event_panel = line.splitn(3, '│').nth(2).unwrap();
                if !event_panel.trim().is_empty() {
                    assert_eq!(
                        event_panel.len() - event_panel.trim_start().len(),
                        1,
                        "log left: {line}"
                    );
                    assert_eq!(
                        line.trim_end_matches([' ', '┐', '│', '┘']).width(),
                        width as usize - 2,
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
                task_details.contains(match width {
                    150 | 151 => " Codex manager 停止时… ",
                    _ => " Codex manager 停止时线程处理 ",
                }),
                "task name missing from middle rule: {capture}"
            );
            for line in &lines[2..top_end] {
                let event_panel = line.splitn(3, '│').nth(2).unwrap();
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
                    line.splitn(3, '│')
                        .nth(2)
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
            let opened = tmux.capture_when(|capture| capture.lines().skip(event_rule + 1).any(|line| line.contains("codex thread 01a11210-f266-7182-b202-269b6b4fe157")));
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
                    .contains("codex thread 01a11210-f266-7182-b202-269b6b4fe157"),
                "clicked user event missing from detail: {opened}"
            );
            // Search follows the last mouse panel, independently of the selected event.
            tmux.mouse(0, 1, event_rule + 1, true);
            tmux.run(&["send-keys", "-t", "probe", "/"]);
            tmux.run(&["send-keys", "-t", "probe", "-l", "await"]);
            let searched = tmux.capture_when(|capture| capture.lines().skip(event_rule + 1).any(|line| line.contains("Search ❯ await")));
            assert!(
                searched
                    .lines()
                    .skip(event_rule + 1)
                    .any(|line| line.contains("Search ❯ await")),
                "detail search missing: {searched}"
            );
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
            // Select the real newest event so each resize starts with the same detail.
            tmux.wait_for(|capture| !capture.contains("Search ❯"));
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
            tmux.wait_for(|capture| capture.lines().skip(event_rule + 1).any(|line| line.contains("decision")));
        }
    }
}

#[test]
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
    call["message"]["content"][0]["input"] = serde_json::json!({"command": "sleep 2"});
    let mut tool = Command::new("sleep").arg("2").spawn().unwrap();
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(transcript)
            .unwrap(),
        "{call}"
    )
    .unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let frames = ["•··", "·•·", "··•"];
    tmux.wait_for(|capture| frames.iter().any(|frame| capture.contains(frame)));
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
fn terminal_task_details_section_positions() {
    let (_directory, tmux, transcript) =
        background_text_probe("details-sections", "Running checks\n");
    let background = include_str!("fixtures/background/records.jsonl");
    let original = format!(
        "{}\n{background}\n{}",
        std::fs::read_to_string(&transcript).unwrap(),
        include_str!("fixtures/claude-todos.jsonl")
    );
    let checkout = _directory.path().join("pr-origin/claude");
    std::fs::create_dir_all(&checkout).unwrap();
    for args in [
        vec!["init"],
        vec![
            "remote",
            "add",
            "origin",
            "https://github.com/example-user/panel.git",
        ],
    ] {
        assert!(
            Command::new("git")
                .current_dir(&checkout)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let with_cwd = |records: &str| {
        records
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                if record.get("cwd").is_some() {
                    record["cwd"] = serde_json::json!(checkout);
                }
                record.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let original = with_cwd(&original);
    let pr = with_cwd(include_str!("fixtures/claude-pr-create/foreground.jsonl"));
    for has_pr in [false, true, false] {
        let content = if has_pr {
            format!("{original}\n{pr}\n")
        } else {
            original.clone()
        };
        std::fs::write(&transcript, content).unwrap();
        tmux.wait_for(|capture| {
            capture.contains("#1") == has_pr
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
            let capture = wait_for_size(&tmux, width, 70);
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
            let parser = captured_screen(&capture, width, 70);
            let bottom = (2..lines.len())
                .find(|&y| {
                    parser
                        .screen()
                        .cell(y as u16, left as u16)
                        .unwrap()
                        .contents()
                        == "┴"
                })
                .unwrap();
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
                .map(|x| parser.screen().cell(1, x as u16).unwrap().contents())
                .collect();
            assert!(status.contains(" Inactive "), "{status}");
            assert!(!status.contains("main"), "{status}");
            assert!(!status.contains('#'), "{status}");
            let title: String = (left..right)
                .map(|x| {
                    parser
                        .screen()
                        .cell(bottom as u16, x as u16)
                        .unwrap()
                        .contents()
                })
                .collect();
            assert!(title.starts_with("┴╴claude "), "{title}");
            let end = if width >= 150 { "╶┴" } else { "╶─" };
            if has_pr {
                assert!(title.ends_with(&format!(" #1{end}")), "{title}");
                assert!(!title.contains("main"), "{title}");
            } else {
                assert!(title.ends_with(&format!(" main{end}")), "{title}");
                assert!(!title.contains('#'), "{title}");
            }
            let model: String = (0..=left)
                .map(|x| {
                    parser
                        .screen()
                        .cell(bottom as u16, x as u16)
                        .unwrap()
                        .contents()
                })
                .collect();
            assert!(model.starts_with("─╴claude-opus-5-5 "), "{model}");
            assert!(model.ends_with(" medium╶┴"), "{model}");
            assert!(!model.contains("Model"));
            let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
            let colored = captured_screen(&colored, width, 70);
            for (start, length) in [
                (2, "claude-opus-5-5".len()),
                (
                    model[..model.find("medium").unwrap()].width(),
                    "medium".len(),
                ),
            ] {
                for x in start..start + length {
                    let cell = colored.screen().cell(bottom as u16, x as u16).unwrap();
                    assert!(cell.dim(), "{capture}");
                    assert!(!cell.bold(), "{capture}");
                }
            }
            println!(
                "{{\"task_details_sections\":\"pass\",\"folder_on_title_rule\":\"pass\",\"model_on_chart_rule\":\"pass\",\"metadata_dim\":\"pass\",\"width\":{width},\"pr\":{has_pr}}}"
            );
        }
    }
}

#[path = "support/terminal_codex_exit.rs"]
mod terminal_codex_exit;

#[path = "support/terminal_event_detail_width.rs"]
mod terminal_event_detail_width;

#[path = "support/terminal_paths.rs"]
mod paths;

#[path = "support/terminal_event_scroll.rs"]
mod terminal_event_scroll;


#[path = "support/terminal_model_families.rs"]
mod terminal_model_families;

#[test]
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
