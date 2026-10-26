//! Opt-in, billable checks against real Haiku background workers; no shared daemon teardown.
use super::Client;
use crate::{
    app::live_tests as dashboard,
    providers::{SessionInput, claude::ClaudeProvider},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Live {
    cwd: PathBuf,
    config: PathBuf,
    evidence: PathBuf,
    jobs: Vec<String>,
}

fn command(cwd: &Path, args: &[&str]) -> io::Result<String> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = Command::new("claude")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if let Some(status) = child.try_wait()? {
            stdout.rewind()?;
            stderr.rewind()?;
            let mut text = String::new();
            stdout.read_to_string(&mut text)?;
            stderr.read_to_string(&mut text)?;
            return if status.success() {
                Ok(text)
            } else {
                Err(io::Error::other(format!(
                    "claude {args:?}: {status}: {text}"
                )))
            };
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("claude {args:?}"),
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

impl Live {
    fn new() -> io::Result<Self> {
        let cwd = std::env::current_dir()?;
        let config = match std::env::var_os("CLAUDE_CONFIG_DIR") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(
                std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME unset"))?,
            )
            .join(".claude"),
        };
        let evidence = match std::env::var_os("AGENTS_CLAUDE_INPUT_EVIDENCE") {
            Some(path) => PathBuf::from(path),
            None => tempfile::Builder::new()
                .prefix("agents-claude-input-")
                .tempdir()?
                .keep(),
        };
        fs::create_dir_all(&evidence)?;
        Ok(Self {
            cwd,
            config,
            evidence,
            jobs: Vec::new(),
        })
    }

    fn save(&self, name: &str, value: &Value) -> io::Result<PathBuf> {
        let path = self.evidence.join(name);
        fs::write(&path, serde_json::to_vec_pretty(value)?)?;
        Ok(path)
    }

    fn start(&mut self) -> io::Result<String> {
        // Settings affect this worker only, including environments with a 1000 ms Bash default.
        let settings =
            json!({"env":{"BASH_DEFAULT_TIMEOUT_MS":"60000","BASH_MAX_TIMEOUT_MS":"60000"}})
                .to_string();
        let output = command(
            &self.cwd,
            &[
                "--bg",
                "--model",
                "haiku",
                "--effort",
                "low",
                "--safe-mode",
                "--setting-sources",
                "",
                "--settings",
                &settings,
                "--tools",
                "Bash",
                "--allowedTools",
                "Bash",
                "--permission-mode",
                "dontAsk",
                "Reply exactly INPUT_READY. Do not use tools or edit files.",
            ],
        )?;
        fs::write(self.evidence.join("launch.txt"), &output)?;
        let job = output
            .lines()
            .find_map(|line| line.split("backgrounded · ").nth(1))
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                io::Error::other(
                    "no job id; inspect launch.txt for login/trust/environment blockers",
                )
            })?
            .to_owned();
        self.jobs.push(job.clone());
        self.save("created-jobs.json", &json!(self.jobs))?;
        Ok(job)
    }

    fn registry(&self, job: &str) -> io::Result<Option<Value>> {
        for entry in fs::read_dir(self.config.join("sessions"))? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let value: Value = match fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                Some(value) => value,
                None => continue, // Registry files are concurrently created and removed.
            };
            if value["jobId"] == job {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn wait_registry(&self, job: &str, status: &str) -> io::Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let entry = self.registry(job)?;
            if let Some(entry) = &entry
                && entry["status"] == status
            {
                return Ok(entry.clone());
            }
            if Instant::now() >= deadline {
                self.save("registry-timeout.json", &json!(entry))?;
                return Err(io::Error::other(format!(
                    "job {job} never became {status}; login/trust may block worker"
                )));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn events(&self, id: &str) -> io::Result<Vec<Value>> {
        for project in fs::read_dir(self.config.join("projects"))? {
            let path = project?.path().join(format!("{id}.jsonl"));
            if !path.is_file() {
                continue;
            }
            let text = fs::read_to_string(path)?;
            // A growing JSONL file may end in a partial record; parse complete lines only.
            return text
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
                .map(|line| serde_json::from_str(line).map_err(io::Error::other))
                .collect();
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "worker transcript not created",
        ))
    }

    fn wait_events(
        &self,
        id: &str,
        name: &str,
        ready: impl Fn(&[Value]) -> bool,
    ) -> io::Result<Vec<Value>> {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let events = match self.events(id) {
                Ok(events) => events,
                Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
                Err(error) => return Err(error),
            };
            self.save(name, &json!(events))?;
            if ready(&events) {
                return Ok(events);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "transcript condition failed: {name}"
                )));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn cleanup(&mut self) -> io::Result<()> {
        let mut errors = Vec::new();
        for job in self.jobs.drain(..) {
            // Only IDs returned by this test's own launch can reach these commands.
            let stop = command(&self.cwd, &["stop", &job]);
            let remove = command(&self.cwd, &["rm", &job]);
            if let Err(error) = stop {
                errors.push(error.to_string());
            }
            if let Err(error) = remove {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(io::Error::other(errors.join("; ")))
        }
    }
}

