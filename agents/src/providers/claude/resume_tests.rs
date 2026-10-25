use super::*;

// Relevant fields from a real Claude Code 2.1.309 jobs/<id>/state.json.
const STATE: &str = include_str!("../../../tests/fixtures/claude-stopped-background.json");

fn inactive_session(template: Option<&str>) -> (tempfile::TempDir, ClaudeProvider, Session) {
    let directory = tempfile::tempdir().unwrap();
    let mut state: serde_json::Value = serde_json::from_str(STATE).unwrap();
    let id = state["sessionId"].as_str().unwrap().to_owned();
    tests::transcript(directory.path(), &id, 0);
    if let Some(template) = template {
        state["template"] = template.into();
        let job = directory.path().join("jobs").join(&id[..8]);
        fs::create_dir_all(&job).unwrap();
        fs::write(job.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
    }
    let mut provider = ClaudeProvider::new(directory.path().into());
    let session = provider.refresh(1).sessions.remove(0);
    assert!(session.process.is_none());
    assert_eq!(session.background_job.is_some(), template == Some("bg"));
    (directory, provider, session)
}

#[test]
fn inactive_resume_commands_pass_only_background() {
    for template in [None, Some("bg"), Some("other")] {
        let (_directory, _provider, session) = inactive_session(template);
        let id = &session.id.id;

        for (input, suffix) in [
            (
                SessionInput::Prompt("Continue the test".into()),
                vec!["Continue the test"],
            ),
            (
                SessionInput::SetModel("sonnet".into()),
                vec!["--model", "sonnet"],
            ),
            (
                SessionInput::SetEffort("medium".into()),
                vec!["--effort", "medium"],
            ),
            (
                SessionInput::PromptWithImages {
                    text: "Describe this image".into(),
                    images: vec![PathBuf::from("image.png")],
                },
                vec![],
            ),
        ] {
            let mut expected = vec!["--bg", "--resume", id.as_str()];
            expected.extend(suffix);
            let command = resume_input(&session, &input).unwrap();
            assert_eq!(command.get_program(), "claude");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                expected,
                "{template:?}"
            );
            assert_eq!(command.get_current_dir(), Some(session.cwd.as_path()));
        }
    }
}

#[test]
fn inactive_terminal_resume_passes_only_background() {
    for saved in [false, true] {
        let (_directory, provider, session) = inactive_session(saved.then_some("bg"));
        let id = session.id.id.as_str();
        let terminal = provider.terminal_command(&session).unwrap();
        let command = terminal.command;
        let script = r#"claude --bg --resume "$1" >/dev/null && exec claude attach "$2""#;
        assert_eq!(command.get_program(), "sh");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["-c", script, "sh", id, &id[..8]]
        );
        assert_eq!(command.get_current_dir(), Some(session.cwd.as_path()));
        assert_eq!(terminal.detach, Some(DETACH));
    }
}

#[test]
#[ignore = "requires a dedicated stopped Claude test session and a real PNG"]
fn real_inactive_image_resume() {
    use serde_json::Value;
    use std::time::{Duration, Instant};

    let id = std::env::var("AGENTS_INACTIVE_IMAGE_SESSION").unwrap();
    let cwd = PathBuf::from(std::env::var_os("AGENTS_INACTIVE_IMAGE_CWD").unwrap());
    let image = PathBuf::from(std::env::var_os("AGENTS_INACTIVE_IMAGE_PNG").unwrap());
    let evidence = PathBuf::from(std::env::var_os("AGENTS_INACTIVE_IMAGE_EVIDENCE").unwrap());
    let config = config_dir();
    let state_path = config.join("jobs").join(&id[..8]).join("state.json");
    let state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(state["sessionId"], id);
    let mut provider = ClaudeProvider::new(config);
    let session = provider
        .refresh(1)
        .sessions
        .into_iter()
        .find(|session| session.id.id == id)
        .expect("dedicated test session");
    assert_eq!(session.cwd, cwd);
    assert!(session.process.is_none());
    let transcript = session.transcript.as_ref().unwrap();
    let start = fs::metadata(transcript).unwrap().len() as usize;
    let text = "INACTIVE_IMAGE_RESUME: Describe the image color and shape. Do not use tools.";
    provider
        .send(
            &session,
            SessionInput::PromptWithImages {
                text: text.into(),
                images: vec![image.clone()],
            },
        )
        .unwrap()()
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let bytes = fs::read(transcript).unwrap();
        let records: Vec<Value> = std::str::from_utf8(&bytes[start..])
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if let Some(index) = records.iter().position(|record| {
            record["type"] == "user"
                && record["message"]["content"]
                    .as_array()
                    .is_some_and(|blocks| {
                        blocks.iter().any(|block| {
                            block["text"]
                                .as_str()
                                .is_some_and(|value| value.ends_with(text))
                        }) && blocks.iter().any(|block| {
                            block["type"] == "image"
                                && block["source"]["media_type"] == "image/png"
                                && block["source"]["data"]
                                    == base64::Engine::encode(
                                        &base64::engine::general_purpose::STANDARD,
                                        fs::read(&image).unwrap(),
                                    )
                        })
                    })
        }) && let Some(reply) = records[index + 1..].iter().find(|record| {
            record["type"] == "assistant"
                && record["message"]["content"]
                    .as_array()
                    .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "text"))
        }) {
            let state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
            assert_eq!(state["sessionId"], id);
            assert_eq!(records[index]["sessionId"], id);
            assert_eq!(reply["sessionId"], id);
            fs::write(&evidence, &bytes[start..]).unwrap();
            eprintln!(
                "session={id} transcript={} reply={}",
                transcript.display(),
                reply["message"]["content"]
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no image message and model reply"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn resumed_image_job_parses_real_idle_output() {
    use std::os::unix::process::ExitStatusExt;
    let output = std::process::Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: include_bytes!("../../../tests/fixtures/claude-resume-image-idle.txt").to_vec(),
        stderr: Vec::new(),
    };
    assert_eq!(created_job(&output).unwrap(), "633a1b52");
}
