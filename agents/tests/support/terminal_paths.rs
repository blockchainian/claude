use super::*;

#[test]
fn terminal_cwd_relative_paths() {
    let project = tempfile::tempdir().unwrap();
    let target = project.path().join("agents/src/ui/draft_flow_tests.rs");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "fn main() {}\n").unwrap();
    for name in ["Read", "Edit"] {
        // Keep the recorded provider payload and resolve its file inside the probe project.
        let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for record in &mut records {
            record["cwd"] = project.path().to_str().unwrap().into();
        }
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
        for width in [200, 120] {
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
                    .nth(1)
                    .is_some_and(|row| row.width() == width)
                    && capture.lines().nth(2).is_some_and(|row| {
                        row.matches('│').count() == if width >= 150 { 2 } else { 1 }
                    })
                    && capture
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
    }
}
