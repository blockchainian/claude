use super::*;

fn window_key(tmux: &Tmux, direction: &str) {
    tmux.run(&["send-keys", "-t", "probe", "C-w"]);
    tmux.run(&["send-keys", "-t", "probe", direction]);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_vim_window_navigation_uses_rendered_panels() {
    let mut captures = Vec::new();
    for width in [200, 100] {
        let (_directory, tmux, _transcript) = start_probe(&format!("window-navigation-{width}"));
        tmux.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            "50",
        ]);
        tmux.wait_for(|capture| {
            capture
                .lines()
                .nth(2)
                .is_some_and(|line| line.matches('│').count() == if width == 200 { 2 } else { 1 })
        });
        // Start in the task list, using the same mouse event as an actual click.
        tmux.mouse(0, 2, 2, true);
        window_key(&tmux, "l");
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        // Details ignores slash, so the following question mark opens help.
        // In the task list it would be inserted into the search editor instead.
        tmux.run(&["send-keys", "-t", "probe", "?"]);
        let details = tmux.capture_when(|capture| capture.contains("Task shortcuts"));
        assert!(!details.contains("Search ❯"), "details focus: {details}");
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|capture| !capture.contains("Task shortcuts"));

        window_key(&tmux, "j");
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        let prompt = tmux.capture_when(|capture| capture.contains("❯ /"));
        assert!(!prompt.contains("Search ❯"), "prompt focus: {prompt}");
        tmux.run(&["send-keys", "-t", "probe", "-l", "alpha beta"]);
        tmux.wait_for(|capture| capture.contains("❯ /alpha beta"));
        window_key(&tmux, "x");
        tmux.wait_for(|capture| capture.contains("❯ /alpha x"));
        tmux.run(&["send-keys", "-t", "probe", "C-u"]);
        tmux.wait_for(|capture| !capture.contains("❯ /"));
        if width == 200 {
            window_key(&tmux, "k");
            window_key(&tmux, "l");
        } else {
            window_key(&tmux, "j");
        }
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
        let list = tmux.capture_when(|capture| capture.contains("Search ❯ Stop"));
        let list_search_row = list
            .lines()
            .position(|line| line.contains("Search ❯ Stop"))
            .unwrap();
        // A non-direction key replays Ctrl-W's word deletion in the active editor.
        window_key(&tmux, "x");
        tmux.wait_for(|capture| capture.contains("Search ❯ x"));

        window_key(&tmux, "j");
        if width == 200 {
            // The full-width prompt lies between the top log list and log details.
            window_key(&tmux, "j");
        }
        tmux.run(&["send-keys", "-t", "probe", "/"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "Stop"]);
        let detail = tmux.capture_when(|capture| {
            capture
                .lines()
                .enumerate()
                .any(|(row, line)| row > list_search_row && line.contains("Search ❯ Stop"))
        });
        let path = std::env::temp_dir().join(format!("agents-window-navigation-{width}.txt"));
        std::fs::write(&path, detail).unwrap();
        captures.push(path);
    }
    let verdict = serde_json::json!({
        "verdict": "pass",
        "widths": [200, 100],
        "task_list_to_details": true,
        "details_to_prompt": true,
        "log_list_to_detail": true,
        "non_direction_preserves_word_delete": true,
        "prompt_word_delete": true,
        "captures": captures,
    });
    let path = std::env::temp_dir().join("agents-window-navigation-verdict.json");
    std::fs::write(&path, format!("{verdict}\n")).unwrap();
    println!("{verdict}");
}

