use super::*;

const WIDTH: usize = 240;

fn list_row<'a>(capture: &'a str, label: &str) -> Option<(usize, &'a str)> {
    let divider = event_detail_rows(capture, WIDTH)?.start - 1;
    capture.lines().take(divider).enumerate().find(|(_, row)| {
        row.trim_end_matches(" │")
            .rsplit_once('│')
            .is_some_and(|(_, list)| list.contains(label))
    })
}

fn write_verdict(name: &str, captures: &[String]) {
    let directory = std::env::temp_dir().join("agents-tool-list-probe");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{name}.txt"));
    std::fs::write(&path, captures.join("\n\n")).unwrap();
    let verdict = serde_json::json!({
        "verdict": "pass", "provider": name, "capture": path,
        "tool_names": true, "event_detail_titles": true,
    });
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
fn terminal_codex_tool_list_shell_group() {
    let (_directory, tmux) = terminal_codex_exit::start_codex_probe(
        "codex-tool-list",
        include_str!("../fixtures/codex-inner/git-chain.jsonl"),
    );
    tmux.run(&["resize-window", "-t", "probe", "-x", "240", "-y", "70"]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|row| row.width() == WIDTH && row.matches('─').count() > WIDTH / 2)
    });
    let label = "git {status, diff}, sed";
    let capture = tmux.capture_when(|capture| list_row(capture, label).is_some());
    let (row, line) = list_row(&capture, label).unwrap();
    let column = line[..line.find(label).unwrap()].width();
    tmux.mouse(0, column, row, true);
    let capture = tmux.capture_when(|capture| {
        capture.contains("Exec command") && capture.contains("git status --short")
    });
    let detail = event_detail_rows(&capture, WIDTH).unwrap();
    let title = capture
        .lines()
        .skip(detail.start)
        .take(detail.len())
        .find(|row| row.trim_start().starts_with("Exec command"))
        .unwrap();
    assert_eq!(title.find("Exec command"), Some(1), "{capture}");
    assert!(title.contains(label), "{capture}");
    assert!(capture.contains("git status --short"), "{capture}");
    assert!(!capture.contains("exec_command"), "{capture}");
    write_verdict("codex", &[capture]);
}

#[test]
fn terminal_claude_shell_commands_without_description() {
    let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut bash = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["message"]["content"][0]["name"] == "Bash")
        .unwrap();
    let command: serde_json::Value = serde_json::from_str(
        include_str!("../fixtures/codex-inner/git-chain.jsonl")
            .lines()
            .nth(1)
            .unwrap(),
    )
    .unwrap();
    bash["message"]["content"][0]["input"] = serde_json::json!({
        "command": command["payload"]["item"]["command"][2]
    });
    records.push(bash);
    let transcript = records
        .into_iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    let (_directory, tmux, _transcript) =
        start_probe_with_transcript("claude-shell-commands", &transcript);
    tmux.run(&["resize-window", "-t", "probe", "-x", "240", "-y", "70"]);
    let capture =
        tmux.capture_when(|capture| list_row(capture, "git {status, diff}, sed").is_some());
    let (_, row) = list_row(&capture, "git {status, diff}, sed").unwrap();
    assert!(
        !row.trim_end_matches(" │")
            .rsplit_once('│')
            .unwrap()
            .1
            .contains("Bash"),
        "{capture}"
    );
    write_verdict("claude-shell-commands", &[capture]);
}
