use serde_json::{Map, Value};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

const DEFAULT_KEYS: [&str; 2] = ["model", "effortLevel"];
static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);

struct Settings {
    values: Map<String, Value>,
    permissions: Permissions,
}

fn read(path: &Path) -> io::Result<Option<Settings>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let values = match value {
        Value::Object(values) => values,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude settings must be a JSON object",
            ));
        }
    };
    Ok(Some(Settings {
        values,
        permissions: fs::metadata(path)?.permissions(),
    }))
}

fn write(
    path: &Path,
    values: &Map<String, Value>,
    permissions: Option<Permissions>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let filename = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Settings path has no filename")
    })?;
    let mut name = OsString::from(".");
    name.push(filename);
    name.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary = parent.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        serde_json::to_writer_pretty(&mut file, values)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        return match fs::remove_file(&temporary) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(io::Error::new(
                error.kind(),
                format!("{error}; cannot remove temporary settings: {cleanup}"),
            )),
        };
    }
    Ok(())
}

/// Registers AskUserQuestion without rewriting settings that already contain it.
pub(super) fn ensure_ask_user_question(path: &Path, port: u16) -> io::Result<()> {
    let settings = read(path)?;
    let permissions = settings
        .as_ref()
        .map(|settings| settings.permissions.clone());
    let mut values = settings.map(|settings| settings.values).unwrap_or_default();
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid Claude PreToolUse hooks",
        )
    };
    let hooks = values
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(invalid)?;
    let entries = hooks
        .entry("PreToolUse")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(invalid)?;
    let url = format!("http://127.0.0.1:{port}/ask-user-question");
    let mut exists = false;
    for entry in entries.iter() {
        let entry = entry.as_object().ok_or_else(invalid)?;
        let matcher = match entry.get("matcher") {
            None => None,
            Some(Value::String(matcher)) => Some(matcher.as_str()),
            Some(_) => return Err(invalid()),
        };
        let actions = entry
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        for action in actions {
            let action = action.as_object().ok_or_else(invalid)?;
            if matcher == Some("AskUserQuestion")
                && action.get("type").and_then(Value::as_str) == Some("http")
                && action.get("url").and_then(Value::as_str) == Some(url.as_str())
                && action.get("timeout").and_then(Value::as_u64) == Some(3600)
            {
                exists = true;
            }
        }
    }
    if exists {
        return Ok(());
    }
    entries.push(serde_json::json!({
        "matcher": "AskUserQuestion",
        "hooks": [{"type": "http", "url": url, "timeout": 3600}]
    }));
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    write(path, &values, permissions)
}