fn content(event: &Value) -> &[Value] {
    event["message"]["content"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn replied(events: &[Value], marker: &str) -> bool {
    events.iter().any(|event| {
        event["type"] == "assistant"
            && content(event).iter().any(|block| {
                block["type"] == "text"
                    && block["text"]
                        .as_str()
                        .is_some_and(|text| text.contains(marker))
            })
    })
}

fn delivered(events: &[Value], prompt: &str, busy: bool) -> bool {
    events.iter().any(|event| {
        if busy {
            event["type"] == "attachment"
                && event["attachment"]["type"] == "queued_command"
                && event["attachment"]["prompt"] == prompt
                && event["attachment"]["origin"]["kind"] == "human"
        } else {
            event["type"] == "user"
                && event["message"]["content"] == prompt
                && event["origin"]["kind"] == "human"
        }
    })
}

fn queued(events: &[Value], prompt: &str, operation: &str) -> bool {
    events.iter().any(|event| {
        event["type"] == "queue-operation"
            && event["operation"] == operation
            && event["content"] == prompt
            && (operation != "remove" || event["reason"] == "absorbed_mid_turn")
    })
}

fn tool_result<'a>(events: &'a [Value], tool: &str) -> Option<&'a Value> {
    events
        .iter()
        .flat_map(content)
        .find(|block| block["type"] == "tool_result" && block["tool_use_id"] == tool)
}

