use super::*;

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_path_suffixes_and_cwd_basename() {
    let change: serde_json::Value =
        include_str!("../fixtures/codex-inner/file-change-without-start.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["payload"]["item"]["type"] == "FileChange")
            .unwrap();
    let recorded_path = change["payload"]["item"]["stdout"]
        .as_str()
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("M "))
        .unwrap();
    let basename = Path::new(recorded_path)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let project_root = tempfile::tempdir().unwrap();
    let cwd = project_root.path().join("path-cwd");
    let target = cwd.join("docs/superpowers/specs").join(basename);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, "").unwrap();

    let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for record in &mut records {
        record["cwd"] = cwd.to_str().unwrap().into();
    }
    let mut read: serde_json::Value = include_str!("../fixtures/claude-tool-calls.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["message"]["content"][0]["name"] == "Read")
        .unwrap();
    read["cwd"] = cwd.to_str().unwrap().into();
    read["timestamp"] = "2026-10-18T12:00:00Z".into();
    read["message"]["content"][0]["input"]["file_path"] = target.to_str().unwrap().into();
    records.push(read);
    let transcript: String = records.iter().map(|record| format!("{record}\n")).collect();
    let (_directory, tmux, _transcript) = start_probe_with_transcript("path-suffixes", &transcript);

    let wide = tmux.capture_when(|capture| {
        capture
            .lines()
            .filter(|line| line.contains(basename))
            .count()
            >= 2
            && capture
                .lines()
                .any(|line| line.contains('─') && line.contains("path-cwd"))
    });
    // Both the log summary and the selected tool detail retain the filename.
    for line in wide.lines().filter(|line| line.contains(basename)) {
        assert!(!line.contains(cwd.to_str().unwrap()), "{line}");
        assert!(!line.contains("d…/s…/s…/"), "{line}");
    }
    let border = wide
        .lines()
        .find(|line| line.contains('─') && line.contains("path-cwd"))
        .unwrap();
    assert!(!border.contains("/path-cwd"), "{border}");

    tmux.run(&["resize-window", "-t", "probe", "-x", "52", "-y", "50"]);
    let narrow = tmux.capture_when(|capture| {
        capture.lines().count() == 50
            && capture
                .lines()
                .any(|line| line.contains("2026-06-12-") && line.contains('…'))
    });
    let shortened: Vec<_> = narrow
        .lines()
        .filter(|line| line.contains("2026-06-12-"))
        .collect();
    assert!(!shortened.is_empty(), "{narrow}");
    for line in shortened {
        assert!(line.contains('…'), "{line}");
        assert!(
            !line.contains(basename),
            "filename must truncate on the right: {line}"
        );
        assert!(
            !line.contains("…/2026-06-12-"),
            "drop directory anchors before truncating the filename: {line}"
        );
    }
    let capture_path = std::env::temp_dir().join("agents-path-display-probe.txt");
    std::fs::write(&capture_path, format!("wide:\n{wide}\nnarrow:\n{narrow}")).unwrap();
    let verdict = serde_json::json!({
        "verdict": "pass",
        "surfaces": ["cwd border", "log summary", "tool detail"],
        "widths": [200, 52],
        "capture": capture_path,
    });
    std::fs::write(
        std::env::temp_dir().join("agents-path-display-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
