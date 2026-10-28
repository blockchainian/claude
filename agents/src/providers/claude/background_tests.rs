use super::*;
use chrono::{DateTime, Utc};
use std::os::unix::fs::symlink;
use std::process::Command;
use std::time::{Duration, Instant};

const RECORDS: &str = include_str!("../../../tests/fixtures/background/records.jsonl");

struct HoldingProcess {
    child: std::process::Child,
    outputs: [PathBuf; 2],
}

impl HoldingProcess {
    fn start(stdout: &Path, stderr: &Path) -> Self {
        let outputs = [
            stdout.canonicalize().unwrap(),
            stderr.canonicalize().unwrap(),
        ];
        let process = Self {
            child: Command::new("sleep")
                .arg("300")
                .stdout(fs::OpenOptions::new().append(true).open(stdout).unwrap())
                .stderr(fs::OpenOptions::new().append(true).open(stderr).unwrap())
                .spawn()
                .unwrap(),
            outputs,
        };
        wait_for("child process to hold its output descriptors", || {
            let held = Processes.held_outputs().unwrap();
            Processes.alive(process.child.id()).unwrap()
                && process.outputs.iter().all(|path| held.contains(path))
        });
        process
    }

    fn stop(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
        wait_for("reaped child to release its output descriptors", || {
            let held = Processes.held_outputs().unwrap();
            !Processes.alive(self.child.id()).unwrap()
                && self.outputs.iter().all(|path| !held.contains(path))
        });
    }
}

impl Drop for HoldingProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn wait_for(description: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Claude keeps a session's task outputs under the project it started in, which no later cwd matches.
fn started_directory(tasks: &Tasks, session: &Session) -> PathBuf {
    tasks
        .root
        .join("-started-elsewhere")
        .join(&session.id.id)
        .join("tasks")
}

#[test]
fn files_control_liveness_and_descriptions_survive_transcript_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut tasks = Tasks::new(root.join("tasks-root"));
    let config = root.join("config");
    let transcript = root.join("session.jsonl");
    let records: Vec<serde_json::Value> = RECORDS
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let id = records
        .iter()
        .find_map(|r| r["toolUseResult"]["backgroundTaskId"].as_str())
        .unwrap();
    let mut sessions = vec![super::super::session(
        "session".into(),
        transcript::Summary::default(),
        Some(transcript.clone()),
        Utc::now(),
    )];
    sessions[0].cwd = PathBuf::from("/tmp/project.with punctuation");
    let directory = started_directory(&tasks, &sessions[0]);
    fs::create_dir_all(&directory).unwrap();
    let running = directory.join(format!("{id}.output"));
    fs::write(
        &running,
        include_bytes!("../../../tests/fixtures/background/running.output"),
    )
    .unwrap();
    for (name, fixture) in [
        ("finished", "exited.output"),
        ("killed", "killed.output"),
        ("detached", "detached.output"),
    ] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/background")
                .join(fixture),
            directory.join(format!("{name}.output")),
        )
        .unwrap();
    }
    let orphan = directory.join("orphan.output");
    fs::write(&orphan, b"").unwrap();
    let foreground = directory.join("foreground.output");
    fs::write(&foreground, b"").unwrap();
    symlink(&running, directory.join("agent.output")).unwrap();
    // A complete current turn creates the existing queue quiet point after the task.
    let quiet = include_str!("../../../tests/fixtures/background/quiet-turn.jsonl");
    let foreground_records = include_str!("../../../tests/fixtures/background/foreground.jsonl");
    fs::write(
        &transcript,
        format!(
            "{RECORDS}{foreground_records}{}\n{quiet}",
            " ".repeat(150_000)
        ),
    )
    .unwrap();
    assert!(!Processes.held_outputs().unwrap().contains(&running));
    tasks.refresh(&config, &mut sessions, &Processes).unwrap();
    assert!(
        sessions[0].background.is_empty(),
        "unheld real background output is an orphan"
    );
    assert_eq!(tasks.transcript_bytes, 0);
    let mut writer = HoldingProcess::start(&running, &foreground);
    tasks.refresh(&config, &mut sessions, &Processes).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    assert_eq!(sessions[0].background[0].output, Some(running.clone()));
    let first = sessions[0].background[0].clone();
    let cost = tasks.transcript_bytes;
    assert!(cost > 0);
    // Even a stamp change must not re-read the description of a matched task.
    fs::write(&transcript, format!("{RECORDS}{quiet}{quiet}")).unwrap();
    tasks.refresh(&config, &mut sessions, &Processes).unwrap();
    assert_eq!(sessions[0].background, [first]);
    // Neither matched tasks nor foreground outputs are scanned again on a stamp change.
    let after_change = tasks.transcript_bytes;
    assert_eq!(after_change, cost);
    tasks.refresh(&config, &mut sessions, &Processes).unwrap();
    assert_eq!(tasks.transcript_bytes, after_change);
    let tails = tasks.output_bytes;
    tasks.refresh(&config, &mut sessions, &Processes).unwrap();
    assert_eq!(tasks.output_bytes, tails);
    fs::write(
        &running,
        include_bytes!("../../../tests/fixtures/background/exited.output"),
    )
    .unwrap();
    tasks.refresh(&config, &mut sessions, &Processes).unwrap();
    assert!(sessions[0].background.is_empty());
    // A finished file never becomes a candidate again, even after replacement.
    fs::write(&running, b"").unwrap();
    tasks.refresh(&config, &mut sessions, &Processes).unwrap();
    assert!(sessions[0].background.is_empty());
    writer.stop();
}

