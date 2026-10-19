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
    let capture = assert_title_caret(&tmux, "New task", 0);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("main") && line.contains("New task"))
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
            .any(|line| line.contains("target") && line.contains("New task"))
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
    begin_named_draft(&tmux, "background task");
    tmux.run(&["send-keys", "-t", "probe", "-l", "background prompt"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let branches = tmux.capture_when(|capture| capture.contains("Create new branch"));
    assert!(branches.contains("background task ❯"), "{branches}");
    assert!(
        !branches.contains("background task ❯ background prompt"),
        "{branches}"
    );
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| capture.contains("New branch ❯"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "feature/async"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let capture = tmux.capture_when(|capture| {
        !capture.contains("Create new branch") && !capture.contains("New branch ❯")
    });
    assert!(!capture.contains("background task ❯"), "{capture}");
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

fn save_title_capture(tmux: &Tmux, name: &str) {
    let Some(directory) = std::env::var_os("AGENTS_PROBE_CAPTURE_DIR") else {
        return;
    };
    let directory = Path::new(&directory);
    std::fs::create_dir_all(directory).unwrap();
    for (suffix, flags) in [("txt", vec!["-p"]), ("ansi", vec!["-p", "-e"])] {
        let mut args = vec!["capture-pane", "-t", "probe"];
        args.extend(flags);
        std::fs::write(directory.join(format!("{name}.{suffix}")), tmux.run(&args)).unwrap();
    }
    std::fs::write(
        directory.join(format!("{name}.cursor.txt")),
        tmux.run(&[
            "display-message",
            "-t",
            "probe",
            "-p",
            "#{cursor_x},#{cursor_y},#{cursor_flag}",
        ]),
    )
    .unwrap();
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_names_edit_on_details_rule() {
    let (_directory, tmux, _transcript) = start_probe("inline-task-name");
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    assert_title_caret(&tmux, "New task", 0);
    save_title_capture(&tmux, "01-new-task-placeholder");
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    assert_title_caret(&tmux, "New task", 0);
    tmux.run(&["send-keys", "-t", "probe", "-l", "边框任务"]);
    assert_title_caret(&tmux, "边框任务", 8);
    tmux.run(&["send-keys", "-t", "probe", "Home", "DC", "DC"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "就地"]);
    assert_title_caret(&tmux, "就地任务", 4);
    tmux.run(&["send-keys", "-t", "probe", "End", "BSpace"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "务"]);
    tmux.run(&["send-keys", "-t", "probe", "Left"]);
    assert_title_caret(&tmux, "就地任务", 6);
    save_title_capture(&tmux, "02-new-task-name");
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| capture.contains("就地任务 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "draft prompt"]);
    tmux.wait_for(|capture| capture.contains("就地任务 ❯ draft prompt"));
    save_title_capture(&tmux, "03-new-task-prompt");
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| !capture.contains("就地任务 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    assert_title_caret(&tmux, "New task", 0);
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| title_rule_position(capture, "New task").is_none());
    save_title_capture(&tmux, "04-new-task-cancelled");
    // A cancelled draft leaves Active selected; the fixture task is Inactive.
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);

    let original = "Codex manager 停止时线程处理";
    let capture = tmux.capture_when(|capture| capture.contains(original));
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(original) && !line.contains('─'))
        .unwrap();
    let column = line[..line.find(original).unwrap()].width();
    tmux.mouse(0, column, row, true);
    let capture =
        tmux.capture_when(|capture| title_rule_position(capture, "Codex manager").is_some());
    let (column, row) = title_rule_position(&capture, "Codex manager").unwrap();
    tmux.mouse(0, column, row, true);
    let cursor = tmux.run(&[
        "display-message",
        "-t",
        "probe",
        "-p",
        "#{cursor_x},#{cursor_y},#{cursor_flag}",
    ]);
    let fields: Vec<_> = cursor.trim().split(',').collect();
    let cursor_row: usize = fields[1].parse().unwrap();
    assert!(
        fields[2] == "0" || cursor_row != row,
        "single click must not start name editing: {cursor}"
    );
    std::thread::sleep(Duration::from_millis(600));
    tmux.mouse(0, column, row, true);
    tmux.mouse(0, column, row, true);
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    assert_title_caret(&tmux, "Codex manager", 0);
    tmux.run(&["send-keys", "-t", "probe", "End", "C-u"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "双击改名"]);
    assert_title_caret(&tmux, "双击改名", 8);
    save_title_capture(&tmux, "05-double-click-rename");
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let capture = tmux.capture_when(|capture| {
        title_rule_position(capture, "双击改名").is_some() && !capture.contains(original)
    });
    save_title_capture(&tmux, "06-rename-saved");
    let (column, row) = title_rule_position(&capture, "双击改名").unwrap();
    tmux.mouse(0, column, row, true);
    tmux.mouse(0, column, row, true);
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    assert_title_caret(&tmux, "双击改名", 8);
    tmux.run(&["send-keys", "-t", "probe", "C-u"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "discarded rename"]);
    assert_title_caret(&tmux, "discarded rename", 16);
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| {
        title_rule_position(capture, "双击改名").is_some() && !capture.contains("discarded rename")
    });
    save_title_capture(&tmux, "07-rename-cancelled");
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("双击改名") && !line.contains('─'))
        .unwrap();
    tmux.mouse(0, line[..line.find("双击改名").unwrap()].width(), row, true);
    tmux.run(&["send-keys", "-t", "probe", "r", "Home"]);
    assert_title_caret(&tmux, "双击改名", 0);
    tmux.run(&["send-keys", "-t", "probe", "End", "C-u"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "键盘改名"]);
    assert_title_caret(&tmux, "键盘改名", 8);
    save_title_capture(&tmux, "08-keyboard-rename");
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| {
        title_rule_position(capture, "键盘改名").is_some() && !capture.contains("双击改名")
    });
    save_title_capture(&tmux, "09-keyboard-rename-saved");
    println!(
        r#"{{"verdict":"pass","inline_name":"pass","wide_caret":"pass","double_click_rename":"pass","single_click":"pass","rename_cancel":"pass","keyboard_rename":"pass"}}"#
    );
}
