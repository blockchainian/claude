//! Cross-provider ownership and pending requests recorded by codex-manager.

use super::{ProviderId, SessionKey};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
struct InstalledPlugins {
    plugins: HashMap<String, Vec<InstalledPlugin>>,
}

#[derive(Deserialize)]
struct InstalledPlugin {
    scope: String,
    #[serde(rename = "installPath")]
    install_path: PathBuf,
}

pub(super) fn detach_command(
    config: &Path,
    state: &Path,
    codex_home: &Path,
    thread_id: &str,
) -> io::Result<Command> {
    let installed: InstalledPlugins =
        serde_json::from_slice(&fs::read(config.join("plugins/installed_plugins.json"))?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let plugin = installed
        .plugins
        .get("codex@blockchainian")
        .and_then(|entries| entries.iter().find(|entry| entry.scope == "user"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "codex@blockchainian is not installed at user scope",
            )
        })?;
    let mut command = Command::new("node");
    command
        .arg(plugin.install_path.join("codex-manager/manager.mjs"))
        .args(["detach", "--thread", thread_id])
        .env("CODEX_MANAGER_STATE_DIR", state)
        .env("CODEX_HOME", codex_home);
    Ok(command)
}

pub(super) struct Link {
    pub parent: SessionKey,
    pub name: Option<String>,
    pub needs_input: bool,
}

#[derive(Deserialize)]
struct Supervisor {
    #[serde(rename = "sessionId")]
    session_id: String,
    pid: u32,
}

#[derive(Deserialize)]
struct State {
    threads: Vec<Thread>,
}

