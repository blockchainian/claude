use super::*;
use crate::tasks::NewTask;
use std::os::unix::process::ExitStatusExt;
use std::process::{ExitStatus, Output};

// Verbatim successful output excerpt from the real Claude Code 2.1.299 capture.
const BACKGROUND_OUTPUT: &str = "backgrounded · d887474c\n  claude agents             list sessions\n  claude attach d887474c    open in this terminal\n  claude logs d887474c      show recent output\n  claude stop d887474c      stop this session\n\n";

#[test]
fn image_creation_reports_the_job_before_sending_attachments() {
    let directory = tempfile::tempdir().unwrap();
    let output = Output {
        status: ExitStatus::from_raw(0),
        stdout: include_bytes!("../../../tests/fixtures/claude-background-idle.txt").to_vec(),
        stderr: Vec::new(),
    };
    let reported = std::cell::RefCell::new(None);
    let request = NewTask {
        cwd: directory.path().into(),
        name: None,
        model: None,
        effort: None,
        prompt: "reply ok".into(),
        images: vec![directory.path().join("missing.png")],
    };
    let error = finish_creation(
        output,
        request,
        &directory.path().join("settings.json"),
        &|id| {
            *reported.borrow_mut() = Some(id);
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(reported.into_inner().as_deref(), Some("6f03db53"));
}

#[test]
fn create_uses_task_directory_and_optional_model_effort_before_prompt() {
    let directory = tempfile::tempdir().unwrap();
    let mut request = NewTask {
        cwd: directory.path().into(),
        name: Some("改 Claude Code 的颜色".into()),
        model: Some("haiku".into()),
        effort: Some("high".into()),
        images: Vec::new(),
        prompt: include_str!("../../../tests/fixtures/claude-peer-message.txt").into(),
    };
    let command = ClaudeProvider::create_command(&request);
    assert_eq!(command.get_program(), "claude");
    assert_eq!(command.get_current_dir(), Some(directory.path()));
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            "--bg",
            "--name",
            "改 Claude Code 的颜色",
            "--model",
            "haiku",
            "--effort",
            "high",
            request.prompt.as_str()
        ]
    );
    request.name = None;
    request.model = None;
    request.effort = None;
    let command = ClaudeProvider::create_command(&request);
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["--bg", request.prompt.as_str()]
    );
}

#[test]
fn image_creation_starts_idle_with_model_and_effort() {
    let directory = tempfile::tempdir().unwrap();
    let request = NewTask {
        cwd: directory.path().into(),
        name: None,
        model: Some("haiku".into()),
        effort: Some("high".into()),
        prompt: "describe this".into(),
        images: vec![directory.path().join("image.png")],
    };
    let command = ClaudeProvider::create_command(&request);
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        ["--bg", "--model", "haiku", "--effort", "high"]
    );
}

fn resolve_defaults(
    config: &Path,
    cwd: &Path,
    env: &[(&str, &str)],
) -> io::Result<crate::tasks::TaskDefaults> {
    defaults::resolve(config, cwd, |name| {
        env.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    })
}