#[test]
fn bg_sessions_scan_tasks_without_adoption_and_deduplicate_adopted_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut tasks = Tasks::new(root.join("root"));
    let path = root.join("session.jsonl");
    fs::write(&path, RECORDS).unwrap();
    let records: Vec<serde_json::Value> = RECORDS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let id = records[1]["toolUseResult"]["backgroundTaskId"]
        .as_str()
        .unwrap();
    let mut session = super::super::session(
        "session".into(),
        transcript::Summary::default(),
        Some(path),
        Utc::now(),
    );
    session.process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    session.background_job = Some(crate::model::BackgroundJob {
        id: "job".into(),
        needs: None,
        awaits_prompt: false,
    });
    let directory = started_directory(&tasks, &session);
    fs::create_dir_all(&directory).unwrap();
    let output = directory.join(format!("{id}.output"));
    fs::write(
        &output,
        include_bytes!("../../../tests/fixtures/background/running.output"),
    )
    .unwrap();
    let adopted_output = root.join("adopted.output");
    fs::write(&adopted_output, b"").unwrap();
    let writer = HoldingProcess::start(&output, &adopted_output);
    let mut sessions = [session];
    let jobs = root.join("jobs/job");
    fs::create_dir_all(&jobs).unwrap();
    assert!(!jobs.join("adopt.json").exists());
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    assert_eq!(sessions[0].background[0].output, Some(output.clone()));
    assert_eq!(
        sessions[0].background[0].description,
        records[0]["message"]["content"][0]["input"]["description"]
    );
    let mut adopt: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/background/adopt.json"
    ))
    .unwrap();
    adopt["shells"][0]["pid"] = writer.child.id().into();
    adopt["shells"][0]["outputPath"] = output.to_str().unwrap().into();
    let mut separate = adopt["shells"][0].clone();
    separate["outputPath"] = adopted_output.to_str().unwrap().into();
    adopt["shells"].as_array_mut().unwrap().push(separate);
    fs::write(jobs.join("adopt.json"), adopt.to_string()).unwrap();
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert_eq!(sessions[0].background.len(), 2);
    for expected in [&output, &adopted_output] {
        assert_eq!(
            sessions[0]
                .background
                .iter()
                .filter(|task| task.output.as_ref() == Some(expected))
                .count(),
            1
        );
    }
    fs::write(
        &output,
        include_bytes!("../../../tests/fixtures/background/exited.output"),
    )
    .unwrap();
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    assert_eq!(sessions[0].background[0].output, Some(adopted_output));
}

