use super::*;

fn capture(tmux: &Tmux) -> String {
    tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"])
}

fn assert_covered(tmux: &Tmux, area: (usize, usize, usize, usize), name: &str) {
    let ansi = capture(tmux);
    let screen = captured_screen(&ansi, 200, 50);
    for y in area.1..area.3 {
        for x in area.0..area.2 {
            assert!(
                !screen
                    .screen()
                    .cell(y as u16, x as u16)
                    .unwrap()
                    .underline(),
                "{name} has underline at ({x}, {y}): {ansi:?}"
            );
        }
    }
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_link_overlays_cover_underlines_and_blank_clicks() {
    use sha2::{Digest, Sha256};
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/overlay-probe");
    std::fs::create_dir_all(&output).unwrap();
    let body = (0..30)
        .map(|index| format!("[overlay-link-{index:02}](https://example.org/overlay/{index})"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let records = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            match record["type"].as_str() {
                Some("user") => record["message"]["content"] = body.clone().into(),
                Some("assistant") => record["message"]["content"][0]["text"] = body.clone().into(),
                _ => {}
            }
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (directory, tmux, _) = start_probe_with_setup("link-overlays", &records, |root| {
        let config = root.join("claude");
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
    let config = directory.path().join("claude");
    let mut registry: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/registry.json")).unwrap();
    registry["pid"] = std::process::id().into();
    registry["sessionId"] = "4165651d-b9e3-458d-8612-f8114e312363".into();
    registry["kind"] = "bg".into();
    registry["jobId"] = "overlay-probe".into();
    registry["status"] = "idle".into();
    let start = Command::new("ps")
        .args(["-p", &std::process::id().to_string(), "-o", "lstart="])
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(start.status.success());
    registry["procStart"] = String::from_utf8(start.stdout).unwrap().trim().into();
    std::fs::create_dir_all(config.join("sessions")).unwrap();
    std::fs::write(config.join("sessions/overlay.json"), registry.to_string()).unwrap();
    let baseline =
        tmux.capture_when(|text| text.contains("Active 1") && text.contains("overlay-link-00"));
    std::fs::write(output.join("baseline.txt"), &baseline).unwrap();
    let clipboard = directory.path().join("clipboard/text");
    let sentinel = "overlay clipboard sentinel";
    std::fs::write(&clipboard, sentinel).unwrap();
    let mut checks = Vec::new();
    for value in ["Opus 5.5", "medium"] {
        let baseline = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        let (row, line) = baseline
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Opus 5.5") && line.contains("medium"))
            .unwrap();
        tmux.mouse(0, line[..line.find(value).unwrap()].width(), row, true);
        let marker = if value == "Opus 5.5" {
            "Fable 5.1"
        } else {
            "xhigh"
        };
        let picker = tmux.capture_when(|text| text.contains(marker));
        std::fs::write(output.join(format!("{value}.txt")), &picker).unwrap();
        let (link_y, link_line) = baseline
            .lines()
            .enumerate()
            .find(|(_, line)| {
                line.find("overlay-link-00")
                    .is_some_and(|byte| line[..byte].width() < 100)
            })
            .unwrap();
        let link_x = link_line[..link_line.find("overlay-link-00").unwrap()].width();
        let left = link_line[..link_line.find("overlay-link-00").unwrap()]
            .rfind('│')
            .map(|byte| link_line[..byte].width() + 1)
            .unwrap_or(0);
        let right = link_line[link_line.find("overlay-link-00").unwrap()..]
            .find('│')
            .map(|byte| link_line[..link_line.find("overlay-link-00").unwrap() + byte].width())
            .unwrap_or(200);
        assert_covered(&tmux, (left, 2, right, 29), value);
        tmux.mouse(0, link_x, link_y + 20, true);
        tmux.mouse(0, link_x, link_y + 20, true);
        assert_eq!(std::fs::read_to_string(&clipboard).unwrap(), sentinel);
        let after = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert!(
            after.contains(marker) && !after.contains("Copied"),
            "{after}"
        );
        tmux.run(&["send-keys", "-t", "probe", "Escape"]);
        tmux.wait_for(|text| !text.contains(marker));
        checks.push(
            serde_json::json!({"overlay":value,"underlines":false,"clipboard_unchanged":true}),
        );
    }
    tmux.run(&["send-keys", "-t", "probe", "?"]);
    let help = tmux.capture_when(|text| text.contains("Previous") && text.contains("→/⌃l"));
    std::fs::write(output.join("help.txt"), &help).unwrap();
    assert_covered(&tmux, (1, 2, 199, 48), "help");
    tmux.mouse(0, 80, 40, true);
    tmux.mouse(0, 80, 40, true);
    assert_eq!(std::fs::read_to_string(&clipboard).unwrap(), sentinel);
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|text| text.contains("overlay-link-00"));
    checks
        .push(serde_json::json!({"overlay":"help","underlines":false,"clipboard_unchanged":true}));
    // A task-list click makes Backspace operate on the task, not the prompt.
    tmux.mouse(0, 5, 3, true);
    tmux.run(&["send-keys", "-t", "probe", "BSpace"]);
    let confirm = tmux.capture_when(|text| text.contains("Permanently delete “"));
    std::fs::write(output.join("confirm.txt"), &confirm).unwrap();
    let (top, top_line) = confirm
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains('┌'))
        .unwrap();
    let left = top_line[..top_line.find('┌').unwrap()].width();
    let right = top_line[..top_line.find('┐').unwrap()].width() + 1;
    let bottom = confirm
        .lines()
        .enumerate()
        .skip(top)
        .find(|(_, line)| line.contains('┘'))
        .unwrap()
        .0
        + 1;
    assert_covered(&tmux, (left, top, right, bottom), "confirm");
    tmux.mouse(0, left + 1, top + 1, true);
    tmux.mouse(0, left + 1, top + 1, true);
    assert_eq!(std::fs::read_to_string(&clipboard).unwrap(), sentinel);
    assert!(
        tmux.run(&["capture-pane", "-t", "probe", "-p"])
            .contains("Permanently delete “")
    );
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    checks.push(
        serde_json::json!({"overlay":"confirm","underlines":false,"clipboard_unchanged":true}),
    );
    let verdict = serde_json::json!({"verdict":"pass","checks":checks,
        "limitations":["External link launches are asserted by the in-process action tests", "Notice requires unsupported-terminal fixture; not covered by this Claude background probe"]});
    std::fs::write(
        output.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_confirmation_border_covers_wide_text() {
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/overlay-wide");
    std::fs::create_dir_all(&output).unwrap();
    let body = format!("{}\n\n", "中".repeat(100)).repeat(20);
    let records = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            match record["type"].as_str() {
                Some("user") => record["message"]["content"] = body.clone().into(),
                Some("assistant") => record["message"]["content"][0]["text"] = body.clone().into(),
                _ => {}
            }
            record.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (_directory, tmux, _) = start_probe_with_transcript("wide-overlay", &records);
    tmux.run(&["resize-window", "-t", "probe", "-x", "202", "-y", "50"]);
    tmux.wait_for(|text| text.lines().any(|line| line.width() == 202));
    for _ in 0..12 {
        tmux.mouse(65, 80, 5, false);
    }
    let baseline = capture(&tmux);
    std::fs::write(output.join("before.ansi"), &baseline).unwrap();
    let before = captured_screen(&baseline, 202, 50);
    tmux.mouse(0, 5, 3, true);
    tmux.run(&["send-keys", "-t", "probe", "BSpace"]);
    let dialog = tmux.capture_when(|text| text.contains("Permanently delete “"));
    std::fs::write(output.join("dialog.txt"), &dialog).unwrap();
    let ansi = capture(&tmux);
    std::fs::write(output.join("dialog.ansi"), &ansi).unwrap();
    let screen = captured_screen(&ansi, 202, 50);
    let top = dialog.lines().position(|line| line.contains('┐')).unwrap();
    let bottom = dialog.lines().position(|line| line.contains('┘')).unwrap();
    let left = 61;
    let mut crossings = 0;
    for y in top..=bottom {
        if before.screen().cell(y as u16, left - 1).unwrap().is_wide() {
            assert!(
                before
                    .screen()
                    .cell(y as u16, left)
                    .unwrap()
                    .is_wide_continuation()
            );
            crossings += 1;
            let border = if y == top {
                "┌"
            } else if y == bottom {
                "└"
            } else {
                "│"
            };
            assert_eq!(
                screen.screen().cell(y as u16, left).unwrap().contents(),
                border,
                "border at {left},{y}"
            );
        }
    }
    assert!(
        crossings > 0,
        "Chinese must cross the dialog's left edge: {baseline}"
    );
    tmux.run(&["send-keys", "-t", "probe", "Escape"]);
    tmux.wait_for(|text| !text.contains("Permanently delete “"));
    let restored = captured_screen(&capture(&tmux), 202, 50);
    for y in top..=bottom {
        if before.screen().cell(y as u16, left - 1).unwrap().is_wide() {
            assert_eq!(
                restored
                    .screen()
                    .cell(y as u16, left - 1)
                    .unwrap()
                    .contents(),
                "中"
            );
        }
    }
    let verdict = serde_json::json!({"verdict":"pass", "wide_crossings":crossings, "border_visible":true, "background_restored":true});
    std::fs::write(
        output.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
