use super::*;

#[test]
fn terminal_bash_interpreter_script_colors() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("interpreter-script");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    let fixture = include_str!("../fixtures/claude-inline-script.jsonl");
    let event: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let command = event["message"]["content"][0]["input"]["command"]
        .as_str()
        .unwrap();
    // Exact Bash event from a Claude transcript; commands are displayed, never executed.
    write!(file, "{fixture}").unwrap();
    file.flush().unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.wait_for(|capture| capture.contains("python3") && capture.contains("enumerate(lines)"));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let path = std::env::temp_dir().join("agents-interpreter-script-200x50.ansi");
    std::fs::write(&path, &colored).unwrap();
    let parser = captured_screen(&colored, 200, 50);
    let screen = parser.screen();
    let cell = |needle: &str| {
        let (row, column) = capture
            .lines()
            .enumerate()
            .find_map(|(row, line)| line.find(needle).map(|byte| (row, line[..byte].width())))
            .unwrap_or_else(|| panic!("missing {needle:?}: {capture}"));
        screen.cell(row as u16, column as u16).unwrap()
    };
    let keyword = cell("for i,l").fgcolor();
    let variable = cell("lines=open").fgcolor();
    let string = cell("tests/ui_snapshots").fgcolor();
    let builtin = cell("enumerate(lines)");
    let shell_command = cell("cd ").fgcolor();
    let shell_path = cell(command.split_whitespace().nth(1).unwrap()).fgcolor();
    // The bundled theme leaves Python control keywords at the default foreground.
    assert_eq!(keyword, variable, "Python keyword style changed: {capture}");
    assert!(builtin.italic(), "Python builtin style missing: {capture}");
    assert_ne!(string, variable, "Python string stayed plain: {capture}");
    assert_ne!(keyword, string, "Python syntax colors collapsed: {capture}");
    assert_ne!(
        shell_command, shell_path,
        "shell syntax colors lost: {capture}"
    );
    println!(
        "{{\"interpreter_script_colors\":\"pass\",\"capture\":\"{}\",\"keyword\":\"{keyword:?}\",\"string\":\"{string:?}\",\"shell_command\":\"{shell_command:?}\"}}",
        path.display()
    );
}

#[test]
fn terminal_write_file_colors() {
    use std::io::Write;

    let (_directory, tmux, transcript) = start_probe("write-file");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(transcript)
        .unwrap();
    file.write_all(include_bytes!("../fixtures/claude-write/python.jsonl"))
        .unwrap();
    file.flush().unwrap();
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.wait_for(|capture| capture.contains("import json, sys") && capture.contains("\"user\""));
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    let color = |needle: &str| {
        let (row, column) = capture
            .lines()
            .enumerate()
            .find_map(|(row, line)| line.find(needle).map(|byte| (row, line[..byte].width())))
            .unwrap_or_else(|| panic!("missing {needle:?}: {capture}"));
        parser
            .screen()
            .cell(row as u16, column as u16)
            .unwrap()
            .fgcolor()
    };
    assert_ne!(color("\"user\""), color("import json"), "{capture}");
    let path = std::env::temp_dir().join("agents-write-file-200x50.ansi");
    std::fs::write(&path, colored).unwrap();
    println!(
        "{{\"write_file_colors\":\"pass\",\"capture\":\"{}\"}}",
        path.display()
    );
}

