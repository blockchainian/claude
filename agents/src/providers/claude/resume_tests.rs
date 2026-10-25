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
fn inactive_resume_commands_preserve_saved_background_options() {
    for template in [None, Some("bg"), Some("other")] {
        let (_directory, _provider, session) = inactive_session(template);
        let id = &session.id.id;
        let saved = template == Some("bg");

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
        ] {
            let mut expected = vec!["--bg"];
            // Settings still pass their explicit options and retain their existing behavior.
            if !saved || !matches!(input, SessionInput::Prompt(_)) {
                expected.push("--allow-dangerously-skip-permissions");
            }
            expected.extend(["--resume", id.as_str()]);
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
fn inactive_terminal_resume_preserves_saved_background_options() {
    for saved in [false, true] {
        let (_directory, provider, session) = inactive_session(saved.then_some("bg"));
        let id = session.id.id.as_str();
        let terminal = provider.terminal_command(&session).unwrap();
        let command = terminal.command;
        let script = if saved {
            r#"claude --bg --resume "$1" >/dev/null && exec claude attach "$2""#
        } else {
            r#"claude --bg --allow-dangerously-skip-permissions --resume "$1" >/dev/null && exec claude attach "$2""#
        };
        assert_eq!(command.get_program(), "sh");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["-c", script, "sh", id, &id[..8]]
        );
        assert_eq!(command.get_current_dir(), Some(session.cwd.as_path()));
        assert_eq!(terminal.detach, Some(DETACH));
    }
}