fn write_settings(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn task_defaults_use_the_saved_level_for_the_effective_model() {
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    // Shape of an actual Claude Code user settings payload.
    write_settings(
        &config.path().join("settings.json"),
        r#"{"model":"claude-opus-5-5","effortLevel":"high","modelSettings":{"claude-opus-5":{"effortLevel":"high"},"claude-opus-5-5":{"effortLevel":"xhigh"}}}"#,
    );
    let defaults = resolve_defaults(config.path(), project.path(), &[]).unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(defaults.effort.as_deref(), Some("xhigh"));
    // A top-level user effortLevel does not apply to Opus 5.5.
    write_settings(
        &config.path().join("settings.json"),
        r#"{"model":"claude-opus-5-5","effortLevel":"high"}"#,
    );
    let defaults = resolve_defaults(config.path(), project.path(), &[]).unwrap();
    assert_eq!(defaults.effort, None);
    fs::remove_file(config.path().join("settings.json")).unwrap();
    let defaults = resolve_defaults(config.path(), project.path(), &[]).unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(defaults.effort, None);
}

#[test]
fn task_defaults_follow_claude_settings_precedence() {
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    write_settings(
        &config.path().join("settings.json"),
        r#"{"model":"claude-opus-5-5","modelSettings":{"claude-opus-5-5":{"effortLevel":"xhigh"},"claude-fable-5-1":{"effortLevel":"max"}}}"#,
    );
    write_settings(
        &project.path().join(".claude/settings.json"),
        r#"{"model":"claude-fable-5-1","effortLevel":"low"}"#,
    );
    let defaults = resolve_defaults(config.path(), project.path(), &[]).unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-fable-5-1"));
    assert_eq!(defaults.effort.as_deref(), Some("low"));
    write_settings(
        &project.path().join(".claude/settings.local.json"),
        r#"{"model":"claude-opus-5-5[1m]","modelSettings":{"claude-opus-5-5":{"effortLevel":"medium"}}}"#,
    );
    let defaults = resolve_defaults(config.path(), project.path(), &[]).unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5[1m]"));
    assert_eq!(defaults.effort.as_deref(), Some("medium"));
}

#[test]
fn task_defaults_read_the_local_file_at_the_repository_root() {
    let config = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    assert!(status.success());
    let subdirectory = repository.path().join("sub");
    fs::create_dir(&subdirectory).unwrap();
    write_settings(
        &repository.path().join(".claude/settings.local.json"),
        r#"{"model":"claude-opus-5","effortLevel":"low"}"#,
    );
    write_settings(
        &subdirectory.join(".claude/settings.local.json"),
        r#"{"model":"claude-fable-5","effortLevel":"high"}"#,
    );
    let defaults = resolve_defaults(config.path(), &subdirectory, &[]).unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5"));
    assert_eq!(defaults.effort.as_deref(), Some("low"));
}

#[test]
fn task_defaults_apply_claude_code_environment_variables() {
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let defaults = resolve_defaults(
        config.path(),
        project.path(),
        &[("ANTHROPIC_DEFAULT_MODEL", "claude-opus-5")],
    )
    .unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5"));
    let defaults = resolve_defaults(
        config.path(),
        project.path(),
        &[("ANTHROPIC_DEFAULT_MODEL", "opusplan")],
    )
    .unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
    write_settings(
        &config.path().join("settings.json"),
        r#"{"model":"claude-opus-5-5","modelSettings":{"claude-opus-5-5":{"effortLevel":"xhigh"}}}"#,
    );
    let defaults = resolve_defaults(
        config.path(),
        project.path(),
        &[("ANTHROPIC_DEFAULT_MODEL", "claude-opus-5")],
    )
    .unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
    let defaults = resolve_defaults(
        config.path(),
        project.path(),
        &[
            ("ANTHROPIC_MODEL", "claude-fable-5-1"),
            ("CLAUDE_CODE_EFFORT_LEVEL", "max"),
        ],
    )
    .unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-fable-5-1"));
    assert_eq!(defaults.effort.as_deref(), Some("max"));
    let defaults = resolve_defaults(
        config.path(),
        project.path(),
        &[("CLAUDE_CODE_EFFORT_LEVEL", "auto")],
    )
    .unwrap();
    assert_eq!(defaults.effort, None);
}

#[test]
fn malformed_settings_are_reported() {
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    for text in [
        "{broken",
        "[]",
        r#"{"model":3}"#,
        r#"{"effortLevel":false}"#,
        r#"{"model":null}"#,
        r#"{"model":"claude-opus-5-5","modelSettings":[]}"#,
        r#"{"model":"claude-opus-5-5","modelSettings":{"claude-opus-5-5":"high"}}"#,
    ] {
        write_settings(&project.path().join(".claude/settings.json"), text);
        assert!(
            resolve_defaults(config.path(), project.path(), &[]).is_err(),
            "{text}"
        );
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
fn claude_input_choices_come_only_from_the_model_cache() {
    let directory = tempfile::tempdir().unwrap();
    let provider = ClaudeProvider::new(directory.path().into());
    assert!(provider.input_choices().models.is_empty());
    super::write_fixture_cache(directory.path());
    assert_eq!(provider.input_choices().models[0].id, "claude-opus-5-5");
}

#[test]
fn creation_parses_idle_background_output() {
    let output = Output {
        status: ExitStatus::from_raw(0),
        stdout: include_bytes!("../../../tests/fixtures/claude-background-idle.txt").to_vec(),
        stderr: Vec::new(),
    };
    assert_eq!(created_job(&output).unwrap(), "6f03db53");
}

#[test]
fn task_defaults_fall_back_to_the_cached_models_own_effort() {
    let directory = tempfile::tempdir().unwrap();
    let provider = ClaudeProvider::new(directory.path().into());
    std::fs::write(
        directory.path().join("settings.json"),
        r#"{"model":"claude-fable-5-1"}"#,
    )
    .unwrap();
    let defaults = || provider.task_defaults(directory.path()).unwrap().unwrap();
    assert_eq!(defaults().effort, None);
    super::write_fixture_cache(directory.path());
    assert_eq!(defaults().model.as_deref(), Some("claude-fable-5-1"));
    assert_eq!(defaults().effort.as_deref(), Some("high"));
}

#[test]
fn creation_parses_named_background_output() {
    let output = Output {
        status: ExitStatus::from_raw(0),
        stdout: include_bytes!("../../../tests/fixtures/claude-background-named.txt").to_vec(),
        stderr: Vec::new(),
    };
    assert_eq!(created_job(&output).unwrap(), "17935055");
}

#[test]
fn unconfigured_task_uses_claude_codes_builtin_model_and_effort() {
    let directory = tempfile::tempdir().unwrap();
    let provider = ClaudeProvider::new(directory.path().into());
    let defaults = provider.task_defaults(directory.path()).unwrap().unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(defaults.effort.as_deref(), Some("medium"));
}

#[test]
fn builtin_model_uses_its_saved_effort_without_a_model_override() {
    let directory = tempfile::tempdir().unwrap();
    write_settings(
        &directory.path().join("settings.json"),
        r#"{"modelSettings":{"claude-opus-5-5":{"effortLevel":"xhigh"}}}"#,
    );
    let provider = ClaudeProvider::new(directory.path().into());
    let defaults = provider.task_defaults(directory.path()).unwrap().unwrap();
    assert_eq!(defaults.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(defaults.effort.as_deref(), Some("xhigh"));
}
