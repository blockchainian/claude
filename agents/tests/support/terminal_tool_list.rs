use super::*;

const WIDTH: usize = 240;

fn event_divider(capture: &str) -> Option<usize> {
    capture
        .lines()
        .enumerate()
        .filter(|(_, row)| row.matches('─').count() > WIDTH / 2)
        .map(|(index, _)| index)
        .last()
}

fn list_row<'a>(capture: &'a str, label: &str) -> Option<(usize, &'a str)> {
    let divider = event_divider(capture)?;
    capture.lines().take(divider).enumerate().find(|(_, row)| {
        row.rsplit_once('│')
            .is_some_and(|(_, list)| list.contains(label))
    })
}

fn select_tool(tmux: &Tmux, label: &str) -> String {
    let capture = tmux.capture_when(|capture| list_row(capture, label).is_some());
    let (row, line) = list_row(&capture, label).unwrap();
    let column = line[..line.find(label).unwrap()].width();
    let (name, subject) = label.split_once(' ').unwrap();
    let subject = subject.trim_start_matches('{').split(',').next().unwrap();
    tmux.mouse(0, column, row, true);
    tmux.capture_when(|capture| {
        capture
            .lines()
            .skip(event_divider(capture).unwrap() + 1)
            .any(|row| row.trim_start().starts_with(name) && row.contains(subject))
    })
}

fn assert_title(capture: &str, name: &str, subject: &str) {
    let title = capture
        .lines()
        .skip(event_divider(capture).unwrap() + 1)
        .find(|row| row.trim_start().starts_with(name) && row.contains(subject))
        .unwrap_or_else(|| panic!("missing {name} / {subject} detail title: {capture}"));
    assert_eq!(title[..title.find(name).unwrap()].width(), 1, "{capture}");
    assert_eq!(title.matches(name).count(), 1, "{capture}");
    assert_eq!(
        title[..title.find(subject).unwrap()].width(),
        terminal_event_detail_width::event_detail_center(WIDTH, subject.width()),
        "subject stays centered independently of the tool name: {capture}"
    );
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
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_claude_tool_list_names_groups_and_paths() {
    let project = tempfile::tempdir().unwrap();
    let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let calls = include_str!("../fixtures/tool-list/claude.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap());
    for (index, mut record) in calls.enumerate() {
        let relative = match index {
            0 => "src/read.rs",
            1 => "src/write.rs",
            2 => "src/edit.rs",
            3 => "src/a.rs",
            4 => "src/b.rs",
            _ => panic!("unexpected recorded fixture row"),
        };
        let target = project.path().join(relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "fn main() {}\n").unwrap();
        record["message"]["content"][0]["input"]["file_path"] = target.to_str().unwrap().into();
        records.push(record);
    }
    let bash: serde_json::Value = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["message"]["content"][0]["name"] == "Bash")
        .unwrap();
    let description = bash["message"]["content"][0]["input"]["description"]
        .as_str()
        .unwrap()
        .to_owned();
    records.push(bash);
    for record in &mut records {
        record["cwd"] = project.path().to_str().unwrap().into();
    }
    let transcript = records
        .into_iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    let (_directory, tmux, _transcript) =
        start_probe_with_transcript("claude-tool-list", &transcript);
    tmux.run(&["resize-window", "-t", "probe", "-x", "240", "-y", "70"]);
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .any(|row| row.width() == WIDTH && row.matches('─').count() > WIDTH / 2)
            && [
                "Read src/read.rs",
                "Write src/write.rs",
                "Edit src/edit.rs",
                "Edit {src/a.rs, src/b.rs}",
                description.as_str(),
            ]
            .iter()
            .all(|label| list_row(capture, label).is_some())
    });
    let (_, bash_row) = list_row(&capture, &description).unwrap();
    let bash_list = bash_row.rsplit_once('│').unwrap().1;
    assert!(!bash_list.contains("Bash"), "{capture}");
    assert!(
        !capture.contains(project.path().to_str().unwrap()),
        "{capture}"
    );
    let mut captures = vec![capture];
    for (name, subject) in [
        ("Read", "src/read.rs"),
        ("Write", "src/write.rs"),
        ("Edit", "src/edit.rs"),
    ] {
        let capture = select_tool(&tmux, &format!("{name} {subject}"));
        assert_title(&capture, name, subject);
        captures.push(capture);
    }
    let capture = select_tool(&tmux, "Edit {src/a.rs, src/b.rs}");
    assert_title(&capture, "Edit", "src/a.rs");
    assert_title(&capture, "Edit", "src/b.rs");
    captures.push(capture);
    write_verdict("claude", &captures);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
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
    let title = capture
        .lines()
        .skip(event_divider(&capture).unwrap() + 1)
        .find(|row| row.trim_start().starts_with("Exec command"))
        .unwrap();
    assert_eq!(title.find("Exec command"), Some(1), "{capture}");
    assert!(title.contains(label), "{capture}");
    assert!(capture.contains("git status --short"), "{capture}");
    assert!(!capture.contains("exec_command"), "{capture}");
    write_verdict("codex", &[capture]);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
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
        !row.rsplit_once('│').unwrap().1.contains("Bash"),
        "{capture}"
    );
    write_verdict("claude-shell-commands", &[capture]);
}
