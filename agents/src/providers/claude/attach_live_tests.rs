//! Opt-in checks against an actual background Claude session.
use super::{ClaudeProvider, ask_user_question::AskUserQuestion, attach};
use crate::app::live_tests as dashboard;
use crate::providers::SessionInput;
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

struct Live {
    repo: tempfile::TempDir,
    home: PathBuf,
    backups: Vec<(PathBuf, Option<Vec<u8>>)>,
    jobs: Vec<String>,
}

fn command(cwd: &Path, program: &str, args: &[&str]) -> io::Result<String> {
    let output_file = tempfile::tempfile()?;
    let errors = tempfile::tempfile()?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(output_file.try_clone()?)
        .stderr(errors.try_clone()?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait()? {
            use std::io::{Read, Seek};
            let mut output = output_file;
            output.rewind()?;
            let mut text = String::new();
            output.read_to_string(&mut text)?;
            if !status.success() {
                let mut errors = errors;
                errors.rewind()?;
                errors.read_to_string(&mut text)?;
                return Err(io::Error::other(format!("{program} failed: {text}")));
            }
            return Ok(text);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err(io::Error::new(io::ErrorKind::TimedOut, program.to_owned()));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    std::io::Write::write_all(&mut temporary, bytes)?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

impl Live {
    fn new() -> io::Result<Self> {
        let home =
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME unset"))?)
                .join(".claude");
        let paths = [home.with_extension("json"), home.join("settings.json")];
        let mut backups = Vec::new();
        for path in paths {
            let bytes = match fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            backups.push((path, bytes));
        }
        Ok(Self {
            repo: tempfile::tempdir()?,
            home,
            backups,
            jobs: Vec::new(),
        })
    }

    fn prepare(&mut self) -> io::Result<String> {
        self.trust_repo()?;
        let mut app = dashboard::app(
            Box::new(ClaudeProvider::new(self.home.clone())),
            self.repo.path().to_owned(),
        );
        let job = dashboard::create(
            &mut app,
            crate::tasks::NewTask {
                cwd: self.repo.path().to_owned(),
                name: None,
                model: Some("haiku".into()),
                effort: None,
                images: Vec::new(),
                prompt: "Reply exactly READY and stop. Do not use tools or edit files.".into(),
            },
        );
        self.jobs.push(job.clone());
        Ok(job)
    }

    /// Starts a background job whose AskUserQuestion goes to the hook at `port`, without the
    /// user's own settings and hooks.
    fn prepare_question(&mut self, port: u16) -> io::Result<String> {
        self.trust_repo()?;
        let settings = json!({"hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[{"type":"http","url":format!("http://127.0.0.1:{port}/ask-user-question"),"timeout":3600}]}]}}).to_string();
        let text = command(
            self.repo.path(),
            "claude",
            &[
                "--bg",
                "--model",
                "haiku",
                "--effort",
                "low",
                "--tools",
                "AskUserQuestion",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
                "--setting-sources",
                "",
                "--settings",
                &settings,
                "Use AskUserQuestion exactly once. Ask question \"Hook color?\", header \"Color\", multiSelect false, options Red with description Red choice and Blue with description Blue choice. After my answer, reply DONE. This is an explicit interactive question test.",
            ],
        )?;
        let job = text
            .lines()
            .find_map(|line| line.split("backgrounded · ").nth(1))
            .ok_or_else(|| io::Error::other(format!("no background job id: {text}")))?
            .trim()
            .to_owned();
        self.jobs.push(job.clone());
        Ok(job)
    }

    fn trust_repo(&mut self) -> io::Result<()> {
        let cwd = self.repo.path();
        command(cwd, "git", &["init"])?;
        command(
            cwd,
            "git",
            &[
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "user.name=Agents fixture",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        )?;
        let path = &self.backups[0].0;
        let mut config: Value = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => json!({}),
            Err(error) => return Err(error),
        };
        config["projects"][fs::canonicalize(cwd)?.to_str().unwrap()]["hasTrustDialogAccepted"] =
            json!(true);
        atomic_write(path, &serde_json::to_vec_pretty(&config)?)
    }

    fn registry(&self, job: &str) -> io::Result<Option<Value>> {
        for entry in fs::read_dir(self.home.join("sessions"))? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let value: Value = match fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                Some(value) => value,
                None => continue,
            };
            if value["jobId"] == job {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn status(&self, job: &str, status: &str) -> io::Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(entry) = self.registry(job)?
                && entry["status"] == status
            {
                return Ok(entry);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "registry did not become {status}"
                )));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn transcript(&self, id: &str) -> io::Result<Vec<Value>> {
        for project in fs::read_dir(self.home.join("projects"))? {
            let path = project?.path().join(format!("{id}.jsonl"));
            if !path.is_file() {
                continue;
            }
            let text = fs::read_to_string(path)?;
            return text
                .lines()
                .map(|line| serde_json::from_str(line).map_err(io::Error::other))
                .collect();
        }
        Err(io::Error::other("session transcript not found"))
    }

    fn wait_prompt(&self, id: &str, prompt: &str) -> io::Result<Vec<Value>> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(events) = self.transcript(id)
                && events
                    .iter()
                    .any(|event| event["type"] == "user" && event["message"]["content"] == prompt)
            {
                return Ok(events);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "bracketed paste text did not appear exactly in the session transcript",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_queued_command(&self, id: &str, prompt: &str) -> io::Result<Vec<Value>> {
        let deadline = Instant::now() + Duration::from_secs(90);
        let mut latest = Vec::new();
        loop {
            if let Ok(events) = self.transcript(id) {
                let delivered = events.iter().any(|event| {
                    event["type"] == "attachment"
                        && event["attachment"]["type"] == "queued_command"
                        && event["attachment"]["prompt"] == prompt
                        && event["sessionId"] == id
                });
                latest = events;
                if delivered {
                    return Ok(latest);
                }
            }
            if Instant::now() >= deadline {
                fs::create_dir_all(std::env::temp_dir().join("agents-stage3-live"))?;
                fs::write(
                    std::env::temp_dir().join("agents-stage3-live/queued-transcript-timeout.json"),
                    serde_json::to_vec_pretty(&latest)?,
                )?;
                return Err(io::Error::other(
                    "accepted prompt did not reach a queued_command transcript boundary",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn cleanup(&mut self) -> io::Result<()> {
        let mut errors = Vec::new();
        for job in self.jobs.drain(..) {
            let _ = command(self.repo.path(), "claude", &["stop", &job]);
            if let Err(error) = command(self.repo.path(), "claude", &["rm", &job]) {
                errors.push(error.to_string());
            }
        }
        for (path, bytes) in &self.backups {
            let result = match bytes {
                Some(bytes) => atomic_write(path, bytes),
                None => match fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error),
                },
            };
            if let Err(error) = result {
                errors.push(error.to_string());
            }
            let actual = fs::read(path).ok();
            if actual.as_ref() != bytes.as_ref() {
                errors.push("global config byte restoration failed".to_owned());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(errors.join("; ")))
        }
    }
}

#[test]
#[ignore = "starts a real billable Claude background session and restores global configuration"]
fn real_attach_prompt_paste_busy_steer_interrupt_model() {
    let mut live = Live::new().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> io::Result<()> {
        let job = live.prepare()?;
        let entry = live.status(&job, "idle")?;
        let id = entry["sessionId"]
            .as_str()
            .ok_or_else(|| io::Error::other("registry lacks sessionId"))?
            .to_owned();
        live.wait_prompt(
            &id,
            "Reply exactly READY and stop. Do not use tools or edit files.",
        )?;
        live.status(&job, "idle")?;
        let mut app = dashboard::app(
            Box::new(ClaudeProvider::new(live.home.clone())),
            live.repo.path().to_owned(),
        );
        let session = dashboard::session(&app, &id);
        assert_eq!(
            session.background_job.as_ref().map(|job| job.id.as_str()),
            Some(job.as_str())
        );
        assert_eq!(
            session
                .process
                .as_ref()
                .and_then(|process| process.label.as_deref()),
            Some("bg")
        );
        let settings = live.home.join("settings.json");
        fs::create_dir_all(live.repo.path().join(".claude"))?;
        fs::write(
            live.repo.path().join(".claude/settings.local.json"),
            serde_json::to_vec(&json!({"permissions": {"allow": ["Bash(sleep 10)"]}}))?,
        )?;
        let pasted = "Use the Bash tool to run exactly `sleep 10` three times, in separate foreground tool calls, one after another.\nAfter all three calls, reply agents-paste-✓. Do not edit files or use any other tools.";
        dashboard::send(
            &mut app,
            session.id.clone(),
            SessionInput::Prompt(pasted.to_owned()),
        );
        live.wait_prompt(&id, pasted)?;
        let busy = live.status(&job, "busy")?;
        assert_eq!(busy["sessionId"], id);
        let steer = "After the current tool finishes, include agents-steer-✓ in your next reply and continue the remaining sleep calls. Do not start another task.";
        dashboard::send(
            &mut app,
            session.id.clone(),
            SessionInput::Prompt(steer.to_owned()),
        );
        eprintln!("busy Enter accepted the queued prompt");
        let events = live.wait_queued_command(&id, steer)?;
        assert!(
            events
                .iter()
                .any(|event| event["type"] == "user" && event["message"]["content"] == pasted)
        );
        eprintln!("queued_command reached the real transcript boundary");
        assert_eq!(live.registry(&job)?.unwrap()["sessionId"], id);

        dashboard::send(&mut app, session.id.clone(), SessionInput::Interrupt);
        live.status(&job, "idle")?;
        eprintln!("interrupt returned the registry to idle");
        let defaults: Value = serde_json::from_slice(&fs::read(&settings)?)?;
        dashboard::send(
            &mut app,
            session.id.clone(),
            SessionInput::SetModel("sonnet".to_owned()),
        );
        let restored: Value = serde_json::from_slice(&fs::read(&settings)?)?;
        for key in ["model", "effortLevel"] {
            assert_eq!(
                restored.get(key),
                defaults.get(key),
                "global default {key} changed"
            );
        }
        let model_screen = attach::inspect(&job, live.repo.path())?;
        assert!(
            model_screen.to_lowercase().contains("sonnet"),
            "session model did not change"
        );
        assert_eq!(live.registry(&job)?.unwrap()["sessionId"], id);
        eprintln!("same-session model change preserved global defaults");
        dashboard::delete(&mut app, session.id.clone());
        dashboard::refresh(&mut app);
        assert!(
            !live.home.join("jobs").join(&job).exists(),
            "App delete must remove the real background job"
        );
        assert!(
            live.registry(&job)?.is_none(),
            "App delete must remove registry"
        );
        live.jobs.retain(|created| created != &job);
        Ok(())
    }));
    let cleanup = live.cleanup();
    assert!(cleanup.is_ok(), "cleanup failed: {cleanup:?}");
    match result {
        Ok(result) => result.unwrap(),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[test]
#[ignore = "starts a real billable Claude background session and restores global configuration"]
fn real_attach_clears_left_input_and_keeps_a_pending_question() {
    let mut live = Live::new().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> io::Result<()> {
        let server = Arc::new(AskUserQuestion::bind(0, live.home.clone())?);
        let job = live.prepare_question(server.port)?;
        let deadline = Instant::now() + Duration::from_secs(90);
        let ask = loop {
            if let Some(ask) = server.pending().into_iter().next() {
                break ask;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "real AskUserQuestion request never arrived",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        };
        let left = "agents-left-input stale text\nsecond line";
        attach::type_input(&job, live.repo.path(), left)?;
        let mut provider = ClaudeProvider::new(live.home.clone());
        provider.ask_user_question = Some(Arc::clone(&server));
        let mut app = dashboard::app(Box::new(provider), live.repo.path().to_owned());
        let session = dashboard::session(&app, &ask.session.id);
        let prompt = "After the question, reply agents-left-input-✓ too.";
        dashboard::send(
            &mut app,
            session.id.clone(),
            SessionInput::Prompt(prompt.to_owned()),
        );
        assert_eq!(
            server.pending(),
            vec![ask.clone()],
            "clearing the input must keep the pending question"
        );
        let session = dashboard::session(&app, &ask.session.id);
        dashboard::answer_questions(&mut app, session, vec![vec!["Blue".into()]]);
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let events = live.transcript(&ask.session.id).unwrap_or_default();
            let text = serde_json::to_string(&events)?;
            assert!(
                !text.contains("agents-left-input stale"),
                "left input was submitted"
            );
            if events
                .iter()
                .any(|event| event["toolUseResult"]["answers"] == json!({"Hook color?":"Blue"}))
                && text.contains(prompt)
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(
                    "transcript never recorded both the answer and the prompt",
                ));
            }
            thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }));
    let cleanup = live.cleanup();
    assert!(cleanup.is_ok(), "cleanup failed: {cleanup:?}");
    match result {
        Ok(result) => result.unwrap(),
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
