//! Opt-in, disposable AskUserQuestion delivery checks against Claude Code.
use super::*;
use std::io::Read;
use std::{fs, path::PathBuf, process::Stdio, thread};

struct Probe {
    cwd: tempfile::TempDir,
    config: PathBuf,
    job: Option<String>,
    trust: Option<Value>,
    trust_path: PathBuf,
}

impl Probe {
    fn command(&self, args: &[&str]) -> io::Result<String> {
        let out = tempfile::tempfile()?;
        let err = tempfile::tempfile()?;
        let mut child = Command::new("claude")
            .args(args)
            .current_dir(self.cwd.path())
            .stdin(Stdio::null())
            .stdout(out.try_clone()?)
            .stderr(err.try_clone()?)
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Some(status) = child.try_wait()? {
                use std::io::Seek;
                let mut out = out;
                out.rewind()?;
                let mut text = String::new();
                out.read_to_string(&mut text)?;
                if !status.success() {
                    let mut err = err;
                    err.rewind()?;
                    err.read_to_string(&mut text)?;
                    return Err(io::Error::other(text));
                }
                return Ok(text);
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err(io::Error::other("Claude command timed out"));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn new() -> io::Result<Self> {
        let config = PathBuf::from(std::env::var_os("HOME").ok_or_else(mismatch)?).join(".claude");
        let cwd = tempfile::tempdir_in("/tmp")?;
        let trust_path = config.with_extension("json");
        let mut value: Value = serde_json::from_slice(&fs::read(&trust_path)?)?;
        let key = cwd.path().canonicalize()?.to_string_lossy().into_owned();
        let trust = value["projects"].get(&key).cloned();
        value["projects"][&key]["hasTrustDialogAccepted"] = json!(true);
        fs::write(&trust_path, serde_json::to_vec_pretty(&value)?)?;
        Ok(Self {
            cwd,
            config,
            job: None,
            trust,
            trust_path,
        })
    }
    fn start(&mut self) -> io::Result<()> {
        let prompt = "This is an isolated terminal answer transport probe. Use ONLY AskUserQuestion. Make exactly FIVE calls in order and wait for each answer before the next. Do not answer yourself. Use exact question/header text. Every question has options Red (description Red choice), Green (description Green choice), Blue (description Blue choice). Call 1: one question question='Transport single?' header='Single' multiSelect=false. Call 2: one question question='Transport multi?' header='Multi' multiSelect=true. Call 3: two questions in ONE call: question='Transport first?' header='First' multiSelect=false and question='Transport second?' header='Second' multiSelect=false. Call 4: one question question='Transport other?' header='Other' multiSelect=false. Call 5: one question question='Transport multi Other?' header='MultiOther' multiSelect=true. After all answers reply DONE and stop. This explicitly requests testing interactive question behavior.";
        self.start_prompt(prompt)
    }
    fn start_prompt(&mut self, prompt: &str) -> io::Result<()> {
        let output = self.command(&[
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
            "--safe-mode",
            prompt,
        ])?;
        let id = output
            .split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_ascii_hexdigit()))
            .find(|s| s.len() == 8 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| io::Error::other(format!("Missing job id: {output}")))?;
        self.job = Some(id.to_owned());
        eprintln!("probe job {id}");
        Ok(())
    }
    fn waiting(&self, text: &str) -> io::Result<Value> {
        let path = self
            .config
            .join("jobs")
            .join(self.job.as_ref().unwrap())
            .join("state.json");
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Ok(bytes) = fs::read(&path)
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                && value["block"]["questions"]
                    .as_array()
                    .is_some_and(|qs| qs.iter().any(|q| q["question"] == text))
            {
                return Ok(value);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "Question {text} not reached; state {}",
                    fs::read_to_string(&path).unwrap_or_default()
                )));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    fn transcript(&self, id: &str) -> io::Result<PathBuf> {
        for dir in fs::read_dir(self.config.join("projects"))? {
            let path = dir?.path().join(format!("{id}.jsonl"));
            if path.is_file() {
                return Ok(path);
            }
        }
        Err(io::Error::other("Probe transcript missing"))
    }
    fn cleanup(&mut self) -> io::Result<()> {
        if let Some(job) = self.job.take() {
            let _ = self.command(&["stop", &job]);
            self.command(&["rm", &job])?;
        }
        let mut value: Value = serde_json::from_slice(&fs::read(&self.trust_path)?)?;
        let key = self
            .cwd
            .path()
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        if let Some(saved) = self.trust.take() {
            value["projects"][&key] = saved;
        } else if let Some(projects) = value["projects"].as_object_mut() {
            projects.remove(&key);
        }
        fs::write(&self.trust_path, serde_json::to_vec_pretty(&value)?)?;
        Ok(())
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            eprintln!("Probe cleanup failed: {error}");
        }
    }
}

