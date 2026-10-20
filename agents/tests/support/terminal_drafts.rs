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
fn terminal_submitted_draft_stays_new_while_git_continues_and_failure_keeps_prompt() {
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
                &serde_json::json!({"config":{"worktree_root": root.path().join("trees"), "grouping":"project"}, "state":{"pinned":[]}})
            ).unwrap()).unwrap();
        },
    );
    begin_named_draft(&tmux, "background task");
    tmux.run(&["send-keys", "-t", "probe", "-l", "background prompt"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.split('│').any(|part| part.trim() == ".."))
    });
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let branches = tmux.capture_when(|capture| capture.contains("Create new branch"));
    assert!(branches.contains("background task ❯"), "{branches}");
    assert!(
        branches.contains("background task ❯ background prompt"),
        "{branches}"
    );
    tmux.run(&["send-keys", "-t", "probe", "End"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.wait_for(|capture| capture.contains("New branch ❯"));
    tmux.run(&["send-keys", "-t", "probe", "-l", "feature/async"]);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    let capture = tmux.capture_when(|capture| {
        !capture.contains("Create new branch") && !capture.contains("New branch ❯")
    });
    assert!(
        capture.contains("background task ❯ background prompt"),
        "{capture}"
    );
    // Choosing the branch returns to the prompt; only this Enter starts git.
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    assert!(
        !capture.contains("Creating") && !capture.contains("Sending"),
        "{capture}"
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !started.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "git hook did not start: {}",
            tmux.run(&["capture-pane", "-t", "probe", "-p"])
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    tmux.run(&["send-keys", "-t", "probe", "-l", "still editable"]);
    let frozen = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(
        frozen.contains("background task ❯ background prompt"),
        "{frozen}"
    );
    assert!(frozen.contains("New"), "{frozen}");
    assert!(!frozen.contains("still editable"), "{frozen}");
    assert!(!release.exists());
    std::fs::write(&release, "").unwrap();
    tmux.wait_for(|capture| {
        capture.contains("probe checkout rejected")
            && capture.contains("background task ❯ background prompt")
    });
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
    tmux.wait_for(|capture| {
        capture
            .lines()
            .any(|line| line.split('│').any(|part| part.trim() == ".."))
    });
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| capture.contains("就地任务 ❯"));
    tmux.run(&["send-keys", "-t", "probe", "C-w"]);
    tmux.run(&["send-keys", "-t", "probe", "j"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "draft prompt"]);
    tmux.wait_for(|capture| capture.contains("就地任务 ❯ draft prompt"));
    save_title_capture(&tmux, "03-new-task-prompt");
    delete_named_draft(&tmux);
    tmux.wait_for(|capture| !capture.contains("就地任务 ❯"));
    tmux.mouse(0, 2, 2, true);
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    assert_title_caret(&tmux, "New task", 0);
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|capture| title_rule_position(capture, "New task").is_none());
    save_title_capture(&tmux, "04-new-task-cancelled");
    // A cancelled draft leaves Active selected; choose the fixture task in Inactive.
    tmux.mouse(0, 195, 0, true);

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

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_new_task_status_is_capitalized_in_list_and_details() {
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

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_draft_name_blur_persists_without_enter() {
    for blur in [false, true] {
        let (directory, tmux, _transcript) =
            start_probe(if blur { "name-blur" } else { "name-enter" });
        tmux.run(&["send-keys", "-t", "probe", "n"]);
        assert_title_caret(&tmux, "New task", 0);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Persistent terminal name"]);
        assert_title_caret(&tmux, "Persistent terminal name", 24);
        if blur {
            tmux.mouse(0, 0, 0, true);
        } else {
            tmux.run(&["send-keys", "-t", "probe", "Enter"]);
        }
        if blur {
            tmux.wait_for(|capture| capture.contains("Persistent terminal name ❯"));
        } else {
            tmux.wait_for(|capture| {
                capture
                    .lines()
                    .any(|line| line.split('│').any(|part| part.trim() == ".."))
            });
        }
        let path = directory.path().join("claude/agents.json");
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            document["state"]["drafts"][0]["name"],
            "Persistent terminal name"
        );
        if !blur {
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
            tmux.wait_for(|capture| capture.contains("Persistent terminal name ❯"));
        }
        save_title_capture(&tmux, if blur { "blur-saved" } else { "enter-saved" });
        tmux.run(&["send-keys", "-t", "probe", "C-c"]);
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            document["state"]["drafts"][0]["name"],
            "Persistent terminal name"
        );
    }
    println!(
        r#"{{"verdict":"pass","blur_save":"pass","enter_save":"pass","immediate_json":"pass"}}"#
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_draft_defaults_and_inherited_choices() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    repository(&source, "main");
    let mut checks = Vec::new();
    for (settings, model, effort) in [
        (None, "Opus 5.5", "medium"),
        (
            Some(
                r#"{"model":"claude-fable-5-1","effortLevel":"high","modelSettings":{"claude-fable-5-1":{"effortLevel":"xhigh"}}}"#,
            ),
            "Fable 5.1",
            "xhigh",
        ),
    ] {
        for width in [120, 200] {
            let (_directory, tmux, _) = start_probe_with_setup(
                "draft-defaults",
                &transcript_at(&source),
                |directory| {
                    let config = directory.join("claude");
                    std::fs::write(config.join(".claude.json"),
                        r#"{"oauthAccount":{"organizationUuid":"org-1","accountUuid":"ACCOUNT-1"}}"#).unwrap();
                    let hash = Sha256::digest(b"account-1")
                        .iter()
                        .take(6)
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>();
                    let cache = config.join("cache/model-catalog");
                    std::fs::create_dir_all(&cache).unwrap();
                    std::fs::write(
                        cache.join(format!("org-1-{hash}-cc.json")),
                        include_str!("../fixtures/claude-model-catalog.json"),
                    )
                    .unwrap();
                    if let Some(settings) = settings {
                        std::fs::write(config.join("settings.json"), settings).unwrap();
                    }
                },
            );
            tmux.run(&[
                "resize-window",
                "-t",
                "probe",
                "-x",
                &width.to_string(),
                "-y",
                "50",
            ]);
            tmux.run(&["send-keys", "-t", "probe", "n"]);
            for named in [false, true] {
                let title = if named {
                    tmux.run(&["send-keys", "-t", "probe", "-l", "Draft probe"]);
                    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
                    "Draft probe"
                } else {
                    "New task"
                };
                for value in [model, effort] {
                    let capture = tmux.capture_when(|capture| {
                        capture.contains(title)
                            && capture
                                .lines()
                                .any(|line| line.contains(model) && line.contains(effort))
                    });
                    let (row, line) = capture
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains(model) && line.contains(effort))
                        .unwrap();
                    let column = line[..line.find(value).unwrap()].width();
                    tmux.mouse(0, column, row, true);
                    let picker = tmux.capture_when(|capture| {
                        capture.contains("esc back") && capture.contains("enter select")
                    });
                    assert!(!picker.contains("Unknown"), "{picker}");
                    assert!(!picker.contains("Default"), "{picker}");
                    assert_eq!(
                        picker
                            .lines()
                            .filter(|line| line.split('│').any(|part| part.trim() == value))
                            .count(),
                        1,
                        "{picker}"
                    );
                    tmux.mouse(0, column, row, true);
                    tmux.wait_for(|capture| {
                        capture.contains(title) && !capture.contains("enter select")
                    });
                    tmux.mouse(0, column, row, true);
                    tmux.wait_for(|capture| capture.contains("enter select"));
                    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
                    tmux.wait_for(|capture| {
                        capture.contains(title)
                            && !capture.contains("enter select")
                            && capture
                                .lines()
                                .any(|line| line.contains(model) && line.contains(effort))
                    });
                }
                for value in ["source", "main"] {
                    let capture = tmux.capture_when(|capture| {
                        capture.lines().any(|line| {
                            line.contains(title) && line.contains("source") && line.contains("main")
                        }) && !capture.contains("enter select")
                    });
                    let (row, line) = capture
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains(title) && line.contains(value))
                        .unwrap();
                    let column = line[..line.find(value).unwrap()].width();
                    tmux.mouse(0, column, row, true);
                    tmux.wait_for(|capture| capture.contains("enter select"));
                    tmux.mouse(0, column, row, true);
                    tmux.wait_for(|capture| {
                        !capture.contains("enter select")
                            && capture.lines().any(|line| {
                                line.contains(title)
                                    && line.contains("source")
                                    && line.contains("main")
                            })
                    });
                }
                checks.push(serde_json::json!({"width":width,"model":model,"effort":effort,"named":named,"clickable":true,"inherit":true,"same_field_cancel":true}));
            }
        }
    }
    let verdict = serde_json::json!({"verdict":"pass","checks":checks});
    std::fs::write(
        std::env::temp_dir().join("agents-draft-defaults-verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_picker_bottom_layout() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    repository(&source, "main");
    let output = Command::new("git")
        .args(["branch", "feature/layout"])
        .current_dir(&source)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let (_directory, tmux, _) =
        start_probe_with_setup("picker-bottom", &transcript_at(&source), |directory| {
            use sha2::{Digest, Sha256};
            let config = directory.join("claude");
            std::fs::write(
                config.join(".claude.json"),
                r#"{"oauthAccount":{"organizationUuid":"org-1","accountUuid":"ACCOUNT-1"}}"#,
            )
            .unwrap();
            let hash = Sha256::digest(b"account-1")
                .iter()
                .take(6)
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let cache = config.join("cache/model-catalog");
            std::fs::create_dir_all(&cache).unwrap();
            std::fs::write(
                cache.join(format!("org-1-{hash}-cc.json")),
                include_str!("../fixtures/claude-model-catalog.json"),
            )
            .unwrap();
        });
    tmux.run(&["send-keys", "-t", "probe", "n"]);
    let capture = assert_title_caret(&tmux, "New task", 0);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("New task") && line.contains("main"))
        .unwrap();
    tmux.mouse(0, line[..line.find("main").unwrap()].width(), row, true);
    let capture = tmux.capture_when(|capture| capture.contains("Create new branch"));
    let lines: Vec<_> = capture.lines().collect();
    let create = lines
        .iter()
        .position(|line| line.contains("Create new branch"))
        .unwrap();
    let branch = lines
        .iter()
        .position(|line| line.split('│').any(|part| part.trim() == "main"))
        .unwrap();
    assert_eq!(create - branch, 2, "{capture}");
    assert!(
        lines[create - 1]
            .split('│')
            .next_back()
            .unwrap()
            .trim()
            .is_empty(),
        "{capture}"
    );
    assert!(lines[create + 1].contains("New task"), "{capture}");
    println!(
        "BRANCH CAPTURE\n{}",
        lines[branch - 1..=create + 1].join("\n")
    );
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    let capture = assert_title_caret(&tmux, "New task", 0);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Opus 5.5") && line.contains("medium"))
        .unwrap();
    tmux.mouse(0, line[..line.find("Opus 5.5").unwrap()].width(), row, true);
    let capture =
        tmux.capture_when(|capture| capture.contains("enter select") && capture.contains("Sonnet"));
    let lines: Vec<_> = capture.lines().collect();
    let bottom = lines
        .iter()
        .position(|line| line.contains("New task") && line.contains('─'))
        .unwrap();
    assert!(
        lines[bottom - 1]
            .split('│')
            .any(|part| part.trim() == "Sonnet 4.6"),
        "{capture}"
    );
    println!("MODEL CAPTURE\n{}", lines[bottom - 5..=bottom].join("\n"));
    println!("{{\"picker_bottom\":\"pass\",\"branch_gap\":\"pass\"}}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_title_blur_saves_and_unchanged_enter_closes() {
    let (_directory, tmux, _transcript) = start_probe("title-blur-save");
    tmux.run(&["send-keys", "-t", "probe", "r"]);
    tmux.run(&["send-keys", "-t", "probe", "End", "C-u"]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "Saved on title blur"]);
    assert_title_caret(&tmux, "Saved on title blur", 19);
    tmux.run(&["send-keys", "-t", "probe", "C-w"]);
    tmux.run(&["send-keys", "-t", "probe", "h"]);
    let saved = tmux.capture_when(|capture| {
        title_rule_position(capture, "Saved on title blur").is_some()
            && capture
                .lines()
                .any(|line| line.contains("Saved on title blur") && !line.contains('─'))
    });
    assert!(!saved.contains("Rename ❯"));
    tmux.run(&["send-keys", "-t", "probe", "r"]);
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    assert_title_caret(&tmux, "Saved on title blur", 0);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    tmux.capture_when(|capture| {
        title_rule_position(capture, "Saved on title blur").is_some()
            && tmux
                .run(&["display-message", "-t", "probe", "-p", "#{cursor_flag}"])
                .trim()
                == "0"
    });
    let verdict = serde_json::json!({
        "verdict": "pass",
        "title_blur_saved": true,
        "unchanged_enter_closed": true,
    });
    std::fs::write(
        std::env::temp_dir().join("agents-title-blur-verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

fn assert_draft_prompt_caret(tmux: &Tmux, name: &str, offset: usize) {
    let prefix = format!("{name} ❯");
    tmux.capture_when(|capture| {
        let Some((row, line)) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(&prefix))
        else {
            return false;
        };
        let column = line[..line.find(&prefix).unwrap()].width() + prefix.width() + 1 + offset;
        let cursor = tmux.run(&[
            "display-message",
            "-t",
            "probe",
            "-p",
            "#{cursor_x},#{cursor_y},#{cursor_flag}",
        ]);
        cursor.trim() == format!("{column},{row},1")
    });
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_draft_wizard_focus_order_and_clicks() {
    use std::os::unix::fs::PermissionsExt;
    let mut checks = Vec::new();
    for width in [120, 200] {
        for out_of_order in [false, true] {
            println!("wizard width={width} out_of_order={out_of_order}: start");
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("source");
            repository(&source, "main");
            let started = root.path().join("checkout-started");
            let hook = source.join(".git/hooks/post-checkout");
            // A real failed checkout proves creation reached git without starting a provider.
            std::fs::write(&hook, format!(
                "#!/usr/bin/env python3\nimport pathlib, sys\npathlib.Path({}).touch()\nprint('wizard checkout rejected', file=sys.stderr)\nsys.exit(1)\n",
                serde_json::to_string(started.to_str().unwrap()).unwrap(),
            )).unwrap();
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
            let (directory, tmux, _) = start_probe_with_setup(
                "wizard-focus",
                &transcript_at(&source),
                |directory| {
                    std::fs::write(directory.join("claude/agents.json"), serde_json::to_vec(
                        &serde_json::json!({"config":{"worktree_root":root.path().join("trees"),"grouping":"project"},"state":{"pinned":[]}})
                    ).unwrap()).unwrap();
                },
            );
            tmux.run(&[
                "resize-window",
                "-t",
                "probe",
                "-x",
                &width.to_string(),
                "-y",
                "50",
            ]);
            tmux.run(&["send-keys", "-t", "probe", "n"]);
            assert_title_caret(&tmux, "New task", 0);
            tmux.run(&["send-keys", "-t", "probe", "-l", "Wizard task"]);
            assert_title_caret(&tmux, "Wizard task", 11);
            if out_of_order {
                let capture = tmux
                    .capture_when(|capture| title_rule_position(capture, "Wizard task").is_some());
                let (row, line) = capture
                    .lines()
                    .enumerate()
                    .find(|(_, line)| {
                        line.contains("Wizard task") && line.contains("main") && line.contains('─')
                    })
                    .unwrap();
                tmux.mouse(0, line[..line.find("main").unwrap()].width(), row, true);
            } else {
                tmux.run(&["send-keys", "-t", "probe", "Enter"]);
                tmux.wait_for(|capture| {
                    capture
                        .lines()
                        .any(|line| line.split('│').any(|part| part.trim() == ".."))
                });
                tmux.run(&["send-keys", "-t", "probe", "Enter"]);
            }
            tmux.wait_for(|capture| capture.contains("Create new branch"));
            tmux.run(&["send-keys", "-t", "probe", "End"]);
            tmux.run(&["send-keys", "-t", "probe", "Enter"]);
            tmux.wait_for(|capture| capture.contains("New branch ❯"));
            tmux.run(&["send-keys", "-t", "probe", "-l", "foo"]);
            tmux.run(&["send-keys", "-t", "probe", "Enter"]);
            tmux.wait_for(|capture| {
                !capture.contains("New branch ❯") && !capture.contains("Create new branch")
            });
            println!("wizard width={width} out_of_order={out_of_order}: branch saved");
            let name = if out_of_order {
                let capture = tmux
                    .capture_when(|capture| title_rule_position(capture, "Wizard task").is_some());
                let (column, row) = title_rule_position(&capture, "Wizard task").unwrap();
                tmux.mouse(0, column, row, true);
                tmux.mouse(0, column, row, true);
                assert_title_caret(&tmux, "Wizard task", 11);
                tmux.run(&["send-keys", "-t", "probe", "C-u"]);
                tmux.run(&["send-keys", "-t", "probe", "-l", "Renamed wizard"]);
                tmux.run(&["send-keys", "-t", "probe", "Enter"]);
                "Renamed wizard"
            } else {
                "Wizard task"
            };
            let capture = tmux.capture_when(|capture| capture.contains(&format!("{name} ❯")));
            assert!(!capture.contains("wizard prompt"), "{capture}");
            if out_of_order {
                let prefix = format!("{name} ❯");
                let (row, line) = capture
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains(&prefix))
                    .unwrap();
                let column = line[..line.find(&prefix).unwrap()].width() + prefix.width() + 1;
                tmux.mouse(0, column, row, true);
            }
            println!("wizard width={width} out_of_order={out_of_order}: prompt caret");
            assert_draft_prompt_caret(&tmux, name, 0);
            tmux.run(&["send-keys", "-t", "probe", "-l", "wizard prompt"]);
            tmux.wait_for(|capture| capture.contains(&format!("{name} ❯ wizard prompt")));
            assert_draft_prompt_caret(&tmux, name, "wizard prompt".width());
            let document: serde_json::Value = serde_json::from_slice(
                &std::fs::read(directory.path().join("claude/agents.json")).unwrap(),
            )
            .unwrap();
            let draft = &document["state"]["drafts"][0];
            assert_eq!(draft["name"], name);
            assert_eq!(draft["branch"], serde_json::json!({"new":"foo"}));
            tmux.run(&["send-keys", "-t", "probe", "Enter"]);
            tmux.wait_for(|capture| {
                capture.contains("wizard checkout rejected")
                    && capture.contains(&format!("{name} ❯ wizard prompt"))
            });
            assert!(started.exists());
            let document: serde_json::Value = serde_json::from_slice(
                &std::fs::read(directory.path().join("claude/agents.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                document["state"]["drafts"][0]["input"]["text"],
                "wizard prompt"
            );
            checks.push(serde_json::json!({"width":width,"out_of_order":out_of_order,"name_focus":true,"branch":"foo","prompt_focus":true,"create_reached_git":true}));
        }
    }
    let verdict = serde_json::json!({"verdict":"pass","checks":checks});
    if let Some(directory) = std::env::var_os("AGENTS_PROBE_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join("wizard-verdict.json"),
            serde_json::to_vec_pretty(&verdict).unwrap(),
        )
        .unwrap();
    }
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_draft_escape_follows_details_and_prompt_focus() {
    let mut checks = Vec::new();
    for width in [120, 200] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        repository(&source, "main");
        let (directory, tmux, _) =
            start_probe_with_transcript("draft-escape-focus", &transcript_at(&source));
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        begin_named_draft(&tmux, "Escape focus");
        let capture = tmux.capture_when(|capture| {
            title_rule_position(capture, "Escape focus").is_some()
                && !capture.contains("enter select")
        });
        assert!(!capture.contains("enter select"), "{capture}");
        let (row, line) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| {
                line.contains("Escape focus") && line.contains("main") && line.contains('─')
            })
            .unwrap();
        tmux.mouse(0, line[..line.find("main").unwrap()].width(), row, true);
        tmux.wait_for(|capture| capture.contains("Create new branch"));
        // Closing the branch selector leaves Details active with no child UI open.
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| !capture.contains("enter select"));
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| capture.contains("Permanently delete “Escape focus”"));
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        let capture = tmux.capture_when(|capture| {
            !capture.contains("Permanently delete “")
                && title_rule_position(capture, "Escape focus").is_some()
        });
        assert!(!capture.contains("enter select"), "{capture}");
        let (row, line) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| {
                line.contains("Escape focus") && line.contains("main") && line.contains('─')
            })
            .unwrap();
        tmux.mouse(0, line[..line.find("main").unwrap()].width(), row, true);
        let capture = tmux.capture_when(|capture| capture.contains("Create new branch"));
        let prefix = "Escape focus ❯";
        assert!(capture.contains(prefix), "{capture}");
        tmux.run(&["send-keys", "-t", "probe", "C-w"]);
        tmux.run(&["send-keys", "-t", "probe", "j"]);
        assert_draft_prompt_caret(&tmux, "Escape focus", 0);
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(capture.contains("Create new branch"), "{capture}");
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| capture.contains("Permanently delete “Escape focus”"));
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| {
            !capture.contains("Permanently delete “")
                && title_rule_position(capture, "Escape focus").is_some()
        });
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let (row, line) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| {
                line.contains("Escape focus") && line.contains("main") && line.contains('─')
            })
            .unwrap();
        tmux.mouse(0, line[..line.find("main").unwrap()].width(), row, true);
        click_text(&tmux, "Create new branch");
        tmux.wait_for(|capture| capture.contains("New branch ❯"));
        tmux.run(&["send-keys", "-t", "probe", "-l", "foo"]);
        let capture = tmux.capture_when(|capture| capture.contains("New branch ❯ foo"));
        let (row, line) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(prefix))
            .unwrap();
        let column = line[..line.find(prefix).unwrap()].width() + prefix.width() + 1;
        tmux.mouse(0, column, row, true);
        assert_draft_prompt_caret(&tmux, "Escape focus", 0);
        let capture = tmux.capture_when(|capture| !capture.contains("New branch ❯"));
        let document: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("claude/agents.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            document["state"]["drafts"][0]["branch"],
            serde_json::json!({"new":"foo"})
        );
        let (row, line) = capture
            .lines()
            .enumerate()
            .find(|(_, line)| {
                line.contains("Escape focus") && line.contains("foo") && line.contains('─')
            })
            .unwrap();
        tmux.mouse(0, line[..line.find("foo").unwrap()].width(), row, true);
        click_text(&tmux, "foo");
        tmux.wait_for(|capture| capture.contains("New branch ❯ foo"));
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| {
            !capture.contains("New branch ❯") && capture.contains("enter select")
        });
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| !capture.contains("enter select"));
        checks.push(serde_json::json!({"width":width,"details_empty_escape":"delete_confirmation","prompt_with_branch_selector_escape":"delete_confirmation","cancel_preserves_draft":true,"branch_blur_saved":"foo","branch_reopen_prefilled":true,"branch_escape_layers":true}));
    }
    let verdict = serde_json::json!({"verdict":"pass","checks":checks});
    if let Some(directory) = std::env::var_os("AGENTS_PROBE_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join("escape-verdict.json"),
            serde_json::to_vec_pretty(&verdict).unwrap(),
        )
        .unwrap();
    }
    println!("{verdict}");
}
