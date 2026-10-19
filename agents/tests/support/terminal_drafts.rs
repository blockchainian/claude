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
    let capture = tmux.capture_when(|capture| capture.contains("New prompt ❯"));
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

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_submitted_draft_closes_while_git_continues_and_failure_shows_notice() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    repository(&source, "main");
    let started = root.path().join("started");
    let release = root.path().join("release");
    let hook = source.join(".git/hooks/post-checkout");
    // Pause the real git checkout, then reject it without invoking a task provider.
    std::fs::write(
        &hook,
        format!(
            "#!/usr/bin/env python3
import pathlib, signal, sys, time
signal.alarm(20)
pathlib.Path({}).touch()
while not pathlib.Path({}).exists():
 time.sleep(0.05)
print('probe checkout rejected', file=sys.stderr)
sys.exit(1)
",
            serde_json::to_string(started.to_str().unwrap()).unwrap(),
            serde_json::to_string(release.to_str().unwrap()).unwrap(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (_directory, tmux, _transcript) = start_probe_with_setup(
        "async-draft",
        &transcript_at(&source),
        |directory| {
            std::fs::write(directory.join("claude/agents.json"), serde_json::to_vec(
                &serde_json::json!({"worktree_root": root.path().join("trees"), "grouping":"project", "pinned":[]})
            ).unwrap()).unwrap();
        },
    );
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    let capture = tmux.capture_when(|capture| capture.contains("New prompt ❯"));
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("main") && line.contains(" New "))
        .unwrap();
    tmux.mouse(0, line[..line.find("main").unwrap()].width(), row, true);
    tmux.wait_for(|capture| capture.contains("Create new branch"));
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "feature/async"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "background task"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| capture.contains("New prompt ❯"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "background prompt"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let capture = tmux.capture_when(|capture| !capture.contains("New prompt ❯"));
    assert!(!capture.contains("New prompt ❯"), "{capture}");
    assert!(
        !capture.contains("Creating") && !capture.contains("Sending"),
        "{capture}"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !started.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "git hook did not start"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    tmux.run(&["send-keys", "-t", "probe", "-l", "still editable"]);
    tmux.wait_for(|capture| capture.contains("❯ still editable"));
    assert!(!release.exists());
    std::fs::write(&release, "").unwrap();
    tmux.wait_for(|capture| capture.contains("Preparing worktree"));
    println!(r#"{{"background_git":"pass","prompt_restored":"pass","failure_notice":"pass"}}"#);
}
