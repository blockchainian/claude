use super::*;

#[test]
fn terminal_shell_heredoc_formatting() {
    use std::io::Write;

    let fixture = include_str!("../fixtures/claude-shell-heredoc.jsonl");
    let event: serde_json::Value = serde_json::from_str(fixture).unwrap();
    let command = event["message"]["content"][0]["input"]["command"]
        .as_str()
        .unwrap();
    let (prefix, script_and_suffix) = command.split_once('\n').unwrap();
    let (body, suffix) = script_and_suffix.split_once("EOF\n").unwrap();
    assert!(prefix.ends_with("<<'EOF'"));
    assert!(body.contains("from 'node:fs'\n"));
    // Shell sections display the command chain as separate commands, omitting
    // the outer && and semicolon separators while preserving quoted scripts.
    let (directory_command, heredoc_command) = prefix.split_once(" && ").unwrap();
    let (suffix_commands, python_body) = suffix.split_once('\n').unwrap();
    let suffix_commands: Vec<_> = suffix_commands.splitn(3, "; ").collect();
    assert_eq!(suffix_commands.len(), 3);
    assert!(suffix_commands[0].starts_with("node "));
    assert!(suffix_commands[1].starts_with("echo "));
    assert_eq!(suffix_commands[2], "python3 -c \"");
    let shell_rows: Vec<_> = [directory_command, heredoc_command, "EOF"]
        .into_iter()
        .chain(suffix_commands)
        .chain(python_body.lines())
        .collect();
    let output = std::env::var_os("AGENTS_HEREDOC_PROBE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/heredoc-format-cal/probe")
        });
    std::fs::create_dir_all(&output).unwrap();

    for provider in ["claude", "codex"] {
        // The real upstream command is displayed by the app, never executed.
        let (_directory, tmux) = if provider == "claude" {
            let (directory, tmux, transcript) = start_probe("heredoc-format-claude");
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(transcript)
                .unwrap();
            write!(file, "{fixture}").unwrap();
            file.flush().unwrap();
            (directory, tmux)
        } else {
            let mut record = include_str!("../fixtures/codex-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .find(|record| record["payload"]["item"]["type"] == "CommandExecution")
                .unwrap();
            record["payload"]["item"]["command"][2] = command.into();
            record["payload"]["item"]["aggregated_output"] = "".into();
            terminal_codex_exit::start_codex_probe("heredoc-format-codex", &format!("{record}\n"))
        };
        tmux.run(&["resize-window", "-t", "probe", "-x", "360", "-y", "80"]);
        tmux.run(&["send-keys", "-t", "probe", "End"]);
        let capture = tmux.capture_when(|capture| {
            capture.contains("beautiful-mermaid") && capture.contains("collections.Counter")
        });
        let text_path = output.join(format!("{provider}.txt"));
        let ansi_path = output.join(format!("{provider}.ansi"));
        std::fs::write(&text_path, &capture).unwrap();
        std::fs::write(
            &ansi_path,
            tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]),
        )
        .unwrap();

        let formatted = capture.contains("import { readFileSync } from 'node:fs';")
            && capture.contains("import { renderMermaidASCII } from 'beautiful-mermaid';");
        let mut displayed_rows = capture
            .lines()
            .map(|line| line.trim().trim_start_matches(['┌', '│', '└']).trim_start());
        let shell_preserved = shell_rows
            .iter()
            .all(|expected| displayed_rows.any(|actual| actual == *expected));
        let verdict = serde_json::json!({
            "verdict": if formatted && shell_preserved { "pass" } else { "fail" },
            "provider": provider,
            "formatted": formatted,
            "shell_preserved": shell_preserved,
            "shell_rows": shell_rows,
            "capture": text_path,
            "ansi": ansi_path,
        });
        std::fs::write(
            output.join(format!("{provider}.json")),
            serde_json::to_vec_pretty(&verdict).unwrap(),
        )
        .unwrap();
        println!("{verdict}");
        assert!(
            formatted,
            "{provider}: heredoc was not formatted: {capture}"
        );
        assert!(shell_preserved, "{provider}: shell text changed: {capture}");
    }
}