fn shell_file_read_colors(provider: &str, index: usize) {
    let (case, command, highlighted) = [
        ("range", "sed -n '10,80p' src/foo.rs", true),
        ("unquoted", "sed -n 1,140p src/sessions/mod.rs", true),
        (
            "ranges",
            "cd /x/agents/src/ui && sed -n '690,730p;790,815p;885,905p' render.rs",
            true,
        ),
        (
            "pipeline-ranges",
            "cat a.rs | sed -n '15,25p;125,140p;555,575p'",
            true,
        ),
        (
            "regex-quit",
            r"sed -n '/#\[cfg(test)\]/q;p' src/rename/walker.rs",
            true,
        ),
        (
            "arithmetic",
            r#"sed -n "$((n-12)),$((n+12))p" claude.strings"#,
            false,
        ),
        ("variable", r#"sed -n "${n}p" claude.strings"#, false),
        (
            "arithmetic-rust",
            r#"sed -n "$((n-12)),$((n+12))p" a.rs"#,
            true,
        ),
        ("variable-rust", r#"sed -n "${n}p" a.rs"#, true),
        ("expressions", "sed -e '1,3p' -e '9p' -n a.rs", true),
        ("count", "sed -n '1,3p;60,90p' a.rs", true),
        ("grep", "sed -n 1,5p a.rs | grep x", false),
        ("substitute", "sed 's/ *$//' lr.out", false),
        ("number", "sed -n '1,5p;=' f.rs", false),
        ("script-file", "sed -f script.sed f.rs", false),
        ("append", r"sed '1a\foo' f.rs", false),
    ][index];
    let body = "let answer = \"file-read-highlight\";";
    let name = format!("file-read-{provider}-{case}");
    let (_directory, tmux) = start_shell_output_probe(&name, command, body, provider);
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "50"]);
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let capture = tmux.capture_when(|capture| capture.contains(body));
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 200, 50);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(body))
        .unwrap();
    let color = |needle| {
        parser
            .screen()
            .cell(
                row as u16,
                line[..line.find(needle).unwrap()].width() as u16,
            )
            .unwrap()
            .fgcolor()
    };
    assert_eq!(
        color("answer") != color("file-read-highlight"),
        highlighted,
        "{provider} {case} ({command}): {capture}"
    );
    let path = std::env::temp_dir().join(format!("agents-{name}.ansi"));
    std::fs::write(&path, colored).unwrap();
    let verdict = serde_json::json!({"verdict": "pass", "provider": provider, "highlighted": highlighted, "capture": path});
    std::fs::write(path.with_extension("json"), verdict.to_string()).unwrap();
    println!("{verdict}");
}

fn shell_file_read_markdown(provider: &str, index: usize) {
    let (case, command) = [
        (
            "regex-range",
            "sed -n '/## Install/,/^## /p' memory/agents.md",
        ),
        ("delete", "sed '/^$/d' notes.md"),
    ][index];
    let name = format!("file-read-{provider}-{case}");
    let (_directory, tmux) =
        start_shell_output_probe(&name, command, "**markdown-probe**", provider);
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let capture = tmux.capture_when(|capture| capture.contains("markdown-probe"));
    assert!(
        !capture.contains("**markdown-probe**"),
        "{provider} {case}: Markdown markup stayed literal: {capture}"
    );
    let path = std::env::temp_dir().join(format!("agents-{name}.ansi"));
    std::fs::write(
        &path,
        tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]),
    )
    .unwrap();
    let verdict = serde_json::json!({"verdict": "pass", "provider": provider, "markdown": true, "capture": path});
    std::fs::write(path.with_extension("json"), verdict.to_string()).unwrap();
    println!("{verdict}");
}

#[test]
fn terminal_shell_file_read_colors_claude_range() {
    shell_file_read_colors("claude", 0);
}

#[test]
fn terminal_shell_file_read_colors_claude_unquoted() {
    shell_file_read_colors("claude", 1);
}

#[test]
fn terminal_shell_file_read_colors_claude_ranges() {
    shell_file_read_colors("claude", 2);
}

#[test]
fn terminal_shell_file_read_colors_claude_pipeline_ranges() {
    shell_file_read_colors("claude", 3);
}

#[test]
fn terminal_shell_file_read_colors_claude_regex_quit() {
    shell_file_read_colors("claude", 4);
}

#[test]
fn terminal_shell_file_read_colors_claude_arithmetic() {
    shell_file_read_colors("claude", 5);
}

#[test]
fn terminal_shell_file_read_colors_claude_variable() {
    shell_file_read_colors("claude", 6);
}

#[test]
fn terminal_shell_file_read_colors_claude_arithmetic_rust() {
    shell_file_read_colors("claude", 7);
}

#[test]
fn terminal_shell_file_read_colors_claude_variable_rust() {
    shell_file_read_colors("claude", 8);
}

#[test]
fn terminal_shell_file_read_colors_claude_expressions() {
    shell_file_read_colors("claude", 9);
}

#[test]
fn terminal_shell_file_read_colors_claude_count() {
    shell_file_read_colors("claude", 10);
}

#[test]
fn terminal_shell_file_read_colors_claude_grep() {
    shell_file_read_colors("claude", 11);
}

