//! Opt-in round trips against real Claude sessions with isolated configuration.
use super::*;
use crate::{app::live_tests as dashboard, providers::claude::ClaudeProvider};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct Live {
    root: tempfile::TempDir,
    config: PathBuf,
    jobs: Vec<String>,
}
impl Live {
    fn new() -> Self {
        assert!(
            std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_some(),
            "set CLAUDE_CODE_OAUTH_TOKEN for the isolated Claude service"
        );
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        fs::create_dir(&config).unwrap();
        fs::write(config.join("settings.json"), "{}").unwrap();
        let cwd = fs::canonicalize(root.path()).unwrap();
        fs::write(config.join(".claude.json"), json!({"hasCompletedOnboarding":true,"projects":{cwd.to_str().unwrap():{"hasTrustDialogAccepted":true}}}).to_string()).unwrap();
        Self {
            root,
            config,
            jobs: Vec::new(),
        }
    }
    fn command(&self, args: &[&str]) -> String {
        let output = tempfile::tempfile().unwrap();
        let errors = tempfile::tempfile().unwrap();
        let mut child = Command::new("claude")
            .args(args)
            .env("CLAUDE_CONFIG_DIR", &self.config)
            .current_dir(self.root.path())
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(errors.try_clone().unwrap())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("Claude command timed out");
            }
            thread::sleep(Duration::from_millis(50));
        };
        use std::io::{Read, Seek};
        let mut output = output;
        output.rewind().unwrap();
        let mut text = String::new();
        output.read_to_string(&mut text).unwrap();
        let mut errors = errors;
        errors.rewind().unwrap();
        errors.read_to_string(&mut text).unwrap();
        assert!(status.success(), "Claude command failed: {text}");
        text
    }
    fn start(&mut self, port: u16) -> String {
        let settings = json!({"hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[{"type":"http","url":format!("http://127.0.0.1:{port}/ask-user-question"),"timeout":3600}]}]}}).to_string();
        let text = self.command(&["--bg", "--model", "haiku", "--effort", "low", "--tools", "AskUserQuestion", "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}", "--setting-sources", "", "--settings", &settings, "Use AskUserQuestion exactly once. Before calling it, send exactly this message: I will ask you to choose a color. Ask question \"Hook color?\", header \"Color\", multiSelect false, options Red with description Red choice and Blue with description Blue choice. After my answer, reply DONE. This is an explicit interactive question test."]);
        let job = text
            .lines()
            .find_map(|line| line.split("backgrounded · ").nth(1))
            .expect("background job id")
            .trim()
            .to_owned();
        self.jobs.push(job.clone());
        job
    }
    fn state(&self, job: &str) -> Value {
        fs::read(self.config.join("jobs").join(job).join("state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null)
    }
    fn events(&self, id: &str) -> Vec<Value> {
        let Ok(projects) = fs::read_dir(self.config.join("projects")) else {
            return Vec::new();
        };
        for project in projects.flatten() {
            if let Ok(text) = fs::read_to_string(project.path().join(format!("{id}.jsonl"))) {
                return text
                    .lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect();
            }
        }
        Vec::new()
    }
}
impl Drop for Live {
    fn drop(&mut self) {
        for job in &self.jobs {
            self.command(&["stop", job]);
            self.command(&["rm", job]);
        }
        self.command(&["daemon", "stop", "--any"]);
    }
}
fn until<T>(message: &str, mut condition: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(value) = condition() {
            return value;
        }
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "starts billable Claude sessions; requires CLAUDE_CODE_OAUTH_TOKEN"]
fn real_ask_user_question_dashboard_answer_and_empty_response() {
    let mut live = Live::new();
    let server = Arc::new(AskUserQuestion::bind(0, live.config.clone()).unwrap());
    let job = live.start(server.port);
    let ask = until("real AskUserQuestion request never arrived", || {
        server.pending().into_iter().next()
    });
    assert_eq!(live.state(&job)["tempo"], "active");
    assert!(live.state(&job)["block"].is_null());
    let mut provider = ClaudeProvider::new(live.config.clone());
    provider.ask_user_question = Some(Arc::clone(&server));
    let mut app = dashboard::app(Box::new(provider), live.root.path().to_owned());
    let session = dashboard::session(&app, &ask.session.id);
    assert_eq!(session.status, crate::model::SessionStatus::NeedsInput);
    assert!(session.background_job.is_some());
    assert!(
        session.transcript.is_some(),
        "AskUserQuestion session has no transcript"
    );
    dashboard::answer_questions(&mut app, session, vec![vec!["Blue".into()]]);
    until(
        "real transcript never recorded AskUserQuestion answers",
        || {
            live.events(&ask.session.id)
                .iter()
                .any(|event| event["toolUseResult"]["answers"] == json!({"Hook color?":"Blue"}))
                .then_some(())
        },
    );
    assert!(server.pending().is_empty());
    let job = live.start(server.port);
    let ask = until("second real AskUserQuestion request never arrived", || {
        server.pending().into_iter().next()
    });
    server.release(&ask.session.id).unwrap();
    until(
        "empty HTTP response never restored Claude questions",
        || {
            live.state(&job)["block"]["questions"]
                .as_array()
                .filter(|questions| !questions.is_empty())
                .map(|_| ())
        },
    );
    assert!(server.pending().is_empty());
}

#[test]
#[ignore = "starts billable Claude sessions; requires CLAUDE_CODE_OAUTH_TOKEN"]
fn real_ask_user_question_dashboard_escape_resends_message() {
    let mut live = Live::new();
    let server = Arc::new(AskUserQuestion::bind(0, live.config.clone()).unwrap());
    live.start(server.port);
    let ask = until("real AskUserQuestion request never arrived", || {
        server.pending().into_iter().next()
    });
    let mut provider = ClaudeProvider::new(live.config.clone());
    provider.ask_user_question = Some(Arc::clone(&server));
    let mut app = dashboard::app(Box::new(provider), live.root.path().to_owned());
    let session = dashboard::session(&app, &ask.session.id);
    dashboard::reject_questions(&mut app, session);
    until("Claude did not continue after dashboard Esc", || {
        let events = live.events(&ask.session.id);
        let rejected = events.iter().position(|event| {
            event["message"]["content"].as_array().is_some_and(|blocks| blocks.iter().any(|block| {
                block["type"] == "tool_result" && block["tool_use_id"] == ask.id
                    && block["is_error"] == true
                    && block["content"].as_str().is_some_and(|text| text.starts_with("PreToolUse:AskUserQuestion hook error: The user dismissed these questions because the message you sent with them was not shown."))
            }))
        })?;
        events[rejected + 1..].iter().find_map(|event| {
            if event["type"] != "assistant" {
                return None;
            }
            let texts: Vec<_> = event["message"]["content"]
                .as_array()?
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect();
            let text = texts.join("\n");
            if text.is_empty() {
                return None;
            }
            println!("Claude after dashboard Esc: {text}");
            assert_eq!(text.trim(), "I will ask you to choose a color.");
            assert!(
                event["message"]["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|block| block["type"] != "tool_use")
            );
            Some(())
        })
    });
    assert!(server.pending().is_empty());
}
