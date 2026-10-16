use super::*;
use chrono::Utc;
use std::cell::Cell;
use std::os::unix::fs::symlink;
use std::process::Command;

const RECORDS: &str = include_str!("../../../tests/fixtures/background/records.jsonl");

struct Observed {
    held: HashSet<PathBuf>,
    alive: HashSet<u32>,
    scans: Cell<usize>,
}
impl Liveness for Observed {
    fn held_outputs(&self) -> io::Result<HashSet<PathBuf>> {
        self.scans.set(self.scans.get() + 1);
        Ok(self.held.clone())
    }
    fn alive(&self, pid: u32) -> io::Result<bool> {
        Ok(self.alive.contains(&pid))
    }
}

#[test]
fn files_control_liveness_and_descriptions_survive_transcript_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut tasks = Tasks::new(dir.path().join("tasks-root"));
    let config = dir.path().join("config");
    let transcript = dir.path().join("session.jsonl");
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
    let directory = tasks.directory(&sessions[0]);
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
    let mut observed = Observed {
        held: [running.clone(), foreground].into(),
        alive: HashSet::new(),
        scans: Cell::new(0),
    };
    let held = std::mem::take(&mut observed.held);
    tasks.refresh(&config, &mut sessions, &observed).unwrap();
    assert!(
        sessions[0].background.is_empty(),
        "unheld real background output is an orphan"
    );
    assert_eq!(tasks.transcript_bytes, 0);
    observed.held = held;
    tasks.refresh(&config, &mut sessions, &observed).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    assert_eq!(sessions[0].background[0].output, running);
    let first = sessions[0].background[0].clone();
    let cost = tasks.transcript_bytes;
    assert!(cost > 0);
    // Even a stamp change must not re-read the description of a matched task.
    fs::write(&transcript, format!("{RECORDS}{quiet}{quiet}")).unwrap();
    tasks.refresh(&config, &mut sessions, &observed).unwrap();
    assert_eq!(sessions[0].background, [first]);
    // Neither matched tasks nor foreground outputs are scanned again on a stamp change.
    let after_change = tasks.transcript_bytes;
    assert_eq!(after_change, cost);
    tasks.refresh(&config, &mut sessions, &observed).unwrap();
    assert_eq!(tasks.transcript_bytes, after_change);
    assert_eq!(observed.scans.get(), 4);
    let tails = tasks.output_bytes;
    tasks.refresh(&config, &mut sessions, &observed).unwrap();
    assert_eq!(tasks.output_bytes, tails);
    fs::write(
        &running,
        include_bytes!("../../../tests/fixtures/background/exited.output"),
    )
    .unwrap();
    tasks.refresh(&config, &mut sessions, &observed).unwrap();
    assert!(sessions[0].background.is_empty());
    // A finished file never becomes a candidate again, even after replacement.
    fs::write(&running, b"").unwrap();
    tasks.refresh(&config, &mut sessions, &observed).unwrap();
    assert!(sessions[0].background.is_empty());
}

#[test]
fn adopted_shells_use_pid_and_footer_in_the_bg_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut tasks = Tasks::new(dir.path().join("tasks"));
    let mut adopt: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/background/adopt.json"
    ))
    .unwrap();
    let pid = adopt["shells"][0]["pid"].as_u64().unwrap() as u32;
    let output = dir.path().join("adopted.output");
    fs::write(&output, b"").unwrap();
    adopt["shells"][0]["outputPath"] = output.to_str().unwrap().into();
    fs::create_dir_all(dir.path().join("jobs/50517e58")).unwrap();
    fs::write(
        dir.path().join("jobs/50517e58/adopt.json"),
        adopt.to_string(),
    )
    .unwrap();
    let mut session = super::super::session(
        "50517e58-041e-49c8-95e0-a41879199991".into(),
        transcript::Summary::default(),
        None,
        Utc::now(),
    );
    session.process = Some(crate::model::Process {
        pid: 1,
        label: Some("bg".into()),
        started_at: None,
    });
    session.background_job = Some(crate::model::BackgroundJob {
        id: "50517e58".into(),
        needs: None,
    });
    let mut sessions = [session];
    let observed = Observed {
        held: HashSet::new(),
        alive: [pid].into(),
        scans: Cell::new(0),
    };
    tasks.refresh(dir.path(), &mut sessions, &observed).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    assert_eq!(
        sessions[0].background[0].description,
        adopt["shells"][0]["description"]
    );
    assert_eq!(observed.scans.get(), 0);
    let dead = Observed {
        alive: HashSet::new(),
        ..observed
    };
    tasks.refresh(dir.path(), &mut sessions, &dead).unwrap();
    assert!(sessions[0].background.is_empty());
    fs::write(
        output,
        include_bytes!("../../../tests/fixtures/background/detached.output"),
    )
    .unwrap();
    tasks
        .refresh(
            dir.path(),
            &mut sessions,
            &Observed {
                alive: [pid].into(),
                ..dead
            },
        )
        .unwrap();
    assert!(sessions[0].background.is_empty());
}

#[test]
fn slug_matches_claudes_utf16_replacement_and_long_path_hash() {
    assert_eq!(project_slug("/tmp/a.b_中😀"), "-tmp-a-b----");
    let cwd = format!("/{}", "a".repeat(210));
    assert_eq!(project_slug(&cwd), format!("-{}-djaaup", "a".repeat(199)));
}