#[test]
fn terminal_shell_file_read_colors_claude_substitute() {
    shell_file_read_colors("claude", 12);
}

#[test]
fn terminal_shell_file_read_colors_claude_number() {
    shell_file_read_colors("claude", 13);
}

#[test]
fn terminal_shell_file_read_colors_claude_script_file() {
    shell_file_read_colors("claude", 14);
}

#[test]
fn terminal_shell_file_read_colors_claude_append() {
    shell_file_read_colors("claude", 15);
}

#[test]
fn terminal_shell_file_read_colors_claude_regex_range() {
    shell_file_read_markdown("claude", 0);
}

#[test]
fn terminal_shell_file_read_colors_claude_delete() {
    shell_file_read_markdown("claude", 1);
}

#[test]
fn terminal_shell_file_read_colors_codex_range() {
    shell_file_read_colors("codex", 0);
}

#[test]
fn terminal_shell_file_read_colors_codex_unquoted() {
    shell_file_read_colors("codex", 1);
}

#[test]
fn terminal_shell_file_read_colors_codex_ranges() {
    shell_file_read_colors("codex", 2);
}

#[test]
fn terminal_shell_file_read_colors_codex_pipeline_ranges() {
    shell_file_read_colors("codex", 3);
}

#[test]
fn terminal_shell_file_read_colors_codex_regex_quit() {
    shell_file_read_colors("codex", 4);
}

#[test]
fn terminal_shell_file_read_colors_codex_arithmetic() {
    shell_file_read_colors("codex", 5);
}

#[test]
fn terminal_shell_file_read_colors_codex_variable() {
    shell_file_read_colors("codex", 6);
}

#[test]
fn terminal_shell_file_read_colors_codex_arithmetic_rust() {
    shell_file_read_colors("codex", 7);
}

#[test]
fn terminal_shell_file_read_colors_codex_variable_rust() {
    shell_file_read_colors("codex", 8);
}

#[test]
fn terminal_shell_file_read_colors_codex_expressions() {
    shell_file_read_colors("codex", 9);
}

#[test]
fn terminal_shell_file_read_colors_codex_count() {
    shell_file_read_colors("codex", 10);
}

#[test]
fn terminal_shell_file_read_colors_codex_grep() {
    shell_file_read_colors("codex", 11);
}

#[test]
fn terminal_shell_file_read_colors_codex_substitute() {
    shell_file_read_colors("codex", 12);
}

#[test]
fn terminal_shell_file_read_colors_codex_number() {
    shell_file_read_colors("codex", 13);
}

#[test]
fn terminal_shell_file_read_colors_codex_script_file() {
    shell_file_read_colors("codex", 14);
}

#[test]
fn terminal_shell_file_read_colors_codex_append() {
    shell_file_read_colors("codex", 15);
}

#[test]
fn terminal_shell_file_read_colors_codex_regex_range() {
    shell_file_read_markdown("codex", 0);
}

#[test]
fn terminal_shell_file_read_colors_codex_delete() {
    shell_file_read_markdown("codex", 1);
}