fn paging_transcript() -> String {
    let mut records = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let prompt = (1..=80)
        .map(|row| format!("task row {row:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    records[0]["message"]["content"] = prompt.clone().into();
    records[2]["aiTitle"] = "Paging task 00".into();
    records[3]["lastPrompt"] = prompt.into();
    let assistant = records[1].clone();
    for index in 0..40 {
        let mut record = assistant.clone();
        record["uuid"] = format!("paging-{index}").into();
        record["message"]["id"] = format!("paging-message-{index}").into();
        record["message"]["content"][0]["text"] = if index == 39 {
            (1..=80)
                .map(|row| format!("detail row {row:02}"))
                .collect::<Vec<_>>()
                .join("  \n")
        } else {
            format!("page entry {index:02}")
        }
        .into();
        record["timestamp"] = format!("2026-10-18T01:{index:02}:00Z").into();
        records.push(record);
    }
    records.iter().map(|record| format!("{record}\n")).collect()
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_active_panel_paging_stays_in_focused_panel() {
    let mut evidence = Vec::new();
    for width in [200, 100] {
        let contents = paging_transcript();
        let (_directory, tmux, _) = start_probe_with_setup(
            &format!("active-panel-paging-{width}"),
            &contents,
            |directory| {
                let project = directory.join("claude/projects/project");
                for index in 1..12 {
                    let id = format!("00000000-0000-0000-0000-{index:012}");
                    let transcript = contents
                        .lines()
                        .map(|line| {
                            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                            record["sessionId"] = id.clone().into();
                            if record["type"] == "ai-title" {
                                record["aiTitle"] = format!("Paging task {index:02}").into();
                            }
                            format!("{record}\n")
                        })
                        .collect::<String>();
                    std::fs::write(project.join(format!("{id}.jsonl")), transcript).unwrap();
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
        let initial = tmux.capture_when(|capture| {
            capture.contains("Inactive 10")
                && capture.lines().nth(2).is_some_and(|line| {
                    line.matches('│').count() == if width == 200 { 2 } else { 1 }
                })
                && capture.contains("detail row")
        });
        let rows = initial.lines().collect::<Vec<_>>();
        let separators = rows[2]
            .chars()
            .enumerate()
            .filter_map(|(column, character)| (character == '│').then_some(column))
            .collect::<Vec<_>>();
        let rules = rows
            .iter()
            .enumerate()
            .skip(2)
            .filter_map(|(row, line)| (line.matches('─').count() > width / 2).then_some(row))
            .collect::<Vec<_>>();
        assert_eq!(rules.len(), if width == 200 { 3 } else { 4 }, "{initial}");
        let task_list = (0, separators[0], 2, rules[0]);
        let task_detail = (
            separators[0] + 1,
            if width == 200 { separators[1] } else { width },
            2,
            rules[0],
        );
        let log_list = if width == 200 {
            (separators[1] + 1, width, 2, rules[0])
        } else {
            (0, width, rules[1] + 1, rules[2])
        };
        let log_detail = (0, width, rules[rules.len() - 2] + 1, rules[rules.len() - 1]);
        let slice = |capture: &str, (left, right, top, bottom): (usize, usize, usize, usize)| {
            capture
                .lines()
                .skip(top)
                .take(bottom - top)
                .map(|line| {
                    line.chars()
                        .skip(left)
                        .take(right - left)
                        .collect::<String>()
                })
                .filter(|line| {
                    line.contains("task row")
                        || line.contains("detail row")
                        || line.contains("Paging task")
                        || line.contains("page entry")
                })
                .map(|line| {
                    if let Some(start) = line.find("Paging task ") {
                        line[start..]
                            .split_whitespace()
                            .take(3)
                            .collect::<Vec<_>>()
                            .join(" ")
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
        };
        let selected_task = |capture: &str| {
            capture
                .lines()
                .find(|line| {
                    (line.contains("Paging task") || line.contains("No task"))
                        && (line.contains('❯') || line.contains('❮'))
                })
                .unwrap_or_else(|| panic!("selected task missing: {capture}"))
                .to_owned()
        };
        // Wheel activation in either details pane must keep paging out of both lists.
        for (name, region) in [("task_details", task_detail), ("log_details", log_detail)] {
            tmux.mouse(0, task_list.0 + 2, task_list.2 + 2, true);
            tmux.mouse(65, region.0 + 2, region.2 + 2, false);
            let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
            tmux.run(&["send-keys", "-t", "probe", "C-f"]);
            let forward =
                tmux.capture_when(|capture| slice(capture, region) != slice(&before, region));
            assert_eq!(
                selected_task(&before),
                selected_task(&forward),
                "{name}: task selection moved: {forward}"
            );
            assert_eq!(
                slice(&before, task_list),
                slice(&forward, task_list),
                "{name}: task list moved: {forward}"
            );
            assert_eq!(
                slice(&before, log_list),
                slice(&forward, log_list),
                "{name}: log list moved: {forward}"
            );
            tmux.run(&["send-keys", "-t", "probe", "C-b"]);
            let backward =
                tmux.capture_when(|capture| slice(capture, region) != slice(&forward, region));
            assert_eq!(
                selected_task(&before),
                selected_task(&backward),
                "{name}: task selection moved back: {backward}"
            );
            assert_eq!(
                slice(&before, task_list),
                slice(&backward, task_list),
                "{name}: task list moved back: {backward}"
            );
            assert_eq!(
                slice(&before, log_list),
                slice(&backward, log_list),
                "{name}: log list moved back: {backward}"
            );
            evidence.push(serde_json::json!({"width":width, "panel":name, "wheel_activation":true, "forward":true, "backward":true, "lists_unchanged":true}));
        }
        // Ctrl-W uses the same focus state as the click route above.
        window_key(&tmux, "k");
        if width == 200 {
            window_key(&tmux, "k");
            window_key(&tmux, "l");
        }
        let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        tmux.run(&["send-keys", "-t", "probe", "C-b"]);
        let moved =
            tmux.capture_when(|capture| slice(capture, log_list) != slice(&before, log_list));
        assert_eq!(
            slice(&before, task_list),
            slice(&moved, task_list),
            "log-list paging moved tasks: {moved}"
        );
        tmux.run(&["send-keys", "-t", "probe", "C-f"]);
        let returned =
            tmux.capture_when(|capture| slice(capture, log_list) != slice(&moved, log_list));
        assert_eq!(
            slice(&before, task_list),
            slice(&returned, task_list),
            "log-list forward paging moved tasks: {returned}"
        );
        evidence.push(serde_json::json!({"width":width, "panel":"log_list", "forward":true, "backward":true, "ctrl_w_focus":true, "task_list_unchanged":true}));
        tmux.mouse(0, task_list.0 + 2, task_list.2 + 2, true);
        let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        tmux.run(&["send-keys", "-t", "probe", "C-f"]);
        let forward = tmux.capture_when(|capture| selected_task(capture) != selected_task(&before));
        tmux.run(&["send-keys", "-t", "probe", "C-b"]);
        tmux.capture_when(|capture| selected_task(capture) != selected_task(&forward));
        evidence.push(serde_json::json!({"width":width, "panel":"task_list", "forward":true, "backward":true}));
        // Prompt does not support paging; an editor change provides the input barrier.
        tmux.mouse(0, 2, rules[0] + 1, true);
        let before = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        tmux.run(&["send-keys", "-t", "probe", "C-f", "C-b"]);
        tmux.run(&["send-keys", "-t", "probe", "-l", "paging barrier"]);
        let after = tmux.capture_when(|capture| capture.contains("❯ paging barrier"));
        for region in [task_list, task_detail, log_list, log_detail] {
            assert_eq!(
                slice(&before, region),
                slice(&after, region),
                "prompt paging escaped: {after}"
            );
        }
        tmux.run(&["send-keys", "-t", "probe", "C-u"]);
        let capture = std::env::temp_dir().join(format!("agents-active-panel-paging-{width}.txt"));
        std::fs::write(&capture, after).unwrap();
        evidence.push(serde_json::json!({"width":width, "panel":"prompt", "unsupported_paging_ignored":true, "capture":capture}));
    }
    let verdict = serde_json::json!({"verdict":"pass", "evidence":evidence});
    std::fs::write(
        std::env::temp_dir().join("agents-active-panel-paging-verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