#[test]
#[ignore = "starts a disposable Claude session with its real Other editor already focused"]
fn ask_live_other_focus_after_restart() {
    let mut probe = Probe::new().unwrap();
    probe.start_prompt("Use ONLY AskUserQuestion. Ask exactly one single-select question: question 'Cold Other?' header 'Cold' with exactly two options Red (description Red choice) and Green (description Green choice). Wait for my answer. This is an explicit interactive question restart test.").unwrap();
    let state = probe.waiting("Cold Other?").unwrap();
    let id = state["sessionId"].as_str().unwrap();
    let ask = pending(
        probe.job.as_ref().unwrap(),
        probe.cwd.path(),
        format!("claude:{id}").parse().unwrap(),
        Utc::now(),
        &state["block"],
    )
    .unwrap();
    let AskKind::Questions { questions, .. } = &ask.kind;
    let question = question_value(&questions[0]);
    {
        let mut terminal = Terminal::spawn(probe.job.as_ref().unwrap(), probe.cwd.path()).unwrap();
        terminal
            .wait_allowing(
                |screen| parse_page(screen, &question).is_ok(),
                Some("Enter to select"),
            )
            .unwrap();
        terminal.write(b"3").unwrap();
        terminal
            .wait_allowing(
                |screen| {
                    body(screen).is_ok_and(|page| {
                        page.lines()
                            .any(|line| numbered(line).is_some_and(|(n, focus, _)| n == 3 && focus))
                    })
                },
                Some("Enter to select"),
            )
            .unwrap();
        if let Some(directory) = std::env::var_os("AGENTS_ASK_CAPTURE") {
            let directory = PathBuf::from(directory);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("other-focused.raw"), &terminal.raw).unwrap();
            fs::write(
                directory.join("other-focused.screen.txt"),
                terminal.screen(),
            )
            .unwrap();
            fs::write(directory.join("pending-state.json"), state.to_string()).unwrap();
        }
        terminal.detach().unwrap();
    }
    let answer = Answer {
        answers: BTreeMap::from([(
            "Cold Other?".into(),
            crate::ask::QuestionAnswer {
                answers: Vec::new(),
                other: Some("hey".into()),
            },
        )]),
    };
    let result = send(
        probe.job.as_ref().unwrap(),
        probe.cwd.path(),
        &probe.config.join("settings.json"),
        SessionInput::Answer {
            ask,
            answer,
            transcript: probe.transcript(id).unwrap(),
        },
    );
    if let Some(directory) = std::env::var_os("AGENTS_ASK_CAPTURE") {
        let directory = PathBuf::from(directory);
        fs::write(directory.join("result.txt"), format!("{result:?}")).unwrap();
        fs::write(
            directory.join("transcript.jsonl"),
            fs::read(probe.transcript(id).unwrap()).unwrap(),
        )
        .unwrap();
    }
    probe.cleanup().unwrap();
    result.unwrap();
}

