//! Opt-in rename probe against a disposable interactive Claude session.
use super::*;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::process::Output;
use std::time::{Duration, Instant};

struct Live {
    directory: tempfile::TempDir,
    server: String,
    config: PathBuf,
    before: BTreeSet<PathBuf>,
    claude_environment: Vec<OsString>,
    stopped: bool,
}

fn files(directory: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            paths.extend(files(&entry.path())?);
        } else {
            paths.insert(entry.path());
        }
    }
    Ok(paths)
}

impl Live {
    fn new() -> io::Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("agents-rename-")
            .tempdir_in("/private/tmp")?;
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
        let server = directory
            .path()
            .file_name()
            .expect("temporary directory name")
            .to_string_lossy()
            .into_owned();
        // The launched session has no CLAUDE_CONFIG_DIR override either.
        let config =
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME unset"))?)
                .join(".claude");
        let before = files(&config)?;
        let claude_environment = std::env::vars_os()
            .map(|(name, _)| name)
            .filter(|name| name.to_string_lossy().starts_with("CLAUDE_"))
            .collect();
        Ok(Self {
            directory,
            server,
            config,
            before,
            claude_environment,
            stopped: false,
        })
    }

    fn tmux(&self, args: &[&str]) -> io::Result<Output> {
        let mut command = Command::new("tmux");
        command.args(["-L", &self.server, "-f", "/dev/null"]);
        command.args(args).current_dir(self.directory.path());
        for name in &self.claude_environment {
            command.env_remove(name);
        }
        let output = command.output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "tmux {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(output)
    }

    fn start(&self) -> io::Result<()> {
        let arguments = serde_json::json!([
            "claude",
            "--model",
            "haiku",
            "--messaging-socket-path",
            self.directory.path().join("s.sock"),
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--",
            "Reply exactly READY. Do not use tools."
        ]);
        let launcher = self.directory.path().join("launch.py");
        // Clear again inside the pane, including variables introduced by shell startup.
        fs::write(
            &launcher,
            format!(
                "import json, os\nfor key in list(os.environ):\n    if key.startswith('CLAUDE_'):\n        del os.environ[key]\nargs = json.loads({:?})\nos.execvp(args[0], args)\n",
                arguments.to_string()
            ),
        )?;
        self.tmux(&[
            "new-session",
            "-d",
            "-s",
            "probe",
            "-x",
            "160",
            "-y",
            "48",
            "sleep",
            "120",
        ])?;
        self.tmux(&["set-option", "-w", "-t", "probe:0", "remain-on-exit", "on"])?;
        self.tmux(&[
            "respawn-pane",
            "-k",
            "-t",
            "probe:0.0",
            "python3",
            launcher.to_str().expect("temporary launcher path"),
        ])?;
        Ok(())
    }

    fn screen(&self) -> io::Result<String> {
        let output = self.tmux(&["capture-pane", "-p", "-S", "-", "-t", "probe:0.0"])?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn assert_running(&self, screen: &str) -> io::Result<()> {
        let output = self.tmux(&[
            "display-message",
            "-p",
            "-t",
            "probe:0.0",
            "#{pane_dead}:#{pane_dead_status}",
        ])?;
        let status = String::from_utf8_lossy(&output.stdout);
        if status.trim().starts_with("1:") {
            return Err(io::Error::other(format!(
                "Claude pane exited ({status}): {screen}"
            )));
        }
        Ok(())
    }

    fn key(&self, text: &str) -> io::Result<()> {
        self.tmux(&["send-keys", "-t", "probe:0.0", "-l", text])?;
        self.tmux(&["send-keys", "-t", "probe:0.0", "Enter"])?;
        Ok(())
    }

    fn registry(&self) -> io::Result<Option<(PathBuf, serde_json::Value)>> {
        for entry in fs::read_dir(self.config.join("sessions"))? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            if value["messagingSocketPath"]
                == self
                    .directory
                    .path()
                    .join("s.sock")
                    .to_string_lossy()
                    .as_ref()
            {
                return Ok(Some((path, value)));
            }
        }
        Ok(None)
    }

    fn stop(&mut self) -> io::Result<()> {
        if !self.stopped {
            self.tmux(&["kill-server"])?;
            self.stopped = true;
        }
        Ok(())
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if !self.stopped {
            match self.screen() {
                Ok(screen) => eprintln!("Claude pane before cleanup:\n{screen}"),
                Err(error) => eprintln!("Claude pane capture: {error}"),
            }
        }
        if let Err(error) = self.stop() {
            eprintln!("Claude rename cleanup: {error}");
        }
        match files(&self.config) {
            Ok(after) => println!(
                "{}",
                serde_json::json!({
                    "claude_files_created_during_probe": after.difference(&self.before).collect::<Vec<_>>()
                })
            ),
            Err(error) => eprintln!("Claude files inventory: {error}"),
        }
        // TempDir removes only the probe directory; Claude's files are left alone.
    }
}

#[test]
#[ignore = "starts one real Haiku session in a private tmux server"]
fn rename_real_socket_session() -> io::Result<()> {
    let mut live = Live::new()?;
    live.start()?;
    let mut provider = ClaudeProvider::new(live.config.clone());
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut trusted = false;
    let (registry_path, session) = loop {
        if let Some((path, value)) = live.registry()? {
            let id = value["sessionId"]
                .as_str()
                .ok_or_else(|| io::Error::other("Claude registry lacks sessionId"))?;
            if let Some(session) = provider
                .refresh(1)
                .sessions
                .into_iter()
                .find(|session| session.id.id == id && session.process.is_some())
            {
                break (path, session);
            }
        }
        let screen = live.screen()?;
        live.assert_running(&screen)?;
        if !trusted && screen.contains("Yes, I trust this folder") {
            live.key("1")?;
            trusted = true;
        }
        assert!(
            Instant::now() < deadline,
            "Claude session was not registered: {screen}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let name = format!("agents socket {} 中文 \"quote\" $HOME ; /exit", live.server);
    provider
        .send(&session, SessionInput::Rename(name.clone()))
        .expect("Claude rename job")()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let screen = live.screen()?;
        live.assert_running(&screen)?;
        let registry: serde_json::Value = serde_json::from_slice(&fs::read(&registry_path)?)?;
        if registry["name"] == name {
            assert_eq!(registry["nameSource"], "peer");
            println!(
                "{}",
                serde_json::json!({
                    "verdict": "pass", "session": session.id.id,
                    "pid": session.process.as_ref().expect("live process").pid,
                    "name": name, "registry": registry_path,
                    "transcript": session.transcript, "name_source": registry["nameSource"],
                    "socket": live.directory.path().join("s.sock"),
                })
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Claude did not publish socket rename: {registry}\n{screen}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    live.stop()?;
    Ok(())
}
