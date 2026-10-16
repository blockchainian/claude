use super::*;
use crate::tasks::NewTask;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

// Verbatim successful output excerpt from the real Claude Code 2.1.299 capture.
const BACKGROUND_OUTPUT: &str = "backgrounded · d887474c\n  claude agents             list sessions\n  claude attach d887474c    open in this terminal\n  claude logs d887474c      show recent output\n  claude stop d887474c      stop this session\n\n";

#[test]
fn create_uses_task_directory_and_optional_model_effort_before_prompt() {
    let directory = tempfile::tempdir().unwrap();
    let provider = ClaudeProvider::new(directory.path().into());
    let mut request = NewTask {
        cwd: directory.path().into(),
        model: Some("haiku".into()),
        effort: Some("high".into()),
        prompt: include_str!("../../../tests/fixtures/claude-peer-message.txt").into(),
    };
    let command = provider.create(&request).unwrap().unwrap();
    assert_eq!(command.get_program(), "claude");
    assert_eq!(command.get_current_dir(), Some(directory.path()));
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "--bg",
            "--model",
            "haiku",
            "--effort",
            "high",
            request.prompt.as_str()
        ]
    );
    request.model = None;
    request.effort = None;
    let command = provider.create(&request).unwrap().unwrap();
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["--bg", request.prompt.as_str()]
    );
}

#[test]
fn task_defaults_read_real_settings_fields_without_inventing_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let provider = ClaudeProvider::new(directory.path().into());
    // Safe values copied from an actual Claude Code settings payload.
    fs::write(
        directory.path().join("settings.json"),
        r#"{"model":"claude-opus-5-5","effortLevel":"high"}"#,
    )
    .unwrap();
    let defaults = provider.task_defaults().unwrap().unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(defaults.effort.as_deref(), Some("high"));
    fs::remove_file(directory.path().join("settings.json")).unwrap();
    let defaults = provider.task_defaults().unwrap().unwrap();
    assert_eq!(defaults.model, None);
    assert_eq!(defaults.effort, None);
}

#[test]
fn malformed_settings_are_reported() {
    let directory = tempfile::tempdir().unwrap();
    let provider = ClaudeProvider::new(directory.path().into());
    for text in [
        "{broken",
        "[]",
        r#"{"model":3}"#,
        r#"{"effortLevel":false}"#,
        r#"{"model":null}"#,
    ] {
        fs::write(directory.path().join("settings.json"), text).unwrap();
        assert!(provider.task_defaults().unwrap().is_err(), "{text}");
    }
}

#[test]
fn creation_parses_verbatim_background_output_and_reports_invalid_results() {
    let directory = tempfile::tempdir().unwrap();
    let provider = ClaudeProvider::new(directory.path().into());
    let mut output = Output {
        status: ExitStatus::from_raw(0),
        stdout: BACKGROUND_OUTPUT.as_bytes().to_vec(),
        stderr: Vec::new(),
    };
    assert_eq!(provider.created_job(&output).unwrap(), "d887474c");
    output
        .stdout
        .extend_from_slice(BACKGROUND_OUTPUT.as_bytes());
    assert!(provider.created_job(&output).is_err());
    output.stdout = BACKGROUND_OUTPUT.as_bytes().to_vec();
    output.status = ExitStatus::from_raw(256);
    assert!(provider.created_job(&output).is_err());
    output.status = ExitStatus::from_raw(0);
    output.stdout = BACKGROUND_OUTPUT
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    assert!(provider.created_job(&output).is_err());
}

#[test]
fn claude_input_choices_are_the_supported_model_and_effort_names() {
    let directory = tempfile::tempdir().unwrap();
    let choices = ClaudeProvider::new(directory.path().into()).input_choices();
    assert_eq!(choices.models, ["fable", "opus", "sonnet", "haiku"]);
    assert_eq!(choices.efforts, ["low", "medium", "high", "xhigh", "max"]);
}