fn run(live: &mut Live, cases: &mut Vec<Value>) -> io::Result<()> {
    let job = live.start()?;
    let entry = live.wait_registry(&job, "idle")?;
    let id = entry["sessionId"]
        .as_str()
        .ok_or_else(|| io::Error::other("registry missing sessionId"))?
        .to_owned();
    live.wait_events(&id, "ready.json", |events| replied(events, "INPUT_READY"))?;
    let client = Client::new(&live.config);
    let mut app = dashboard::app(
        Box::new(ClaudeProvider::new(live.config.clone())),
        live.evidence.join("preferences"),
    );

    for through_dashboard in [false, true] {
        let route = if through_dashboard {
            "dashboard"
        } else {
            "client"
        };
        for busy in [false, true] {
            let label = format!("{route}-{}", if busy { "busy" } else { "idle" });
            live.wait_registry(&job, "idle")?;
            let mut tool = None;
            let tool_marker = format!("INPUT_TOOL_{}", label.replace('-', "_"));
            if busy {
                let prompt = format!(
                    "Use Bash exactly once to run `sleep 12; printf {tool_marker}` with timeout 60000 and run_in_background false. Wait for it in the foreground, then reply TOOL_DONE. Do not edit files or use other tools."
                );
                client.enqueue(&job, &prompt)?;
                let events = live.wait_events(&id, &format!("{label}-pending.json"), |events| {
                    events.iter().flat_map(content).any(|block| {
                        block["type"] == "tool_use"
                            && block["name"] == "Bash"
                            && block["input"]["command"]
                                .as_str()
                                .is_some_and(|command| command.contains(&tool_marker))
                            && block["id"]
                                .as_str()
                                .is_some_and(|id| tool_result(events, id).is_none())
                    })
                })?;
                let block = events
                    .iter()
                    .flat_map(content)
                    .find(|block| {
                        block["type"] == "tool_use"
                            && block["name"] == "Bash"
                            && block["input"]["command"]
                                .as_str()
                                .is_some_and(|command| command.contains(&tool_marker))
                    })
                    .unwrap();
                tool = Some(
                    block["id"]
                        .as_str()
                        .ok_or_else(|| io::Error::other("Bash missing id"))?
                        .to_owned(),
                );
            }
            let marker = format!("INPUT_REPLY_{}", label.replace('-', "_"));
            let prompt = format!("Reply exactly {marker} and stop. Do not use tools.");
            if through_dashboard {
                dashboard::refresh(&mut app);
                let session = dashboard::session(&app, &id);
                assert_eq!(
                    session.background_job.as_ref().map(|job| job.id.as_str()),
                    Some(job.as_str())
                );
                // Refresh and keyboard selection happen before the final pending-tool check.
                dashboard::select(&mut app, &session.id);
            }
            if let Some(tool) = &tool {
                let events = live.events(&id)?;
                assert!(
                    tool_result(&events, tool).is_none(),
                    "Bash finished before enqueue"
                );
                live.save(&format!("{label}-at-send.json"), &json!(events))?;
            }
            if through_dashboard {
                let session = dashboard::session(&app, &id);
                dashboard::send(&mut app, session.id, SessionInput::Prompt(prompt.clone()));
            } else {
                client.enqueue(&job, &prompt)?;
            }
            let evidence = format!("{label}-delivered.json");
            let events = live.wait_events(&id, &evidence, |events| {
                delivered(events, &prompt, busy)
                    && queued(events, &prompt, "enqueue")
                    && replied(events, &marker)
                    && (!busy
                        || (queued(events, &prompt, "remove")
                            && tool.as_ref().is_some_and(|tool| {
                                tool_result(events, tool).is_some_and(|result| {
                                    result["is_error"] != true
                                        && result["content"]
                                            .as_str()
                                            .is_some_and(|text| text.contains(&tool_marker))
                                })
                            })))
            })?;
            if through_dashboard {
                dashboard::refresh(&mut app);
                let _ = dashboard::session(&app, &id);
            }
            cases.push(
                json!({"case":label,"success":true,"session_id":id,"job_id":job,
                "human":true,"queue_enqueue":true,"assistant_reply":marker,
                "tool_pending_at_send":busy,"tool_succeeded":busy,"absorbed_mid_turn":busy,
                "evidence":live.evidence.join(evidence),"records":events.len()}),
            );
            live.save("progress.json", &json!(cases))?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "billable: starts a real Haiku job; four idle/busy client/dashboard deliveries, up to 8 minutes"]
fn real_background_input_client_and_dashboard() {
    let mut live = Live::new().expect("live environment");
    eprintln!("Claude input evidence: {}", live.evidence.display());
    let mut cases = Vec::new();
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut live, &mut cases)));
    let cleanup = live.cleanup();
    let error = match &result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(panic) => Some(
            panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "test panic".to_owned()),
        ),
    };
    let success = error.is_none() && cleanup.is_ok();
    live.save("verdict.json", &json!({"success":success,"cases":cases,"error":error,
        "cleanup_error":cleanup.as_ref().err().map(ToString::to_string),"evidence_dir":live.evidence})).expect("write verdict");
    assert!(
        success,
        "real Claude input failed; inspect {}/verdict.json",
        live.evidence.display()
    );
}