#[test]
#[ignore = "starts a disposable Claude session and inspects its real question from two clients"]
fn ask_live_concurrent_question_inspection() {
    let mut probe = Probe::new().unwrap();
    probe.start_prompt("Use ONLY AskUserQuestion. Ask exactly TWO single-select questions in ONE call. First: question 'Concurrent first?' header 'First'. Second: question 'Concurrent second?' header 'Second'. Both have exactly two options Red (description Red choice) and Green (description Green choice). Wait for my answer. This is an explicit interactive question inspection test.").unwrap();
    let state = probe.waiting("Concurrent first?").unwrap();
    // Allow dashboards already watching this disposable session to finish their
    // initial inspection before the two clients under test start together.
    thread::sleep(Duration::from_secs(3));
    let session: SessionKey = format!("claude:{}", state["sessionId"].as_str().unwrap())
        .parse()
        .unwrap();
    let start = std::sync::Barrier::new(2);
    let results = thread::scope(|scope| {
        let inspect = || {
            start.wait();
            pending(
                probe.job.as_ref().unwrap(),
                probe.cwd.path(),
                session.clone(),
                Utc::now(),
                &state["block"],
            )
        };
        let first = scope.spawn(inspect);
        let second = scope.spawn(inspect);
        [first.join().unwrap(), second.join().unwrap()]
    });
    if let Some(directory) = std::env::var_os("AGENTS_ASK_CAPTURE") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("pending-state.json"), state.to_string()).unwrap();
        fs::write(
            directory.join("concurrent.raw"),
            probe
                .command(&["logs", probe.job.as_ref().unwrap()])
                .unwrap(),
        )
        .unwrap();
        fs::write(directory.join("results.txt"), format!("{results:?}")).unwrap();
    }
    probe.cleanup().unwrap();
    for result in results {
        assert!(result.is_ok(), "concurrent inspection failed: {result:?}");
    }
}

#[test]
#[ignore = "starts billable disposable Claude sessions and adds/removes isolated cwd trust"]
fn ask_live_single_multi_two_other() {
    let mut probe = Probe::new().unwrap();
    probe.start().unwrap();
    for (text, answers) in [
        ("Transport single?", vec![vec!["Green"]]),
        ("Transport multi?", vec![vec!["Blue", "Red"]]),
        ("Transport first?", vec![vec!["Green"], vec!["Green"]]),
        (
            "Transport other?",
            vec![vec!["user_note: custom transport note"]],
        ),
        (
            "Transport multi Other?",
            vec![vec!["Red", "user_note: custom multi note"]],
        ),
    ] {
        let state = probe.waiting(text).unwrap();
        let id = state["sessionId"].as_str().unwrap().to_owned();
        use crate::providers::Provider;
        let mut provider = crate::providers::claude::ClaudeProvider::new(probe.config.clone());
        let deadline = Instant::now() + Duration::from_secs(25);
        let session = loop {
            if let Some(session) = provider
                .refresh(1)
                .sessions
                .into_iter()
                .find(|s| s.id.id == id && s.ask.is_some())
            {
                break session;
            }
            assert!(
                Instant::now() < deadline,
                "live provider did not resolve pending ask"
            );
            thread::sleep(Duration::from_millis(50));
        };
        let transcript = probe.transcript(&id).unwrap();
        let pending_text = fs::read_to_string(&transcript).unwrap();
        let pending_has_call = pending_text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|record| {
                record["message"]["content"]
                    .as_array()
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item["name"] == "AskUserQuestion"
                                && item["input"]["questions"]
                                    .as_array()
                                    .is_some_and(|qs| qs.iter().any(|q| q["question"] == text))
                        })
                    })
            });
        eprintln!(
            "pending {text}: transcript_has_call={pending_has_call}; provider ask={}",
            session.ask.as_ref().unwrap().id
        );
        let mut app = crate::app::live_tests::app(Box::new(provider), probe.cwd.path().to_owned());
        crate::app::live_tests::answer_questions(
            &mut app,
            session,
            answers
                .into_iter()
                .map(|values| values.into_iter().map(str::to_owned).collect())
                .collect(),
        );
        if let Some(directory) = std::env::var_os("AGENTS_ASK_CAPTURE") {
            fs::write(
                PathBuf::from(directory).join("transcript.jsonl"),
                fs::read(&transcript).unwrap(),
            )
            .unwrap();
        }
        eprintln!("verified live {text}");
    }
    probe.cleanup().unwrap();
}

