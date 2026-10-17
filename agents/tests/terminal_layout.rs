use std::path::Path;
use std::process::Command;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

#[path = "support/terminal_questions.rs"]
mod terminal_questions;

#[path = "support/terminal_headings.rs"]
mod terminal_headings;

#[path = "support/terminal_scripts.rs"]
mod terminal_scripts;

#[path = "support/terminal_disabled_prompt.rs"]
mod terminal_disabled_prompt;

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
    let result = start_probe_with_transcript(name, include_str!("fixtures/transcript.jsonl"));
    result.1.wait_for(|capture| capture.contains("Stop hook"));
    result
}

fn start_probe_with_transcript(
    name: &str,
    contents: &str,
) -> (tempfile::TempDir, Tmux, std::path::PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let transcript = project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl");
    std::fs::write(&transcript, contents).unwrap();
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
    tmux.wait_for(|capture| capture.contains("Update"));
    (directory, tmux, transcript)
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_prompt_newline_entry() {
    let (_directory, tmux, _transcript) = start_probe("prompt-newlines");
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    tmux.wait_for(|capture| capture.contains("New task ❯"));
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
                pair[0].contains("New task ❯ first line") && pair[1].trim() == "second line"
            })
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let path = std::env::temp_dir().join(format!("agents-prompt-{name}.txt"));
        std::fs::write(&path, capture).unwrap();
        println!(
            "{{\"prompt_newline\":\"pass\",\"entry\":\"{name}\",\"capture\":\"{}\"}}",
            path.display()
        );
        tmux.run(&["send-keys", "-t", "probe", "C-c"]);
        tmux.wait_for(|capture| capture.contains("New task ❯") && !capture.contains("first line"));
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
            let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            let (row, line) = capture
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains(needle))
                .unwrap();
            let column = line[..line.find(needle).unwrap()].width() + offset;
            tmux.wait_for(|_| {
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
        tmux.wait_for(|capture| capture.contains("Rename ❯ "));
        tmux.run(&["send-keys", "-t", "probe", "Home"]);
        assert_caret("Rename ❯ ", 9);
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    }
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    tmux.wait_for(|capture| capture.contains("New task ❯"));
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .last()
            .is_some_and(|line| line.contains("g group by"))
    });
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("New task ❯"))
        .unwrap();
    let marker = line[..line.find('❯').unwrap()].width();
    tmux.mouse(0, marker, row, true);
    tmux.run(&["send-keys", "-t", "probe", "-l", "你好"]);
    tmux.wait_for(|capture| capture.contains("New task ❯ 你好"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let row = capture
        .lines()
        .position(|line| line.contains("New task ❯ 你好"))
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
    let call = include_str!("fixtures/claude-tool-calls.jsonl")
        .lines()
        .next()
        .unwrap();
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
fn terminal_leaving_list_search_preserves_detail() {
    for mouse_exit in [false, true] {
        let (_directory, tmux, _transcript) = start_probe(if mouse_exit {
            "search-exit-mouse"
        } else {
            "search-exit-escape"
        });
        tmux.run(&["resize-window", "-t", "probe", "-x", "120", "-y", "50"]);
        tmux.wait_for(|capture| capture.lines().any(|line| line.matches('─').count() > 100));
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
                lines[top_end + 1].contains('❯'),
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
            assert!(opened_lines[log_rule].contains("↓ Latest"), "{opened}");
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
                    .any(|line| line.contains("Search ❯ await")),
                "detail search missing: {searched}"
            );
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
            // Select the real newest event so each resize starts with the same detail.
            std::thread::sleep(Duration::from_millis(200));
            let assistant_row = (prompt + 2..log_rule)
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
        assert!(older.contains("↓ Latest"), "{older}");
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
        let list: String = pinned.lines().skip(row).take(if width >= 150 {
            prompt - row - 1
        } else {
            pinned.lines().enumerate().filter_map(|(index, line)| (line.matches('─').count() > width / 2).then_some(index)).last().unwrap() - row
        }).map(|line| line.split('│').next_back().unwrap()).collect::<Vec<_>>().join("\n");
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
            let positions = ["Tasks", "Prompt", "Update"]
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
fn terminal_log_latest_uses_list_bottom_border() {
    for width in [80, 200] {
        let (_directory, tmux, _transcript) = start_probe("log-latest-border");
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
        tmux.wait_for(|capture| capture.contains("↓ Latest"));
        let older = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let rows: Vec<_> = older.lines().collect();
        assert!(
            rows[bottom].contains("↓ Latest"),
            "Latest missing from bottom border: {older}"
        );
        assert!(
            !rows[top].contains("Latest"),
            "Latest on top border: {older}"
        );
        assert!(
            (rows[model].contains("claude-opus-5-5") || rows[model].contains("Opus 5.5"))
                && rows[model].contains("medium"),
            "{older}"
        );
        let byte = rows[bottom].find("↓ Latest").unwrap();
        let jump = rows[bottom][..byte].width();
        assert_eq!(jump - 1, column + (width - column - 1 - 10 - 10) / 2);
        if width < 150 {
            tmux.mouse(65, 2, bottom + 3, false);
            tmux.wait_for(|capture| capture.contains("↑ Top"));
            let scrolled = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            let (top_row, top_line) = scrolled
                .lines()
                .enumerate()
                .find(|(_, row)| row.contains("↑ Top"))
                .unwrap();
            assert!(
                scrolled.lines().nth(bottom).unwrap().contains("↓ Latest"),
                "{scrolled}"
            );
            let top_column = top_line[..top_line.find("↑ Top").unwrap()].width();
            tmux.mouse(0, top_column, top_row, true);
            tmux.wait_for(|capture| !capture.contains("↑ Top") && capture.contains("↓ Latest"));
        }
        tmux.mouse(0, jump, bottom, true);
        tmux.wait_for(|capture| !capture.contains("↓ Latest") && capture.contains("decision"));
        let double_click = |column: usize, row: usize| {
            let column = column + 1;
            let row = row + 1;
            let sequence = format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").repeat(2);
            tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
        };
        let border_column = column + (width - column) / 3;
        double_click(border_column, top);
        tmux.wait_for(|capture| {
            capture.contains("↓ Latest") && capture.contains("run_in_background")
        });
        double_click(border_column, bottom);
        tmux.wait_for(|capture| !capture.contains("↓ Latest") && capture.contains("decision"));
        tmux.mouse(64, column, top + 1, false);
        tmux.wait_for(|capture| capture.contains("↓ Latest"));
        double_click(jump, bottom);
        tmux.wait_for(|capture| !capture.contains("↓ Latest") && capture.contains("decision"));
        let path = std::env::temp_dir().join(format!("agents-latest-border-{width}x50.txt"));
        std::fs::write(&path, older).unwrap();
        println!(
            "{{\"latest_bottom_border\":\"pass\",\"width\":{width},\"capture\":\"{}\"}}",
            path.display()
        );
    }
}

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
    double_click(2);
    tmux.wait_for(|capture| capture.contains("↓ Latest"));
    let oldest = capture("oldest");
    assert!(oldest.contains("Codex Manager 在一个 turn"), "{oldest}");
    double_click(bottom + 1);
    tmux.wait_for(|capture| !capture.contains("↓ Latest") && capture.contains("对，必须"));
    let latest = capture("latest");
    assert!(latest.contains("对，必须"));
    println!("{{\"log_border_double_clicks\":\"pass\",\"width\":200,\"height\":45}}");
}

#[test]
#[ignore = "requires tmux and macOS clipboard; run with --ignored --nocapture"]
fn terminal_detail_right_click_and_double_click_copy() {
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
    // The helper waits 200 ms, so also wait past the configured double-click window.
    tmux.mouse(0, column, row, true);
    std::thread::sleep(Duration::from_millis(350));
    let after_left = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(
        !after_left.contains("\"apiBlockIndex\""),
        "left click changed mode: {after_left}"
    );
    tmux.mouse(2, column, row, true);
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
    tmux.mouse(2, column, row, true);
    tmux.wait_for(|capture| !capture.contains("\"apiBlockIndex\""));
    let parsed = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (column, row) = parsed
        .lines()
        .enumerate()
        .find_map(|(row, line)| {
            let byte = line.rfind("对，必须")?;
            (row > 25).then_some((line[..byte].width(), row))
        })
        .expect("parsed body after right-click round trip");
    let sequence = format!(
        "\x1b[<0;{};{}M\x1b[<0;{};{}m",
        column + 1,
        row + 1,
        column + 1,
        row + 1
    )
    .repeat(2);
    tmux.run(&["send-keys", "-t", "probe", "-l", &sequence]);
    tmux.wait_for(|capture| capture.contains("Copied assistant message"));
    println!(
        "{{\"raw_transcript_copy\":\"pass\",\"note\":\"Copied transcript\",\"width\":200,\"height\":45,\"pbpaste\":\"pretty JSON matches\"}}"
    );
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
#[ignore = "requires tmux and macOS clipboard; run with --ignored --nocapture"]
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
            "怎么判断命令后面",
            true,
        ),
        (
            "agent-prompt",
            format!("{}\n", agents[2]),
            "You are the fork",
            true,
        ),
        (
            "tool-prompt",
            format!("{}\n", calls[16]),
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
        let (_directory, tmux, transcript) = start_probe(&format!("reading-{kind}"));
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
            let expected_left = if prose && width >= 150 {
                50
            } else if matches!(kind, "compaction" | "queued") {
                // Existing narrow mode centers a short body inside the full-width column.
                1 + (usize::from(width) - 2 - line.trim().width()) / 2
            } else {
                1
            };
            if kind == "command" {
                assert!(
                    left < usize::from(width / 4),
                    "command acquired a reading margin: {capture}"
                );
                assert!(
                    rows.iter()
                        .skip(row)
                        .any(|line| line.trim().width() > usize::from(width / 2)),
                    "command wrapped at prose width: {capture}"
                );
            } else if kind == "agent-prompt" && width < 150 {
                // Its existing group centers at the widest full-width wrapped row.
                assert!(left <= 3, "agent prompt unexpectedly narrowed: {capture}");
                assert!(
                    rows.iter().skip(row).any(|line| line.trim().width() > 100),
                    "agent prompt did not use the full narrow column: {capture}"
                );
            } else if kind != "output" {
                assert_eq!(left, expected_left, "{kind} at {width}: {capture}");
            } else {
                let recorded: serde_json::Value = serde_json::from_str(calls[3]).unwrap();
                let content = recorded["toolUseResult"]["file"]["content"]
                    .as_str()
                    .unwrap();
                let widest = content.lines().map(UnicodeWidthStr::width).max().unwrap();
                let padding = (usize::from(width) - 2 - widest) / 2;
                assert_eq!(
                    left,
                    1 + padding + "set -g ".width(),
                    "output layout changed: {capture}"
                );
                assert!(
                    capture.contains(content.lines().max_by_key(|line| line.width()).unwrap()),
                    "output wrapped at prose width: {capture}"
                );
            }
            if prose && width >= 150 {
                for line in rows
                    .iter()
                    .skip(row)
                    .take_while(|line| !line.trim().is_empty())
                {
                    assert!(
                        line.trim_end().trim_end_matches('│').trim_end().width() <= 149,
                        "prose exceeds reading column: {capture}"
                    );
                }
            }
            if kind == "send" {
                assert!(
                    rows[rule + 1].starts_with(" thread 00000000-0000-4000-8000-000000000000"),
                    "identifier missing from top-left header: {capture}"
                );
                assert!(
                    !rows
                        .iter()
                        .skip(rule + 2)
                        .any(|line| line.contains("threadId")),
                    "identifier retained in field list: {capture}"
                );
            }
            if kind == "tool-prompt" {
                assert!(
                    rows[rule + 1].starts_with(" Claude 图片链接"),
                    "name missing from top-left header: {capture}"
                );
                let cwd = rows
                    .iter()
                    .skip(rule + 2)
                    .find(|line| line.contains("cwd"))
                    .expect("remaining cwd field");
                assert_eq!(
                    cwd.len() - cwd.trim_start().len(),
                    expected_left,
                    "cwd outside reading column: {capture}"
                );
            }
            if kind == "assistant" {
                // Drag in screen coordinates to the next character's boundary.
                tmux.mouse(0, left, row, false);
                tmux.mouse(32, left + "对，必须".width(), row, false);
                let release = format!("\x1b[<0;{};{}m", left + "对，必须".width() + 1, row + 1);
                tmux.run(&["send-keys", "-t", "probe", "-l", &release]);
                tmux.wait_for(|capture| capture.contains("Copied"));
                let selected = Command::new("pbpaste").output().unwrap();
                assert!(selected.status.success());
                assert_eq!(String::from_utf8(selected.stdout).unwrap(), "对，必须");
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
                tmux.wait_for(|capture| capture.contains("Copied assistant message"));
                let copied = Command::new("pbpaste").output().unwrap();
                assert!(copied.status.success());
                let source: serde_json::Value = serde_json::from_str(
                    include_str!("fixtures/transcript.jsonl")
                        .lines()
                        .nth(1)
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(
                    String::from_utf8(copied.stdout).unwrap(),
                    source["message"]["content"][0]["text"].as_str().unwrap()
                );
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
