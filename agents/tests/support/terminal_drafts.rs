use super::*;

fn picker_has_row(capture: &str, label: &str) -> bool {
    capture.lines().any(|line| {
        line.split('│')
            .nth(1)
            .is_some_and(|part| part.trim() == label)
    })
}

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
    tmux.wait_for(|capture| picker_has_row(capture, "destination"));
    click_text(&tmux, "..");
    tmux.wait_for(|capture| picker_has_row(capture, "group"));
    click_text(&tmux, "group");
    tmux.wait_for(|capture| picker_has_row(capture, "destination"));
    click_text(&tmux, "destination");
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.contains("target") && line.contains("New task"))
    });
    println!("{{\"repository_single_click\":\"pass\",\"folder\":\"pass\",\"parent\":\"pass\"}}");
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
fn terminal_new_task_status_is_capitalized_in_list_and_task_details() {
    let (_directory, tmux, _transcript) = start_probe("draft-status");
    begin_named_draft(&tmux, "Status probe");
    let capture = tmux.capture_when(|capture| {
        capture.lines().any(|line| {
            line.contains("Status probe") && line.contains("New") && !line.contains('─')
        })
    });
    assert!(
        capture
            .lines()
            .any(|line| line.contains('─') && line.split_whitespace().any(|word| word == "New")),
        "{capture}"
    );
    assert!(
        !capture.lines().any(|line| line.contains("Status probe")
            && line.split_whitespace().any(|word| word == "new")),
        "{capture}"
    );
    save_title_capture(&tmux, "new-task-status");
    println!(
        "{}",
        serde_json::json!({"verdict":"pass", "task_list":"New", "details":"New"})
    );
}
