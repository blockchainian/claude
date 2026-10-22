#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Probe {
    tmux: Tmux,
    deadline: Instant,
    output: PathBuf,
    _directory: tempfile::TempDir,
}

impl Probe {
    fn run(&self, args: &[&str]) -> String {
        assert!(
            Instant::now() < self.deadline,
            "terminal probe deadline exceeded"
        );
        self.tmux.run(args)
    }

    fn capture(&self) -> String {
        self.run(&["capture-pane", "-t", "probe", "-p", "-e"])
    }

    fn wait(&self, name: &str, ready: impl Fn(&str) -> bool) {
        let deadline = (Instant::now() + Duration::from_secs(15)).min(self.deadline);
        loop {
            let capture = self.capture();
            std::fs::write(self.output.join(format!("{name}.ansi")), &capture).unwrap();
            if ready(&capture) {
                return;
            }
            assert!(Instant::now() < deadline, "{name} timed out: {capture}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn key(&self, key: &str) {
        self.run(&["send-keys", "-t", "probe", key]);
    }

    fn start(contents: &str, name: &str, output: &Path, deadline: Instant) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("claude");
        let project = config.join("projects/project");
        std::fs::create_dir_all(&project).unwrap();
        let id = contents
            .lines()
            .find_map(|line| {
                let record: serde_json::Value = serde_json::from_str(line).unwrap();
                record["sessionId"].as_str().map(str::to_owned)
            })
            .expect("real fixture session id");
        let codex = directory.path().join("codex");
        std::fs::create_dir(&codex).unwrap();
        let binary = std::env::var_os("AGENTS_PROBE_BIN")
            .expect("AGENTS_PROBE_BIN must name the source build");
        let binary = Path::new(&binary).canonicalize().unwrap();
        let visible = contents
            .lines()
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                record["isSidechain"] = false.into();
                serde_json::to_string(&record).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(project.join(format!("{id}.jsonl")), visible).unwrap();
        let probe = Self {
            tmux: Tmux::new(format!("agents-thinking-{name}-{}", std::process::id())),
            deadline,
            output: output.to_path_buf(),
            _directory: directory,
        };
        probe.run(&[
            "new-session",
            "-d",
            "-s",
            "probe",
            "-x",
            "200",
            "-y",
            "50",
            "-e",
            &format!("CLAUDE_CONFIG_DIR={}", config.display()),
            "-e",
            &format!("CLAUDE_CODE_TMPDIR={}", probe._directory.path().display()),
            "-e",
            &format!("CODEX_HOME={}", codex.display()),
            "-e",
            &format!(
                "CODEX_MANAGER_STATE_DIR={}",
                probe._directory.path().display()
            ),
            binary.to_str().unwrap(),
        ]);
        probe.wait("discovery", |capture| capture.contains("Inactive 1"));
        probe.key("Tab");
        probe
    }
}

#[test]
#[ignore = "requires tmux and AGENTS_PROBE_BIN pointing to an agent-owned build"]
fn terminal_thinking_list_detail_and_last_activity() {
    let output = std::env::var_os("AGENTS_THINKING_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-thinking-terminal"));
    std::fs::create_dir_all(&output).unwrap();
    let outcome = std::panic::catch_unwind(|| {
        let fixture = include_str!("fixtures/claude-thinking.jsonl");
        let thinking: serde_json::Value =
            serde_json::from_str(fixture.lines().next().unwrap()).unwrap();
        let mut prompt: serde_json::Value = include_str!("fixtures/transcript.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["type"] == "user")
            .unwrap();
        let at =
            chrono::DateTime::parse_from_rfc3339(thinking["timestamp"].as_str().unwrap()).unwrap();
        prompt["timestamp"] = (at - chrono::Duration::milliseconds(231782))
            .to_rfc3339()
            .into();
        prompt["cwd"] = thinking["cwd"].clone();
        prompt["sessionId"] = thinking["sessionId"].clone();
        let thinking = format!("{prompt}\n{thinking}\n");
        let probe = Probe::start(
            &thinking,
            "encrypted",
            &output,
            Instant::now() + Duration::from_secs(45),
        );
        probe.wait("selected", |capture| {
            capture.contains("Last activity") && capture.matches("(encrypted)").count() == 3
        });
        let capture = probe.capture();
        assert!(capture.contains("Last activity"), "{capture}");
        assert_eq!(capture.matches("(encrypted)").count(), 3, "{capture}");
        let event_row = capture
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("(encrypted)"))
            .map(|(index, _)| index)
            .next()
            .unwrap();
        probe.run(&[
            "send-keys",
            "-t",
            "probe",
            "-l",
            &format!(
                "\x1b[<0;150;{}M\x1b[<0;150;{}m",
                event_row + 1,
                event_row + 1
            ),
        ]);
        probe.wait("detail", |capture| {
            capture.contains("Thinking") && capture.matches("(encrypted)").count() == 3
        });
        probe.key("t");
        probe.wait("duration", |capture| capture.contains("duration"));
        std::fs::write(output.join("complete.ansi"), probe.capture()).unwrap();
    });
    let verdict = serde_json::json!({"passed":outcome.is_ok(),"surfaces":["event list","event detail","last activity","duration sort"]});
    std::fs::write(
        output.join("verdict.json"),
        serde_json::to_string_pretty(&verdict).unwrap(),
    )
    .unwrap();
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