#[test]
fn adopted_shells_use_pid_and_footer_in_the_bg_session() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut tasks = Tasks::new(root.join("tasks"));
    let mut adopt: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/background/adopt.json"
    ))
    .unwrap();
    let output = root.join("adopted.output");
    fs::write(&output, b"").unwrap();
    let mut writer = HoldingProcess::start(&output, &output);
    let shell_start = crate::processes::start_time(writer.child.id()).unwrap();
    adopt["shells"][0]["pid"] = writer.child.id().into();
    adopt["shells"][0]["outputPath"] = output.to_str().unwrap().into();
    fs::create_dir_all(root.join("jobs/50517e58")).unwrap();
    let adoption = root.join("jobs/50517e58/adopt.json");
    fs::write(&adoption, adopt.to_string()).unwrap();
    let mut session = super::super::session(
        "50517e58-041e-49c8-95e0-a41879199991".into(),
        transcript::Summary::default(),
        None,
        Utc::now(),
    );
    session.process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    session.background_job = Some(crate::model::BackgroundJob {
        id: "50517e58".into(),
        needs: None,
        awaits_prompt: false,
    });
    let mut sessions = [session];
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    assert_eq!(sessions[0].background[0].started_at, shell_start);
    assert_eq!(
        sessions[0].background[0].description,
        adopt["shells"][0]["description"]
    );
    writer.stop();
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert!(sessions[0].background.is_empty());
    let writer = HoldingProcess::start(&output, &output);
    adopt["shells"][0]["pid"] = writer.child.id().into();
    fs::write(&adoption, adopt.to_string()).unwrap();
    fs::write(
        &output,
        include_bytes!("../../../tests/fixtures/background/detached.output"),
    )
    .unwrap();
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert!(sessions[0].background.is_empty());
}

#[test]
fn notifications_hide_cached_tasks_and_background_results_retry_foreground_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut tasks = Tasks::new(root.join("root"));
    let path = root.join("session.jsonl");
    let mut sessions = [super::super::session(
        "session".into(),
        transcript::Summary::default(),
        Some(path.clone()),
        Utc::now(),
    )];
    let records: Vec<serde_json::Value> = RECORDS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let id = records[1]["toolUseResult"]["backgroundTaskId"]
        .as_str()
        .unwrap();
    let directory = started_directory(&tasks, &sessions[0]);
    fs::create_dir_all(&directory).unwrap();
    let output = directory.join(format!("{id}.output"));
    fs::write(&output, b"").unwrap();
    // Before the background tool result arrives, this is a foreground shell.
    fs::write(&path, format!("{}\n", records[0])).unwrap();
    let _writer = HoldingProcess::start(&output, &output);
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert!(sessions[0].background.is_empty());
    fs::write(&path, RECORDS).unwrap();
    let summary = transcript::read(&path, fs::metadata(&path).unwrap().len()).unwrap();
    tasks.observe("session", &summary);
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    let mut completed = transcript::Summary::default();
    completed.completed_background.insert(id.to_owned());
    tasks.observe("session", &completed);
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert!(sessions[0].background.is_empty());
}

#[test]
fn symlink_task_outputs_are_not_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut tasks = Tasks::new(root.join("root"));
    let mut sessions = [super::super::session(
        "session".into(),
        transcript::Summary::default(),
        None,
        Utc::now(),
    )];
    let directory = started_directory(&tasks, &sessions[0]);
    fs::create_dir_all(&directory).unwrap();
    let target = root.join("subagent.jsonl");
    fs::write(
        &target,
        include_bytes!("../../../tests/fixtures/subagent.jsonl"),
    )
    .unwrap();
    let link = directory.join("agent.output");
    symlink(&target, &link).unwrap();
    let _writer = HoldingProcess::start(&link, &link);
    tasks.refresh(&root, &mut sessions, &Processes).unwrap();
    assert!(sessions[0].background.is_empty());
    assert_eq!(tasks.output_bytes, 0);
}

