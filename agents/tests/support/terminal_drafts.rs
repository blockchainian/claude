use super::*;

fn repository(path: &Path, branch: &str) {
    std::fs::create_dir_all(path).unwrap();
    for args in [
        vec!["init", "-b", branch],
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
            .current_dir(path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

fn transcript_at(cwd: &Path) -> String {
    include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["cwd"] = cwd.to_str().unwrap().into();
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn click_text(tmux: &Tmux, text: &str) {
    let capture = tmux.capture_when(|capture| {
        capture
            .lines()
            .any(|line| line.split('│').any(|part| part.trim() == text))
    });
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.split('│').any(|part| part.trim() == text))
        .unwrap();
    let column = line[..line.find(text).unwrap()].width();
    tmux.mouse(0, column, row, true);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_repository_browser_applies_single_clicks() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    repository(&source, "main");
    let destination = root.path().join("group/destination");
    repository(&destination, "main");
    let output = Command::new("git")
        .args(["checkout", "-b", "target"])
        .current_dir(destination)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let (_directory, tmux, _transcript) =
        start_probe_with_transcript("repository-click", &transcript_at(&source));
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    let capture = tmux.capture_when(|capture| capture.contains("New task ❯"));
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("main") && line.contains(" New "))
        .unwrap();
    let column = line[..line.find("source").unwrap()].width();
    tmux.mouse(0, column, row, true);
    click_text(&tmux, "group");
    click_text(&tmux, "..");
    click_text(&tmux, "group");
    click_text(&tmux, "destination");
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.contains("target") && line.contains(" New "))
    });
    println!("{{\"repository_single_click\":\"pass\",\"folder\":\"pass\",\"parent\":\"pass\"}}");
}
