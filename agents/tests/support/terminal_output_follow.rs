use super::*;
use std::io::Write;

fn append_rows(path: &Path, first: usize, last: usize) {
    let mut output = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    for row in first..=last {
        writeln!(output, "live output row {row:03}").unwrap();
    }
}

fn bottom(tmux: &Tmux) {
    let click = "\x1b[<0;151;49M\x1b[<0;151;49m".repeat(2);
    tmux.run(&["send-keys", "-t", "probe", "-l", &click]);
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_output_files_follow_appends_and_preserve_reading_position() {
    let evidence =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/event-output-watch/terminal");
    std::fs::create_dir_all(&evidence).unwrap();
    let mut outcomes = Vec::new();
    for form in ["launch", "notification", "background"] {
        let mut held = None;
        let mut output_path = None;
        let (_directory, tmux, transcript) = start_probe_with_setup(
            &format!("output-follow-{form}"),
            include_str!("../fixtures/transcript.jsonl"),
            |directory| {
                let session = "4165651d-b9e3-458d-8612-f8114e312363";
                let output = directory
                    .join(format!("claude-{}", unsafe { libc::getuid() }))
                    .join("-output-follow-probe")
                    .join(session)
                    .join("tasks/birls4onj.output");
                std::fs::create_dir_all(output.parent().unwrap()).unwrap();
                std::fs::write(&output, "").unwrap();
                held = Some(
                    Command::new("sleep")
                        .arg("120")
                        .stdout(
                            std::fs::OpenOptions::new()
                                .append(true)
                                .open(&output)
                                .unwrap(),
                        )
                        .spawn()
                        .unwrap(),
                );
                let mut records = include_str!("../fixtures/transcript.jsonl")
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                    .collect::<Vec<_>>();
                let mut launch = include_str!("../fixtures/background/records.jsonl")
                    .lines()
                    .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                    .collect::<Vec<_>>();
                launch[0]["message"]["content"][0]["input"]["run_in_background"] = true.into();
                launch[0]["message"]["content"][0]["input"]["description"] =
                    "Live output probe".into();
                let result = launch[1]["message"]["content"][0]["content"]
                    .as_str()
                    .unwrap();
                let start = result.find("Output is being written to: ").unwrap()
                    + "Output is being written to: ".len();
                let end = result[start..].find(". You will be notified").unwrap() + start;
                launch[1]["message"]["content"][0]["content"] =
                    format!("{}{}{}", &result[..start], output.display(), &result[end..]).into();
                for (index, record) in launch.iter_mut().enumerate() {
                    record["timestamp"] = format!("2026-10-21T23:00:0{index}Z").into();
                }
                records.extend(launch);
                for record in &mut records {
                    record["cwd"] = "/output-follow-probe".into();
                    record["sessionId"] = session.into();
                }
                std::fs::write(
                    directory.join(format!("claude/projects/project/{session}.jsonl")),
                    records
                        .iter()
                        .map(|record| format!("{record}\n"))
                        .collect::<String>(),
                )
                .unwrap();
                output_path = Some(output);
            },
        );
        let output = output_path.unwrap();
        tmux.wait_for(|capture| capture.contains("Live output probe"));
        if form == "notification" {
            let mut record: serde_json::Value = serde_json::from_str(
                include_str!("../fixtures/claude-background-notifications.jsonl")
                    .lines()
                    .next()
                    .unwrap(),
            )
            .unwrap();
            record["content"] = record["content"]
                .as_str()
                .unwrap()
                .replace("/tmp/notification-output", output.to_str().unwrap())
                .into();
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&transcript)
                    .unwrap(),
                "{record}"
            )
            .unwrap();
            tmux.wait_for(|capture| capture.contains("睡12秒再输出"));
        } else if form == "background" {
            let capture = tmux.capture_when(|capture| {
                capture.lines().any(|line| {
                    line.chars()
                        .take(100)
                        .collect::<String>()
                        .contains("Live output probe")
                })
            });
            let row = capture
                .lines()
                .position(|line| {
                    line.chars()
                        .take(100)
                        .collect::<String>()
                        .contains("Live output probe")
                })
                .unwrap();
            tmux.mouse(0, 70, row, true);
        }
        let output_detail = |capture: &str| {
            capture
                .lines()
                .skip(31)
                .take(17)
                .collect::<Vec<_>>()
                .join("\n")
        };
        tmux.wait_for(|capture| {
            output_detail(capture).contains(if form == "notification" {
                "睡12秒再输出"
            } else {
                "Live output probe"
            })
        });
        append_rows(&output, 1, 80);
        let first =
            tmux.capture_when(|capture| output_detail(capture).contains("live output row 080"));
        std::fs::write(evidence.join(format!("{form}-first.txt")), &first).unwrap();
        append_rows(&output, 81, 90);
        tmux.wait_for(|capture| output_detail(capture).contains("live output row 090"));
        tmux.mouse(64, 150, 43, false);
        tmux.mouse(64, 150, 43, false);
        let before = tmux.capture_when(|capture| {
            !output_detail(capture).contains("live output row 090")
                && output_detail(capture).contains("live output row")
        });
        let detail = |capture: &str| {
            capture
                .lines()
                .skip(34)
                .take(14)
                .map(|line| line.replace('▕', "").trim_end().to_owned())
                .collect::<Vec<_>>()
                .join("\n")
        };
        append_rows(&output, 91, 100);
        std::thread::sleep(Duration::from_millis(500));
        let after = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        assert_eq!(
            detail(&before),
            detail(&after),
            "{form}: append moved reading position"
        );
        bottom(&tmux);
        let end =
            tmux.capture_when(|capture| output_detail(capture).contains("live output row 100"));
        std::fs::write(evidence.join(format!("{form}-end.txt")), end).unwrap();
        outcomes.push(serde_json::json!({"form":form,"empty_file_append":true,"bottom_follow":true,"scrolled_position_preserved":true}));
        let mut child = held.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }
    let verdict = serde_json::json!({"verdict":"pass","evidence":outcomes});
    std::fs::write(
        evidence.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