fn start_shell_output_probe(
    name: &str,
    command: &str,
    body: &str,
    provider: &str,
) -> (tempfile::TempDir, Tmux) {
    if provider == "claude" {
        let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut calls: Vec<serde_json::Value> = include_str!("../fixtures/claude-tool-calls.jsonl")
            .lines()
            .take(2)
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        calls[0]["message"]["content"][0]["input"]["command"] = command.into();
        calls[1]["toolUseResult"]["stdout"] = body.into();
        calls[1]["toolUseResult"]["stderr"] = "".into();
        calls[1]["message"]["content"][0]["content"] = body.into();
        records.extend(calls);
        let transcript = records
            .iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>();
        let (directory, tmux, _) = start_probe_with_transcript(name, &transcript);
        (directory, tmux)
    } else {
        let mut record = include_str!("../fixtures/codex-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
            .unwrap();
        record["payload"]["item"]["command"][2] = command.into();
        record["payload"]["item"]["aggregated_output"] = body.into();
        terminal_codex_exit::start_codex_probe(name, &format!("{record}\n"))
    }
}

#[test]
fn terminal_shell_sed_grep_head_colors() {
    let input: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/sed-grep-head/input.json")).unwrap();
    let command = input["tool_use"]["input"]["command"].as_str().unwrap();
    let body = include_str!("../fixtures/sed-grep-head/stdout.txt");
    let (_directory, tmux) = start_shell_output_probe("sed-grep-head", command, body, "claude");
    tmux.run(&["resize-window", "-t", "probe", "-x", "240", "-y", "600"]);
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let capture = tmux.capture_when(|capture| {
        capture.contains("/// Formats source code") && capture.contains("protects_heredoc_bodies()")
    });
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parser = captured_screen(&colored, 240, 600);
    let color = |line: &str, needle: &str| {
        let (row, text) = capture
            .lines()
            .enumerate()
            .find(|(_, text)| text.contains(line))
            .unwrap_or_else(|| panic!("missing {line:?}: {capture}"));
        parser
            .screen()
            .cell(
                row as u16,
                text[..text.find(needle).unwrap()].width() as u16,
            )
            .unwrap()
            .fgcolor()
    };
    assert_ne!(
        color("/// Formats source code", "Formats"),
        color("    Text,", "Text"),
        "first sed stayed plain: {capture}"
    );
    for (line, identifier, string) in [
        ("let label = pointer.rsplit", "pointer", "\"\""),
        ("if lang.eq_ignore_ascii_case(\"md\")", "lang", "\"md\""),
    ] {
        assert_ne!(color(line, identifier), color(line, string), "{capture}");
    }
    let grep = "ui/shell_commands.rs:55:    let mut heredocs = Vec::new();";
    assert_eq!(color(grep, "let"), color(grep, "heredocs"), "{capture}");
    let path = std::env::temp_dir().join("agents-sed-grep-head.ansi");
    std::fs::write(&path, colored).unwrap();
    let verdict = serde_json::json!({"verdict":"pass", "provider":"claude", "rust_sections":3, "grep_plain":true, "capture":path});
    std::fs::write(path.with_extension("json"), verdict.to_string()).unwrap();
    println!("{verdict}");
}

#[test]
fn terminal_shell_line_format_filters_colors() {
    for provider in ["claude", "codex"] {
        for (name, filter) in [
            ("head", "head -25"),
            ("tail", "tail -n 25"),
            ("sed", "sed -n 1,25p"),
        ] {
            probe_shell_output_colors(
                &format!("line-format-{provider}-{name}"),
                provider,
                &format!("sed -n 140,190p a.rs; rg -n answer a.rs | {filter}"),
                &[
                    "let answer = \"file-read-highlight\";",
                    "72:let plain = \"search-output\";",
                ],
                &[(0, "answer", "file-read-highlight")],
                &[(1, "plain =", "search-output")],
            );
        }
    }
}

#[test]
fn terminal_shell_output_boundaries_colors() {
    for provider in ["claude", "codex"] {
        probe_shell_output_colors(
            &format!("literal-marker-{provider}"),
            provider,
            "cat a.rs; rg -n marker a.rs | head -25",
            &[
                "let answer = \"file-read-highlight\";",
                "72:let plain = \"…133 tokens truncated…\";",
            ],
            &[(0, "answer", "file-read-highlight")],
            &[(1, "plain =", "133 tokens")],
        );
    }
    probe_shell_output_colors(
        "merged-diagnostic",
        "codex",
        "cat a.rs; rg -n answer missing.rs a.rs | head -25",
        &[
            "let answer = \"file-read-highlight\";",
            "rg: missing.rs: No such file or directory",
            "72:let plain = \"search-output\";",
        ],
        &[(0, "answer", "file-read-highlight")],
        &[(1, "rg:", "missing.rs"), (2, "plain =", "search-output")],
    );
}

#[test]
fn terminal_shell_multi_file_read_colors() {
    let command = "sed -n '1p' a.rs; echo '---'; head -n 1 b.py && cat Cargo.toml";
    let lines = [
        "let answer = \"rust-output\";",
        "---",
        "print(\"python-output\")",
        "name = \"toml-output\"",
    ];
    probe_shell_output_colors(
        "multi-file-read",
        "claude",
        command,
        &lines,
        &[
            (0, "answer", "rust-output"),
            (2, "print", "python-output"),
            (3, "name", "toml-output"),
        ],
        &[],
    );
}

#[test]
fn terminal_shell_reads_before_other_commands_colors() {
    probe_shell_output_colors(
        "read-tail",
        "claude",
        "sed -n '1p' a.rs; sed -n '2p' a.rs; rg foo src",
        &[
            "let first = \"first-output\";",
            "let second = \"second-output\";",
            "src/main.rs: let plain = \"plain-output\";",
        ],
        &[(0, "first", "first-output"), (1, "second", "second-output")],
        &[(2, "plain =", "plain-output")],
    );
}

fn probe_shell_output_colors(
    name: &str,
    provider: &str,
    command: &str,
    lines: &[&str],
    highlighted: &[(usize, &str, &str)],
    plain: &[(usize, &str, &str)],
) {
    let body = lines.join("\n");
    let (_directory, tmux) = start_shell_output_probe(name, command, &body, provider);
    tmux.run(&["resize-window", "-t", "probe", "-x", "200", "-y", "50"]);
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    let capture = tmux.capture_when(|capture| capture.contains(lines.last().unwrap()));
    let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let path = std::env::temp_dir().join(format!("agents-{name}.ansi"));
    std::fs::write(&path, &colored).unwrap();
    let parser = captured_screen(&colored, 200, 50);
    let output_start = capture
        .lines()
        .position(|line| line.contains(lines[0]))
        .unwrap();
    let rows: Vec<_> = lines
        .iter()
        .map(|needle| {
            capture
                .lines()
                .enumerate()
                .skip(output_start)
                .find_map(|(row, line)| line.contains(needle).then_some((row, line)))
                .unwrap_or_else(|| panic!("missing {needle:?}: {capture}"))
        })
        .collect();
    for pair in rows.windows(2) {
        assert_eq!(
            pair[0].0 + 1,
            pair[1].0,
            "output is not continuous: {capture}"
        );
    }
    let color = |index: usize, needle: &str| {
        let (row, line) = rows[index];
        parser
            .screen()
            .cell(
                row as u16,
                line[..line.find(needle).unwrap()].width() as u16,
            )
            .unwrap()
            .fgcolor()
    };
    for &(index, identifier, string) in highlighted {
        assert_ne!(color(index, identifier), color(index, string), "{capture}");
    }
    for &(index, identifier, string) in plain {
        assert_eq!(color(index, identifier), color(index, string), "{capture}");
    }
    let verdict =
        serde_json::json!({"verdict": "pass", "continuous_rows": rows.len(), "capture": path});
    std::fs::write(path.with_extension("json"), verdict.to_string()).unwrap();
    println!("{verdict}");
}

#[test]
fn terminal_git_global_diff_colors() {
    let call = include_str!("../fixtures/shell-output-corpus.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|call| call["command"] == "chmod +x x.sh && git -C ~/.claude diff -- settings.json")
        .unwrap();
    let command = call["command"].as_str().unwrap();
    let body = call["stdout"].as_str().unwrap();
    for provider in ["claude", "codex"] {
        let name = format!("git-global-diff-{provider}");
        let (_directory, tmux) = start_shell_output_probe(&name, command, body, provider);
        tmux.run(&["send-keys", "-t", "probe", "End"]);
        let capture = tmux.capture_when(|capture| capture.contains("+{\"enabled\": true}"));
        let colored = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
        let parser = captured_screen(&colored, 200, 50);
        let background = |needle: &str| {
            let (row, line) = capture
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains(needle))
                .unwrap();
            parser
                .screen()
                .cell(
                    row as u16,
                    line[..line.find(needle).unwrap()].width() as u16,
                )
                .unwrap()
                .bgcolor()
        };
        let deleted = background("-{\"enabled\": false}");
        let inserted = background("+{\"enabled\": true}");
        let header = background("diff --git a/settings.json b/settings.json");
        assert_ne!(deleted, inserted, "{capture}");
        assert_ne!(deleted, header, "{capture}");
        assert_ne!(inserted, header, "{capture}");
        let path = std::env::temp_dir().join(format!("agents-{name}.ansi"));
        std::fs::write(&path, colored).unwrap();
        let verdict = serde_json::json!({"verdict": "pass", "provider": provider, "capture": path});
        std::fs::write(path.with_extension("json"), verdict.to_string()).unwrap();
        println!("{verdict}");
    }
}