/// Restores Claude's global model and effort defaults after an operation changes them.
pub(super) fn with_restored_defaults<T>(
    path: &Path,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let before = read(path)?;
    let defaults = DEFAULT_KEYS.map(|key| {
        before
            .as_ref()
            .and_then(|settings| settings.values.get(key))
            .cloned()
    });
    let result = operation();
    let restore = (|| {
        let current = read(path)?;
        if current.is_none() && defaults.iter().all(Option::is_none) {
            return Ok(());
        }
        let permissions = current
            .as_ref()
            .or(before.as_ref())
            .map(|settings| settings.permissions.clone());
        let mut values = current.map(|settings| settings.values).unwrap_or_default();
        let mut changed = false;
        for (key, previous) in DEFAULT_KEYS.into_iter().zip(defaults) {
            if values.get(key) != previous.as_ref() {
                changed = true;
            }
            if let Some(previous) = previous {
                values.insert(key.to_owned(), previous);
            } else {
                values.remove(key);
            }
        }
        if changed {
            write(path, &values, permissions)?;
        }
        Ok::<_, io::Error>(())
    })();
    match (result, restore) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(error),
        (Err(operation), Err(restore)) => Err(io::Error::new(
            restore.kind(),
            format!("Cannot restore Claude defaults: {restore}; operation failed: {operation}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Safe default fields copied from an actual Claude Code settings payload.
    const SETTINGS: &str = r#"{
  "model": "claude-opus-5-5",
  "effortLevel": "high"
}
"#;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(&path, SETTINGS).unwrap();
        (directory, path)
    }
    fn read(path: &Path) -> Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
    fn write(path: &Path, value: &Value) {
        fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    #[test]
    fn restores_present_defaults_and_keeps_unrelated_operation_updates() {
        let (_directory, path) = fixture();
        let before = read(&path);
        let result = with_restored_defaults(&path, || {
            let mut settings = read(&path);
            settings["model"] = json!("haiku");
            settings["effortLevel"] = json!("low");
            settings["operation_settings"] = json!({"nested": [1, "保留", true]});
            write(&path, &settings);
            Ok(42)
        })
        .unwrap();
        assert_eq!(result, 42);
        let after = read(&path);
        assert_eq!(after["model"], before["model"]);
        assert_eq!(after["effortLevel"], before["effortLevel"]);
        assert_eq!(
            after["operation_settings"],
            json!({"nested": [1, "保留", true]})
        );
    }

    #[test]
    fn removes_defaults_that_were_absent_and_keeps_created_file() {
        let (_directory, path) = fixture();
        fs::remove_file(&path).unwrap();
        with_restored_defaults(&path, || {
            let mut settings: Value = serde_json::from_str(SETTINGS).unwrap();
            settings["operation_settings"] = json!(["keep", 2]);
            write(&path, &settings);
            Ok(())
        })
        .unwrap();
        let after = read(&path);
        assert!(after.get("model").is_none());
        assert!(after.get("effortLevel").is_none());
        assert_eq!(after["operation_settings"], json!(["keep", 2]));
    }

    #[test]
    fn restores_after_operation_error_and_retains_other_keys() {
        let (_directory, path) = fixture();
        let before = read(&path);
        let error = with_restored_defaults::<()>(&path, || {
            let mut settings = read(&path);
            settings["model"] = json!("sonnet");
            settings["effortLevel"] = json!("max");
            settings["concurrent"] = json!("retained");
            write(&path, &settings);
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "attach interrupted",
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        let after = read(&path);
        assert_eq!(after["model"], before["model"]);
        assert_eq!(after["effortLevel"], before["effortLevel"]);
        assert_eq!(after["concurrent"], "retained");
    }

    #[test]
    fn malformed_before_operation_fails_without_running_it() {
        let (_directory, path) = fixture();
        fs::write(&path, b"{broken").unwrap();
        let mut ran = false;
        assert!(
            with_restored_defaults(&path, || {
                ran = true;
                Ok(())
            })
            .is_err()
        );
        assert!(!ran);
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
    }

    #[test]
    fn malformed_after_operation_is_reported_without_overwriting_it() {
        let (_directory, path) = fixture();
        assert!(
            with_restored_defaults(&path, || {
                fs::write(&path, b"{broken")?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
        for invalid in ["null", "[]", "1"] {
            fs::write(&path, invalid).unwrap();
            let mut ran = false;
            assert!(
                with_restored_defaults(&path, || {
                    ran = true;
                    Ok(())
                })
                .is_err()
            );
            assert!(!ran);
        }
    }

    #[test]
    fn restore_preserves_permissions_and_leaves_no_atomic_temporary_file() {
        let (directory, path) = fixture();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        with_restored_defaults(&path, || {
            let mut settings = read(&path);
            settings["model"] = json!("haiku");
            write(&path, &settings);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn restores_independently_absent_keys_and_leaves_absent_file_absent() {
        let (_directory, path) = fixture();
        let mut settings = read(&path);
        let model = settings["model"].clone();
        settings.as_object_mut().unwrap().remove("effortLevel");
        write(&path, &settings);
        with_restored_defaults(&path, || {
            let mut current = read(&path);
            current["model"] = json!("haiku");
            current["effortLevel"] = json!("low");
            write(&path, &current);
            Ok(())
        })
        .unwrap();
        assert_eq!(read(&path)["model"], model);
        assert!(read(&path).get("effortLevel").is_none());
        fs::remove_file(&path).unwrap();
        with_restored_defaults(&path, || Ok(())).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn ask_user_question_adds_http_configuration_and_preserves_settings_and_permissions() {
        let (directory, path) = fixture();
        let mut before = read(&path);
        before["permissions"] = json!({"allow": ["Read"]});
        before["hooks"] = json!({"Stop": [{"hooks": [{"type": "command", "command": "true"}]}]});
        write(&path, &before);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        ensure_ask_user_question(&path, 31337).unwrap();

        let after = read(&path);
        for key in ["model", "effortLevel", "permissions"] {
            assert_eq!(after[key], before[key]);
        }
        assert_eq!(after["hooks"]["Stop"], before["hooks"]["Stop"]);
        assert_eq!(
            after["hooks"]["PreToolUse"],
            json!([{
                "matcher": "AskUserQuestion",
                "hooks": [{"type": "http", "url": "http://127.0.0.1:31337/ask-user-question", "timeout": 3600}]
            }])
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn ask_user_question_keeps_existing_equivalent_configuration_byte_for_byte() {
        let (_directory, path) = fixture();
        let existing = r#"{ "model":"haiku", "hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[{"type":"command","command":"true"},{"type":"http","url":"http://127.0.0.1:31337/ask-user-question","timeout":3600,"headers":{"X-Test":"retained"}}]}]}}"#;
        fs::write(&path, existing).unwrap();
        ensure_ask_user_question(&path, 31337).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), existing);
    }

    #[test]
    fn ask_user_question_creates_missing_directory_and_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config/settings.json");
        ensure_ask_user_question(&path, 31337).unwrap();
        let settings = read(&path);
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["matcher"],
            "AskUserQuestion"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn ask_user_question_rejects_malformed_structure_without_changing_file() {
        let (_directory, path) = fixture();
        for invalid in [
            "{broken",
            "[]",
            r#"{"hooks":null}"#,
            r#"{"hooks":{"PreToolUse":{}}}"#,
            r#"{"hooks":{"PreToolUse":[null]}}"#,
            r#"{"hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":null}]}}"#,
            r#"{"hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[false]}]}}"#,
        ] {
            fs::write(&path, invalid).unwrap();
            assert_eq!(
                ensure_ask_user_question(&path, 31337).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        }
    }
}