#[test]
#[ignore = "starts billable disposable Claude sessions and adds/removes isolated cwd trust"]
fn ask_live_unanswered_questions() {
    let mut probe = Probe::new().unwrap();
    probe.start_prompt("This is an isolated terminal answer transport probe. Use ONLY AskUserQuestion. Make exactly THREE calls in order and wait for each answer before the next. Do not answer yourself. Use exact question/header text. Every question has options Red (description Red choice), Green (description Green choice), Blue (description Blue choice). Call 1: two questions in ONE call: question='Skip first?' header='SkipFirst' multiSelect=false and question='Skip second?' header='SkipSecond' multiSelect=false. Call 2: one question question='Skip multi?' header='SkipMulti' multiSelect=true. Call 3: three questions in ONE call: question='Skip third first?' header='One' multiSelect=false, question='Skip third middle?' header='Two' multiSelect=true and question='Skip third last?' header='Three' multiSelect=false. After all answers reply DONE and stop. This explicitly requests testing interactive question behavior.").unwrap();
    for (text, answers) in [
        ("Skip first?", vec![vec![], vec!["Green"]]),
        ("Skip multi?", vec![vec![]]),
        ("Skip third first?", vec![vec!["Red"], vec![], vec![]]),
    ] {
        let state = probe.waiting(text).unwrap();
        let id = state["sessionId"].as_str().unwrap().to_owned();
        use crate::providers::Provider;
        let mut provider = crate::providers::claude::ClaudeProvider::new(probe.config.clone());
        let deadline = Instant::now() + Duration::from_secs(25);
        let session = loop {
            if let Some(session) = provider
                .refresh(1)
                .sessions
                .into_iter()
                .find(|s| s.id.id == id && s.ask.is_some())
            {
                break session;
            }
            assert!(
                Instant::now() < deadline,
                "live provider did not resolve pending ask"
            );
            thread::sleep(Duration::from_millis(50));
        };
        let transcript = probe.transcript(&id).unwrap();
        let mut app = crate::app::live_tests::app(Box::new(provider), probe.cwd.path().to_owned());
        let answers = answers
            .into_iter()
            .map(|values| values.into_iter().map(str::to_owned).collect())
            .collect();
        // The last ask is answered while the dashboard shows the session's terminal.
        if text == "Skip third first?" {
            crate::app::live_tests::answer_questions_in_terminal(&mut app, session, answers);
        } else {
            crate::app::live_tests::answer_questions(&mut app, session, answers);
        }
        if let Some(directory) = std::env::var_os("AGENTS_ASK_CAPTURE") {
            fs::write(
                PathBuf::from(directory).join(format!("unanswered-{text}.jsonl")),
                fs::read(&transcript).unwrap(),
            )
            .unwrap();
        }
        eprintln!("verified live {text}");
    }
    probe.cleanup().unwrap();
}

