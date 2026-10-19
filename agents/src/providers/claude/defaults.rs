use crate::tasks::TaskDefaults;
use serde_json::{Map, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

/// `ANTHROPIC_DEFAULT_MODEL` values Claude Code ignores.
const IGNORED_DEFAULT_MODELS: [&str; 4] = ["default", "inherit", "opusplan", "haiku"];

/// Claude Code's first-party account default and its model runtime's default effort.
pub(super) fn builtin() -> TaskDefaults {
    TaskDefaults {
        model: Some("claude-opus-5-5".into()),
        effort: Some("medium".into()),
    }
}

struct SettingsFile {
    values: Map<String, Value>,
    user: bool,
}

impl SettingsFile {
    fn string(&self, name: &str) -> io::Result<Option<String>> {
        string(&self.values, name)
    }

    /// Returns the effort this file sets for `model`: its `modelSettings` entry,
    /// then a top-level `effortLevel`, which Claude Code ignores in user settings.
    fn effort(&self, model: Option<&str>) -> io::Result<Option<String>> {
        let entry = match (self.values.get("modelSettings"), model) {
            (None, _) | (_, None) => None,
            (Some(Value::Object(entries)), Some(model)) => {
                entries.get(model.strip_suffix("[1m]").unwrap_or(model))
            }
            (Some(_), _) => return Err(invalid("modelSettings must be an object")),
        };
        if let Some(entry) = entry {
            let Value::Object(entry) = entry else {
                return Err(invalid("modelSettings entries must be objects"));
            };
            if let Some(level) = string(entry, "effortLevel")? {
                return Ok(Some(level));
            }
        }
        let level = self.string("effortLevel")?;
        Ok(level.filter(|_| !self.user))
    }
}

/// Resolves the model and effort `claude` would start a new session with in `cwd`.
pub(super) fn resolve(
    config_dir: &Path,
    cwd: &Path,
    env: impl Fn(&str) -> Option<String>,
) -> io::Result<TaskDefaults> {
    let files = settings_files(config_dir, cwd)?;
    let mut model = None;
    for file in &files {
        if let Some(value) = file.string("model")? {
            model.get_or_insert(value);
        }
    }
    let model = env("ANTHROPIC_MODEL")
        .filter(|value| !value.is_empty())
        .or(model)
        .or_else(|| {
            env("ANTHROPIC_DEFAULT_MODEL").filter(|value| {
                !value.is_empty() && !IGNORED_DEFAULT_MODELS.contains(&value.as_str())
            })
        })
        .or_else(|| builtin().model);
    let mut effort = None;
    for file in &files {
        if let Some(level) = file.effort(model.as_deref())? {
            effort.get_or_insert(level);
        }
    }
    let effort = match env("CLAUDE_CODE_EFFORT_LEVEL").filter(|value| !value.is_empty()) {
        Some(level) if level == "auto" => None,
        Some(level) => Some(level),
        None => effort,
    };
    Ok(TaskDefaults { model, effort })
}

/// Reads Claude's settings files in precedence order: local, shared project, user.
fn settings_files(config_dir: &Path, cwd: &Path) -> io::Result<Vec<SettingsFile>> {
    let mut paths = vec![(cwd.join(".claude/settings.json"), false)];
    let legacy_local = cwd.join(".claude/settings.local.json");
    let local = match main_checkout(cwd)? {
        Some(root) => root.join(".claude/settings.local.json"),
        None => legacy_local.clone(),
    };
    if local != legacy_local {
        paths.insert(0, (legacy_local, false));
    }
    paths.insert(0, (local, false));
    paths.push((config_dir.join("settings.json"), true));
    let mut files = Vec::new();
    for (path, user) in paths {
        if let Some(values) = read(&path)? {
            files.push(SettingsFile { values, user });
        }
    }
    Ok(files)
}

/// Returns the main checkout's root, where Claude Code keeps the local settings file in a git repository.
fn main_checkout(cwd: &Path) -> io::Result<Option<PathBuf>> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let common = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    Ok(common
        .file_name()
        .filter(|name| *name == ".git")
        .and_then(|_| common.parent())
        .map(Path::to_path_buf))
}

fn read(path: &Path) -> io::Result<Option<Map<String, Value>>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match serde_json::from_slice(&bytes)? {
        Value::Object(values) => Ok(Some(values)),
        _ => Err(invalid("Claude settings must be a JSON object")),
    }
}

fn string(values: &Map<String, Value>, name: &str) -> io::Result<Option<String>> {
    match values.get(name) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(&format!("Claude setting {name} must be a string"))),
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}
