use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

struct Probe {
    socket: String,
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
        let output = Command::new("tmux")
            .env_remove("NO_COLOR")
            .args(["-L", &self.socket])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn capture(&self) -> String {
        self.run(&["capture-pane", "-t", "probe", "-p", "-e"])
    }

    fn wait(
        &self,
        name: &str,
        width: u16,
        height: u16,
        ready: impl Fn(&str, &[String]) -> bool,
    ) -> Vec<String> {
        let deadline = (Instant::now() + Duration::from_secs(15)).min(self.deadline);
        loop {
            let capture = self.capture();
            std::fs::write(self.output.join(format!("{name}.ansi")), &capture).unwrap();
            let mut parser = vt100::Parser::new(height, width, 0);
            for (y, row) in capture.lines().enumerate() {
                parser.process(format!("\x1b[{};1H{row}", y + 1).as_bytes());
            }
            let edge: Vec<_> = (0..height)
                .map(|y| {
                    parser
                        .screen()
                        .cell(y, width - 1)
                        .unwrap()
                        .contents()
                        .to_owned()
                })
                .collect();
            if ready(&capture, &edge) {
                return edge;
            }
            assert!(Instant::now() < deadline, "{name} timed out: {capture}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn key(&self, key: &str) {
        self.run(&["send-keys", "-t", "probe", key]);
    }

    fn resize(&self, width: u16, height: u16) {
        self.run(&[
            "resize-window",
            "-t",
            "probe",
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
        ]);
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
        std::fs::write(project.join(format!("{id}.jsonl")), contents).unwrap();
        let codex = directory.path().join("codex");
        std::fs::create_dir(&codex).unwrap();
        let binary = std::env::var_os("AGENTS_PROBE_BIN")
            .expect("AGENTS_PROBE_BIN must name the source build");
        let binary = Path::new(&binary).canonicalize().unwrap();
        let probe = Self {
            socket: format!("agents-turn-groups-{name}-{}", std::process::id()),
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
        probe.wait("discovery", 200, 50, |capture, _| {
            capture.contains("Inactive 1")
        });
        probe.key("Tab");
        probe
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
    }
}

#[test]
#[ignore = "requires tmux and AGENTS_PROBE_BIN pointing to an agent-owned build"]
fn terminal_turn_groups_follow_layout_sort_and_scroll() {
    let output = std::env::var_os("AGENTS_TURN_GROUPS_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("agents-turn-groups-terminal"));
    std::fs::create_dir_all(&output).unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let outcome = std::panic::catch_unwind(|| {
        let probe = Probe::start(
            include_str!("fixtures/turn-groups/claude-steer.jsonl"),
            "complete",
            &output,
            deadline,
        );
        let mut cases = Vec::new();
        for width in [200, 112] {
            probe.resize(width, 50);
            let name = format!("complete-{width}");
            let edge = probe.wait(&name, width, 50, |_, edge| {
                ["┐", "│", "┘"]
                    .iter()
                    .all(|glyph| edge.iter().any(|cell| cell == glyph))
            });
            let top = edge.iter().position(|cell| cell == "┐").unwrap();
            let bottom = edge.iter().position(|cell| cell == "┘").unwrap();
            assert!(bottom > top + 1);
            assert!(edge[top + 1..bottom].iter().all(|cell| cell == "│"));
            // Click inside the event panel, outside the extra bracket column.
            probe.run(&[
                "send-keys",
                "-t",
                "probe",
                "-l",
                &format!(
                    "\x1b[<0;{};{}M\x1b[<0;{};{}m",
                    width - 10,
                    top + 1,
                    width - 10,
                    top + 1
                ),
            ]);
            probe.key("t");
            probe.wait(&format!("duration-{width}"), width, 50, |capture, edge| {
                capture.contains("duration")
                    && edge
                        .iter()
                        .all(|cell| !["┐", "│", "┘"].contains(&cell.as_str()))
            });
            probe.key("t");
            cases.push(serde_json::json!({"width":width,"complete_bracket":true,"duration_hides_bracket":true}));
        }
        drop(probe);
        let scroll_fixture = include_str!("fixtures/turn-groups/claude-scroll.jsonl");
        let (running, _) = scroll_fixture.trim_end().rsplit_once('\n').unwrap();
        let scroll = Probe::start(running, "scroll", &output, deadline);
        scroll.resize(112, 19);
        let edge = scroll.wait("scroll-bottom", 112, 19, |_, edge| {
            edge.iter().any(|cell| cell == "│") && !edge.iter().any(|cell| cell == "┐")
        });
        assert!(
            !edge.iter().any(|cell| cell == "┘"),
            "running turn must stay open"
        );
        let row = edge.iter().position(|cell| cell == "│").unwrap();
        scroll.run(&[
            "send-keys",
            "-t",
            "probe",
            "-l",
            &format!("\x1b[<0;100;{}M\x1b[<0;100;{}m", row + 1, row + 1),
        ]);
        scroll.key("Home");
        scroll.wait("scroll-top", 112, 19, |_, edge| {
            edge.iter().any(|cell| cell == "┐") && !edge.iter().any(|cell| cell == "┘")
        });
        serde_json::json!({"verdict":"pass","cases":cases,"clipped_head_has_vertical":true,"running_has_no_bottom_corner":true,"scroll_to_head_restores_corner":true})
    });
    let verdict = match &outcome {
        Ok(result) => result.clone(),
        Err(_) => serde_json::json!({"verdict":"fail","captures":output,"deadline_seconds":90}),
    };
    std::fs::write(
        output.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