#[derive(Deserialize)]
struct Thread {
    id: String,
    name: Option<String>,
    #[serde(default)]
    detached: bool,
    #[serde(default)]
    waiting: Vec<serde_json::Value>,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {error}", path.display()),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn links(home: &Path, claude_config: &Path) -> io::Result<HashMap<String, Link>> {
    let entries = match fs::read_dir(home.join("threads")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let mut states = HashMap::new();
    let mut links = HashMap::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(supervisor) = read::<Supervisor>(&path)? else {
            continue;
        };
        let parent_id = match crate::processes::parent_pid(supervisor.pid) {
            Some(pid) => read::<super::super::claude::registry::Entry>(
                &claude_config.join("sessions").join(format!("{pid}.json")),
            )?
            .and_then(|entry| entry.session_id)
            .filter(|id| !id.is_empty()),
            None => None,
        }
        .unwrap_or_else(|| supervisor.session_id.clone());
        let session_id = supervisor.session_id;
        if !states.contains_key(&session_id) {
            let dir = home.join(&session_id);
            let state = read::<State>(&dir.join("state.json"))?;
            let mut threads = HashMap::new();
            if let Some(state) = state {
                let files = fs::read_dir(&dir)?
                    .map(|entry| {
                        entry.map(|entry| entry.file_name().to_string_lossy().into_owned())
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                for thread in state.threads.into_iter().filter(|thread| !thread.detached) {
                    let prefix = format!("{}.ask.", thread.id);
                    let needs_input = !thread.waiting.is_empty()
                        || files
                            .iter()
                            .any(|name| name.starts_with(&prefix) && name.ends_with(".json"));
                    threads.insert(thread.id, (thread.name, needs_input));
                }
            }
            states.insert(session_id.clone(), threads);
        }
        let Some(id) = path.file_stem().and_then(|id| id.to_str()) else {
            continue;
        };
        if let Some((name, needs_input)) = states[&session_id].get(id) {
            links.insert(
                id.to_owned(),
                Link {
                    parent: SessionKey {
                        provider: ProviderId::Claude,
                        id: parent_id,
                    },
                    name: name.clone().filter(|name| !name.trim().is_empty()),
                    needs_input: *needs_input,
                },
            );
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detach_uses_the_installed_user_plugin_and_provider_directories() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("claude");
        fs::create_dir_all(config.join("plugins")).unwrap();
        let record = config.join("plugins/installed_plugins.json");
        let mut installed: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/codex-manager/installed-plugins.json"
        ))
        .unwrap();
        for path in ["plugin cache/codex/first", "plugin cache/codex/second"] {
            let install = directory.path().join(path);
            installed["plugins"]["codex@blockchainian"][0]["installPath"] =
                install.to_str().unwrap().into();
            fs::write(&record, installed.to_string()).unwrap();
            let state = directory.path().join("manager");
            let home = directory.path().join("codex");
            let command = detach_command(&config, &state, &home, "thread-12").unwrap();
            assert_eq!(command.get_program(), "node");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                [
                    install.join("codex-manager/manager.mjs").as_os_str(),
                    "detach".as_ref(),
                    "--thread".as_ref(),
                    "thread-12".as_ref(),
                ]
            );
            assert_eq!(
                command.get_envs().collect::<HashMap<_, _>>(),
                HashMap::from([
                    ("CODEX_HOME".as_ref(), Some(home.as_os_str())),
                    ("CODEX_MANAGER_STATE_DIR".as_ref(), Some(state.as_os_str())),
                ])
            );
        }
        installed["plugins"].as_object_mut().unwrap().clear();
        fs::write(&record, installed.to_string()).unwrap();
        assert_eq!(
            detach_command(&config, directory.path(), directory.path(), "thread-12")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        fs::write(&record, "invalid").unwrap();
        assert_eq!(
            detach_command(&config, directory.path(), directory.path(), "thread-12")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn follows_current_claude_session_for_every_thread_and_falls_back() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("manager");
        let config = directory.path().join("claude");
        fs::create_dir_all(home.join("threads")).unwrap();
        fs::create_dir_all(config.join("sessions")).unwrap();
        let state = home.join("claude-parent");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("state.json"),
            include_bytes!("../../../tests/fixtures/codex-manager/attached-state.json"),
        )
        .unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        // Scrubbed real supervisor and PID-keyed registry payload shapes.
        let mut supervisor: serde_json::Value =
            serde_json::from_str(r#"{"sessionId":"claude-parent","pid":70792}"#).unwrap();
        supervisor["pid"] = child.id().into();
        let registry = config.join(format!("sessions/{}.json", std::process::id()));
        let mut entry: serde_json::Value =
            serde_json::from_str(r#"{"pid":70748,"sessionId":"claude-current"}"#).unwrap();
        entry["pid"] = std::process::id().into();
        fs::write(&registry, entry.to_string()).unwrap();
        let mut state_payload: serde_json::Value =
            serde_json::from_slice(&fs::read(state.join("state.json")).unwrap()).unwrap();
        let mut thread = state_payload["threads"][0].clone();
        thread["id"] = "thread-13".into();
        state_payload["threads"]
            .as_array_mut()
            .unwrap()
            .push(thread);
        fs::write(state.join("state.json"), state_payload.to_string()).unwrap();
        for id in ["thread-12", "thread-13"] {
            fs::write(
                home.join(format!("threads/{id}.json")),
                supervisor.to_string(),
            )
            .unwrap();
        }
        let current = links(&home, &config).unwrap();
        entry["sessionId"] = "claude-after-another-clear".into();
        fs::write(&registry, entry.to_string()).unwrap();
        let after_clear = links(&home, &config).unwrap();
        fs::remove_file(&registry).unwrap();
        let missing_registry = links(&home, &config).unwrap();
        fs::write(&registry, entry.to_string()).unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let exited = links(&home, &config).unwrap();
        for id in ["thread-12", "thread-13"] {
            assert_eq!(current[id].parent.id, "claude-current");
            assert_eq!(after_clear[id].parent.id, "claude-after-another-clear");
            assert_eq!(missing_registry[id].parent.id, "claude-parent");
            assert_eq!(exited[id].parent.id, "claude-parent");
            assert_eq!(current[id].name.as_deref(), Some("Managed worker"));
        }
    }
}
