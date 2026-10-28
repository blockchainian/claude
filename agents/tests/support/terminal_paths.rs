use super::*;

fn cwd_relative_paths(name: &str, width: usize) {
    let project = tempfile::tempdir().unwrap();
    let target = project.path().join("agents/src/ui/draft_flow_tests.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "fn main() {}\n").unwrap();

    // Keep the recorded provider payload and resolve its file inside the probe project.
    let mut records = Vec::new();
    let mut record = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["message"]["content"][0]["name"] == name)
        .unwrap();
    record["cwd"] = project.path().to_str().unwrap().into();
    record["timestamp"] = "2026-10-18T12:00:00Z".into();
    let input = &mut record["message"]["content"][0]["input"];
    input["file_path"] = target.to_str().unwrap().into();
    records.push(record);
    if name == "Read" {
        let mut result = include_str!("../fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["toolUseResult"]["file"].is_object())
            .unwrap();
        result["cwd"] = project.path().to_str().unwrap().into();
        result["timestamp"] = "2026-10-18T12:00:01Z".into();
        result["toolUseResult"]["file"]["filePath"] = target.to_str().unwrap().into();
        records.push(result);
    }
    let transcript = records
        .into_iter()
        .map(|record| format!("{record}\n"))
        .collect::<String>();
    let (_directory, tmux, _transcript) =
        start_probe_with_transcript(&format!("cwd-path-{name}"), &transcript);

    tmux.run(&[
        "resize-window",
        "-t",
        "probe",
        "-x",
        &width.to_string(),
        "-y",
        "50",
    ]);
    let expected = if name == "Read" {
        "agents/src/ui/draft_flow_tests.rs:44-63"
    } else {
        "agents/src/ui/draft_flow_tests.rs"
    };
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .any(|line| line.contains(name) && line.contains(expected))
    });
    let title = capture
        .lines()
        .find(|line| line.contains(name) && line.contains(expected))
        .unwrap();
    assert!(!title.contains(project.path().to_str().unwrap()), "{title}");
    let path = std::env::temp_dir().join(format!("agents-cwd-path-{name}-{width}.txt"));
    std::fs::write(&path, capture).unwrap();
    println!(
        "{{\"cwd_path\":\"pass\",\"tool\":\"{name}\",\"width\":{width},\"capture\":\"{}\"}}",
        path.display()
    );
}

#[test]
fn terminal_cwd_relative_paths_read_200() {
    cwd_relative_paths("Read", 200);
}

#[test]
fn terminal_cwd_relative_paths_read_120() {
    cwd_relative_paths("Read", 120);
}

#[test]
fn terminal_cwd_relative_paths_edit_200() {
    cwd_relative_paths("Edit", 200);
}

#[test]
fn terminal_cwd_relative_paths_edit_120() {
    cwd_relative_paths("Edit", 120);
}