#[test]
fn notifications_hide_cached_tasks_and_background_results_retry_foreground_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut tasks = Tasks::new(dir.path().join("root"));
    let path = dir.path().join("session.jsonl");
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
    let directory = tasks.directory(&sessions[0]);
    fs::create_dir_all(&directory).unwrap();
    let output = directory.join(format!("{id}.output"));
    fs::write(&output, b"").unwrap();
    // Before the background tool result arrives, this is a foreground shell.
    fs::write(&path, format!("{}\n", records[0])).unwrap();
    let observed = Observed {
        held: [output].into(),
        alive: HashSet::new(),
        scans: Cell::new(0),
    };
    tasks.refresh(dir.path(), &mut sessions, &observed).unwrap();
    assert!(sessions[0].background.is_empty());
    fs::write(&path, RECORDS).unwrap();
    let summary = transcript::read(&path, fs::metadata(&path).unwrap().len()).unwrap();
    tasks.observe("session", &summary);
    tasks.refresh(dir.path(), &mut sessions, &observed).unwrap();
    assert_eq!(sessions[0].background.len(), 1);
    let mut completed = transcript::Summary::default();
    completed.completed_background.insert(id.to_owned());
    tasks.observe("session", &completed);
    tasks.refresh(dir.path(), &mut sessions, &observed).unwrap();
    assert!(sessions[0].background.is_empty());
}

#[test]
fn symlink_task_outputs_are_not_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let mut tasks = Tasks::new(dir.path().join("root"));
    let mut sessions = [super::super::session(
        "session".into(),
        transcript::Summary::default(),
        None,
        Utc::now(),
    )];
    let directory = tasks.directory(&sessions[0]);
    fs::create_dir_all(&directory).unwrap();
    let target = dir.path().join("subagent.jsonl");
    fs::write(&target, b"").unwrap();
    let link = directory.join("agent.output");
    symlink(target, &link).unwrap();
    let observed = Observed {
        held: [link].into(),
        alive: HashSet::new(),
        scans: Cell::new(0),
    };
    tasks.refresh(dir.path(), &mut sessions, &observed).unwrap();
    assert!(sessions[0].background.is_empty());
    assert_eq!(observed.scans.get(), 0);
}

/// Explicit integration probe; ordinary unit tests use injected observations.
#[test]
#[ignore = "starts a real sleep and invokes lsof/kill; run explicitly"]
fn real_sleep_output_liveness_and_refresh_cost() {
    let dir = tempfile::tempdir().unwrap();
    let mut tasks = Tasks::new(fs::canonicalize(dir.path()).unwrap().join("root"));
    let path = dir.path().join("session.jsonl");
    fs::write(
        &path,
        format!(
            "{RECORDS}{}",
            include_str!("../../../tests/fixtures/background/quiet-turn.jsonl")
        ),
    )
    .unwrap();
    let records: Vec<serde_json::Value> = RECORDS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let id = records[1]["toolUseResult"]["backgroundTaskId"]
        .as_str()
        .unwrap();
    let mut sessions = [super::super::session(
        "session".into(),
        transcript::Summary::default(),
        Some(path),
        Utc::now(),
    )];
    let directory = tasks.directory(&sessions[0]);
    fs::create_dir_all(&directory).unwrap();
    let output = directory.join(format!("{id}.output"));
    let file = File::create(&output).unwrap();
    struct Sleep(std::process::Child);
    impl Drop for Sleep {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut sleep = Sleep(
        Command::new("sleep")
            .arg("20")
            .stdout(file.try_clone().unwrap())
            .stderr(file)
            .spawn()
            .unwrap(),
    );
    let result = (|| -> io::Result<()> {
        assert!(Processes.alive(sleep.0.id())?);
        let first = std::time::Instant::now();
        tasks.refresh(dir.path(), &mut sessions, &Processes)?;
        let first_elapsed = first.elapsed();
        assert_eq!(sessions[0].background.len(), 1);
        let before = tasks.transcript_bytes;
        let tail_before = tasks.output_bytes;
        let next = std::time::Instant::now();
        tasks.refresh(dir.path(), &mut sessions, &Processes)?;
        eprintln!(
            "first={first_elapsed:?} transcript_bytes={before}; steady={:?} transcript_bytes={} output_bytes={}",
            next.elapsed(),
            tasks.transcript_bytes - before,
            tasks.output_bytes - tail_before
        );
        assert_eq!(tasks.transcript_bytes, before);
        assert_eq!(tasks.output_bytes, tail_before);
        Ok(())
    })();
    sleep.0.kill().unwrap();
    sleep.0.wait().unwrap();
    result.unwrap();
    assert!(!Processes.alive(sleep.0.id()).unwrap());
    tasks
        .refresh(dir.path(), &mut sessions, &Processes)
        .unwrap();
    assert!(sessions[0].background.is_empty());
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
    });
    let mut unregistered = row.clone();
    unregistered.background_job = None;
    let mut rows = [unregistered, row];
    Tasks::new(directory.path().join("tasks"))
        .refresh(directory.path(), &mut rows, &Processes)
        .unwrap();
    assert!(rows[0].background.is_empty());
    assert_eq!(rows[1].background.len(), 1);
    assert_eq!(rows[1].background[0].output, output);
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
    let directory = tasks.directory(&sessions[0]);
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
            output,
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
