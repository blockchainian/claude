use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

#[path = "support/tmux.rs"]
mod tmux;

type Dot = (u16, u16, String, vt100::Color);
type Capture = (String, Vec<Dot>);

fn capture(probe: &tmux::Tmux, output: &std::path::Path, name: &str) -> Capture {
    let ansi = probe.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    fs::write(output.join(format!("{name}.ansi")), &ansi).unwrap();
    let mut parser = vt100::Parser::new(40, 200, 0);
    for (y, row) in ansi.lines().enumerate() {
        parser.process(format!("\x1b[{};1H{row}", y + 1).as_bytes());
    }
    let mut dots = Vec::new();
    for y in 0..40 {
        for x in 0..120 {
            let cell = parser.screen().cell(y, x).unwrap();
            let text = cell.contents().to_owned();
            if x < 60
                && (5..13).contains(&y)
                && text.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
            {
                dots.push((x, y, text, cell.fgcolor()));
            }
        }
    }
    (ansi, dots)
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_usage_chart_stacks_active_tasks_and_preserves_search_layout() {
    let output = std::env::var_os("AGENTS_USAGE_CHART_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-usage-chart-terminal"));
    fs::create_dir_all(&output).unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    let outcome = std::panic::catch_unwind(|| {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("claude");
        let project = config.join("projects/project");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(config.join("sessions")).unwrap();
        let codex = directory.path().join("codex");
        fs::create_dir(&codex).unwrap();
        let started = Command::new("ps")
            .env("TZ", "UTC")
            .args(["-p", &std::process::id().to_string(), "-o", "lstart="])
            .output()
            .unwrap();
        assert!(started.status.success());
        let now = chrono::Utc::now();
        for (id, name, tokens) in [
            ("4165651d-b9e3-458d-8612-f8114e312363", "Stack alpha", 2000),
            ("5165651d-b9e3-458d-8612-f8114e312363", "Stack beta", 3000),
        ] {
            let mut lines: Vec<_> = include_str!("fixtures/transcript.jsonl")
                .lines()
                .take(2)
                .enumerate()
                .map(|(index, line)| {
                    let mut record: Value = serde_json::from_str(line).unwrap();
                    record["sessionId"] = id.into();
                    record["cwd"] = directory.path().to_str().unwrap().into();
                    record["timestamp"] = (now
                        - chrono::Duration::seconds(if index == 0 { 30 } else { 20 }))
                    .to_rfc3339()
                    .into();
                    if index == 0 {
                        record["message"]["content"] = name.into();
                    } else {
                        record["message"]["content"] =
                            serde_json::json!([{"type":"text","text":name}]);
                        record["message"]["usage"]["input_tokens"] = tokens.into();
                        record["message"]["usage"]["cache_creation_input_tokens"] = 0.into();
                        record["message"]["usage"]["cache_read_input_tokens"] = 0.into();
                        record["message"]["usage"]["output_tokens"] = tokens.into();
                    }
                    record.to_string()
                })
                .collect();
            let mut ended: Value = include_str!("fixtures/turn-groups/claude-wakeup.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|record| record["subtype"] == "turn_duration")
                .unwrap();
            ended["sessionId"] = id.into();
            ended["cwd"] = directory.path().to_str().unwrap().into();
            ended["timestamp"] = (now - chrono::Duration::seconds(15)).to_rfc3339().into();
            ended["durationMs"] = 15_000.into();
            lines.push(ended.to_string());
            fs::write(project.join(format!("{id}.jsonl")), lines.join("\n") + "\n").unwrap();
            let mut registry: Value =
                serde_json::from_str(include_str!("fixtures/registry.json")).unwrap();
            registry["pid"] = std::process::id().into();
            registry["procStart"] = String::from_utf8(started.stdout.clone())
                .unwrap()
                .trim()
                .into();
            registry["sessionId"] = id.into();
            registry["cwd"] = directory.path().to_str().unwrap().into();
            registry["name"] = name.into();
            registry["status"] = "idle".into();
            registry["updatedAt"] = now.timestamp_millis().into();
            fs::write(
                config.join(format!("sessions/{id}.json")),
                registry.to_string(),
            )
            .unwrap();
        }
        let binary = std::env::var_os("AGENTS_PROBE_BIN")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
        let binary = std::path::Path::new(&binary).canonicalize().unwrap();
        let probe = tmux::Tmux::new(format!("agents-usage-chart-{}", std::process::id()));
        probe.run(&[
            "new-session",
            "-d",
            "-s",
            "probe",
            "-x",
            "120",
            "-y",
            "40",
            "-e",
            &format!("CLAUDE_CONFIG_DIR={}", config.display()),
            "-e",
            &format!("CODEX_HOME={}", codex.display()),
            "-e",
            &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
            binary.to_str().unwrap(),
        ]);
        let wait = |name: &str, condition: &dyn Fn(&str, &[Dot]) -> bool| {
            loop {
                let result = capture(&probe, &output, name);
                if condition(&result.0, &result.1) {
                    break result;
                }
                assert!(Instant::now() < deadline, "{name} timed out: {}", result.0);
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        wait("discovery", &|text, dots| {
            text.contains("Stack alpha")
                && text.contains("Stack beta")
                && dots.iter().any(|cell| cell.3 == vt100::Color::Idx(3))
                && dots
                    .iter()
                    .any(|cell| cell.3 != vt100::Color::Idx(3) && cell.3 != vt100::Color::Default)
        });
        probe.run(&["send-keys", "-t", "probe", "Home"]);
        let (_, before) = wait("active", &|text, dots| {
            text.lines()
                .nth(3)
                .is_some_and(|row| row.contains("[48;5;233m"))
                && dots.iter().any(|cell| cell.3 == vt100::Color::Idx(3))
        });
        assert!(
            before.iter().any(|upper| upper.3 == vt100::Color::Idx(3)
                && before.iter().any(|lower| {
                    lower.0 == upper.0
                        && lower.1 > upper.1
                        && lower.3 != vt100::Color::Idx(3)
                        && lower.3 != vt100::Color::Default
                })),
            "selected yellow must be above the other active task's dots"
        );
        let shape = |dots: &[Dot]| {
            dots.iter()
                .map(|(x, y, text, _)| (*x, *y, text.clone()))
                .collect::<Vec<_>>()
        };
        probe.run(&["send-keys", "-t", "probe", "Down"]);
        let (_, switched) = wait("switched", &|_, dots| {
            shape(dots) == shape(&before) && dots != before.as_slice()
        });
        assert_eq!(shape(&before), shape(&switched));
        probe.run(&["send-keys", "-t", "probe", "/"]);
        let (_, search) = wait("search", &|text, _| text.contains("Search"));
        assert!(
            search.iter().all(|cell| switched.contains(cell)),
            "search may cover dots but cannot move them"
        );
        probe.run(&["send-keys", "-t", "probe", "Escape"]);
        wait("restored", &|_, dots| dots == switched.as_slice());
        for width in [70, 200] {
            probe.run(&[
                "resize-window",
                "-t",
                "probe",
                "-x",
                &width.to_string(),
                "-y",
                "40",
            ]);
            let (ansi, _) = wait(&format!("rules-{width}"), &|text, _| {
                let mut parser = vt100::Parser::new(40, 200, 0);
                for (y, line) in text.lines().enumerate() {
                    parser.process(format!("\x1b[{};1H{line}", y + 1).as_bytes());
                }
                parser
                    .screen()
                    .rows(0, 200)
                    .next()
                    .unwrap()
                    .trim_end()
                    .chars()
                    .count()
                    == width - 2
            });
            let mut parser = vt100::Parser::new(40, 200, 0);
            for (y, line) in ansi.lines().enumerate() {
                parser.process(format!("\x1b[{};1H{line}", y + 1).as_bytes());
            }
            for line in parser.screen().rows(0, width as u16) {
                if line.contains('─') || line.contains("Ready") {
                    assert!(
                        !line.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)),
                        "rule or gap contains dots at width {width}: {line}"
                    );
                }
            }
        }
        serde_json::json!({"verdict":"pass","active_tasks":2,"stack_colors":true,"selection_preserves_total_shape":true,"search_roundtrip":true,"rules_without_dots":[70,200]})
    });
    let verdict = match &outcome {
        Ok(value) => value.clone(),
        Err(_) => serde_json::json!({"verdict":"fail","captures":output}),
    };
    fs::write(
        output.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
