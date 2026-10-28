use std::io::Write;
use std::process::Command;

pub struct Tmux {
    name: String,
    config: tempfile::NamedTempFile,
}

impl Tmux {
    pub fn new(name: String) -> Self {
        assert!(
            name.starts_with("agents-")
                && name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c)),
            "invalid probe socket name: {name}"
        );
        let mut config = tempfile::NamedTempFile::new().unwrap();
        writeln!(config, "set -g default-terminal tmux-256color").unwrap();
        // Run under the daemon so this survives even a kill of the test process group.
        writeln!(
            config,
            "run-shell -b \"while kill -0 {} 2>/dev/null; do sleep 0.1; done; tmux -L {} kill-server\"",
            std::process::id(),
            name
        )
        .unwrap();
        Self { name, config }
    }

    pub fn run(&self, args: &[&str]) -> String {
        // These probes assert ANSI colors and intensity in a controlled terminal.
        let output = Command::new("tmux")
            .env_remove("NO_COLOR")
            .args(["-L", &self.name, "-f"])
            .arg(self.config.path())
            .args(args)
            .output()
            .expect("tmux must be installed for the terminal probe");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.name, "kill-server"])
            .output();
    }
}