#[test]
fn adoption_skips_unregistered_sessions_and_refreshes_registered_jobs_with_a_real_process() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("adopted.output");
    fs::write(
        &output,
        include_bytes!("../../../tests/fixtures/background/running.output"),
    )
    .unwrap();
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = Running(
        Command::new("sleep")
            .arg("15")
            .stdout(File::create(&output).unwrap())
            .spawn()
            .unwrap(),
    );
    let mut adoption: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/background/adopt.json"
    ))
    .unwrap();
    adoption["shells"][0]["pid"] = child.0.id().into();
    adoption["shells"][0]["outputPath"] = output.to_string_lossy().as_ref().into();
    let jobs = directory.path().join("jobs/distinct-job-id");
    fs::create_dir_all(&jobs).unwrap();
    fs::write(
        jobs.join("adopt.json"),
        serde_json::to_vec(&adoption).unwrap(),
    )
    .unwrap();
    let mut row = super::super::session(
        "50517e58-041e-49c8-95e0-a41879199991".into(),
        transcript::Summary::default(),
        None,
        Utc::now(),
    );
    row.process = Some(crate::model::Process {
        pid: std::process::id(),
        label: Some("bg".into()),
        started_at: None,
    });
    row.background_job = Some(crate::model::BackgroundJob {
        id: "distinct-job-id".into(),
        needs: None,
        awaits_prompt: false,
    });
    let mut unregistered = row.clone();
    unregistered.background_job = None;
    let mut rows = [unregistered, row];
    Tasks::new(directory.path().join("tasks"))
        .refresh(directory.path(), &mut rows, &Processes)
        .unwrap();
    assert!(rows[0].background.is_empty());
    assert_eq!(rows[1].background.len(), 1);
    assert_eq!(rows[1].background[0].output, Some(output));
}

#[test]
fn background_await_start_times_survive_cached_and_chunked_refreshes() {
    use std::io::Write;
    let records = include_str!("../../../tests/fixtures/background/two-awaits.jsonl");
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let mut tasks = Tasks::new(root.join("outputs"));
    let path = root.join("session.jsonl");
    fs::write(&path, records).unwrap();
    let mut sessions = [super::super::session(
        "session".into(),
        transcript::Summary::default(),
        Some(path.clone()),
        Utc::now(),
    )];
    sessions[0].status = crate::model::SessionStatus::Working;
    let directory = started_directory(&tasks, &sessions[0]);
    fs::create_dir_all(&directory).unwrap();
    struct Children(Vec<std::process::Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut children = Children(Vec::new());
    let mut expected = Vec::new();
    let parsed: Vec<serde_json::Value> = records
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for pair in parsed.chunks_exact(2) {
        let id = pair[1]["toolUseResult"]["backgroundTaskId"]
            .as_str()
            .unwrap();
        let output = directory.join(format!("{id}.output"));
        children.0.push(
            Command::new("sleep")
                .arg("60")
                .stdout(File::create(&output).unwrap())
                .spawn()
                .unwrap(),
        );
        expected.push(BackgroundTask {
            description: pair[0]["message"]["content"][0]["input"]["description"]
                .as_str()
                .unwrap()
                .into(),
            output: Some(output),
            started_at: DateTime::parse_from_rfc3339(pair[0]["timestamp"].as_str().unwrap())
                .unwrap()
                .with_timezone(&Utc),
        });
    }
    for refresh in 0..8 {
        if refresh == 2 {
            writeln!(
                fs::OpenOptions::new().append(true).open(&path).unwrap(),
                "{}",
                include_str!("../../../tests/fixtures/background/quiet-turn.jsonl")
            )
            .unwrap();
        }
        if refresh == 4 {
            writeln!(
                fs::OpenOptions::new().append(true).open(&path).unwrap(),
                "{}",
                " ".repeat(150_000)
            )
            .unwrap();
            // A cold scan now walks chunks; the original reader keeps its descriptions.
            let mut cold = Tasks::new(root.join("outputs"));
            cold.refresh(&root, &mut sessions, &Processes).unwrap();
            assert_eq!(sessions[0].background, expected);
        }
        let summary = transcript::read(&path, fs::metadata(&path).unwrap().len()).unwrap();
        tasks.observe("session", &summary);
        tasks.refresh(&root, &mut sessions, &Processes).unwrap();
        assert_eq!(
            sessions[0].background, expected,
            "provider refresh {refresh}"
        );
    }
}