#[test]
#[ignore = "probes billable disposable Claude multi Other key map and records actual screens"]
fn ask_live_multi_other_keymap() {
    let mut probe = Probe::new().unwrap();
    probe.start_prompt("Use ONLY AskUserQuestion. This is an explicit interactive key-map probe. Ask exactly one multiSelect=true question with question 'Transport multi Other?' header 'MultiOther' and options Red (description Red choice), Green (description Green choice), Blue (description Blue choice). Wait for the answer, then reply DONE and stop.").unwrap();
    let state = probe.waiting("Transport multi Other?").unwrap();
    let id = state["sessionId"].as_str().unwrap();
    let source = &state["block"]["questions"][0];
    let mut value = source.clone();
    value["header"] = json!("MultiOther");
    let mut terminal = Terminal::spawn(probe.job.as_ref().unwrap(), probe.cwd.path()).unwrap();
    terminal
        .wait_allowing(
            |screen| parse_page(screen, &value).is_ok(),
            Some("Enter to select"),
        )
        .unwrap();
    let directory = std::env::temp_dir().join("agents-ask-multi-other");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("01-initial.screen.txt"), terminal.screen()).unwrap();
    terminal.write(b"1").unwrap();
    terminal
        .wait_allowing(|screen| checked(screen) == vec![1], Some("Enter to select"))
        .unwrap();
    fs::write(
        directory.join("01-red-selected.screen.txt"),
        terminal.screen(),
    )
    .unwrap();
    assert!(parse_page(&terminal.screen(), &value).is_ok());
    terminal.write(b"4").unwrap();
    terminal
        .wait_allowing(
            |screen| checked(screen) == vec![1, 4],
            Some("Enter to select"),
        )
        .unwrap();
    fs::write(
        directory.join("02-digit-other.screen.txt"),
        terminal.screen(),
    )
    .unwrap();
    for n in 2..=4 {
        assert!(parse_page(&terminal.screen(), &value).is_ok());
        terminal.write(b"\x1b[B").unwrap();
        terminal
            .wait_allowing(
                |screen| {
                    body(screen).is_ok_and(|page| {
                        page.lines()
                            .any(|line| numbered(line).is_some_and(|(index, f, _)| index == n && f))
                    })
                },
                Some("Enter to select"),
            )
            .unwrap();
        fs::write(
            directory.join(format!("03-focus-{n}.screen.txt")),
            terminal.screen(),
        )
        .unwrap();
    }
    fs::write(
        directory.join("03-other-focused.screen.txt"),
        terminal.screen(),
    )
    .unwrap();
    terminal.paste("custom multi note").unwrap();
    terminal
        .wait_allowing(
            |screen| screen.contains("custom multi note"),
            Some("Enter to select"),
        )
        .unwrap();
    fs::write(directory.join("04-pasted.screen.txt"), terminal.screen()).unwrap();
    assert!(parse_page(&terminal.screen(), &value).is_ok());
    terminal.write(b"\t").unwrap();
    terminal
        .wait_allowing(
            |screen| {
                screen.lines().any(|line| {
                    line.trim()
                        .strip_prefix('❯')
                        .is_some_and(|s| s.trim() == "Submit")
                })
            },
            Some("Enter to select"),
        )
        .unwrap();
    fs::write(
        directory.join("05-submit-focused.screen.txt"),
        terminal.screen(),
    )
    .unwrap();
    assert!(parse_page(&terminal.screen(), &value).is_ok());
    terminal.write(b"\r").unwrap();
    terminal
        .wait_allowing(
            |screen| screen.contains("Review your answers"),
            Some("Enter to select"),
        )
        .unwrap();
    fs::write(directory.join("06-review.screen.txt"), terminal.screen()).unwrap();
    eprintln!("multi Other review:\n{}", terminal.screen());
    assert!(
        terminal
            .screen()
            .lines()
            .any(|line| line.trim() == "❯ 1. Submit answers")
    );
    terminal.write(b"\r").unwrap();
    let path = probe.transcript(id).unwrap();
    terminal
        .wait_allowing(
            |_| {
                fs::read_to_string(&path)
                    .is_ok_and(|text| text.contains("\\\"Transport multi Other?\\\"=\\\""))
            },
            None,
        )
        .unwrap();
    fs::write(directory.join("transcript.jsonl"), fs::read(&path).unwrap()).unwrap();
    eprintln!("multi Other artifacts {}", directory.display());
    terminal.detach().unwrap();
    probe.cleanup().unwrap();
}
