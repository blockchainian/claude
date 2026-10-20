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

    fn rows(&self, width: u16, height: u16) -> Vec<Vec<String>> {
        let capture = self.capture();
        let mut parser = vt100::Parser::new(height, width, 0);
        for (y, row) in capture.lines().enumerate() {
            parser.process(format!("\x1b[{};1H{row}", y + 1).as_bytes());
        }
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| parser.screen().cell(y, x).unwrap().contents().to_owned())
                    .collect()
            })
            .collect()
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
            let rows = probe.rows(width, 50);
            for (row, glyph) in edge.iter().enumerate().take(bottom + 1).skip(top) {
                assert_eq!(
                    rows[row][usize::from(width) - 2],
                    " ",
                    "time/bracket gap: {width}, row {row}"
                );
                assert_ne!(
                    rows[row][usize::from(width) - 3],
                    " ",
                    "time must end immediately before the gap: {width}, row {row}"
                );
                assert_eq!(rows[row][usize::from(width) - 1], *glyph);
            }
            std::fs::write(
                output.join(format!("list-{width}.txt")),
                rows[top..=bottom]
                    .iter()
                    .map(|row| row.concat())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
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
            probe.wait(&format!("detail-{width}"), width, 50, |capture, _| {
                capture
                    .lines()
                    .skip(bottom + 1)
                    .any(|line| line.contains("19:59:00"))
            });
            let rows = probe.rows(width, 50);
            let (detail_start, detail) = rows
                .iter()
                .enumerate()
                .skip(bottom + 1)
                .find(|(_, row)| row.concat().contains("19:59:00"))
                .expect("clicked user event must display its timestamp in the detail title");
            assert_eq!(
                detail[usize::from(width) - 3],
                "0",
                "detail time ends two cells before the edge"
            );
            assert!(
                detail[usize::from(width) - 2..]
                    .iter()
                    .all(|cell| cell.trim().is_empty()),
                "detail right padding"
            );
            std::fs::write(
                output.join(format!("detail-{width}.txt")),
                rows[detail_start..]
                    .iter()
                    .take(6)
                    .map(|row| row.concat())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            probe.key("t");
            probe.wait(&format!("duration-{width}"), width, 50, |capture, edge| {
                capture.contains("duration")
                    && edge
                        .iter()
                        .all(|cell| !["┐", "│", "┘"].contains(&cell.as_str()))
            });
            probe.key("t");
            cases.push(serde_json::json!({"width":width,"complete_bracket":true,"time_bracket_gap_cells":1,"detail_right_padding_cells":2,"duration_hides_bracket":true}));
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
        let scrollbar_row = edge
            .iter()
            .position(|cell| cell == "▕")
            .expect("short detail panel must retain its scrollbar at the terminal edge");
        let rows = scroll.rows(112, 19);
        assert_eq!(rows[scrollbar_row][111], "▕");
        assert!(
            rows[scrollbar_row][110].trim().is_empty(),
            "scrollbar remains outside the extra time padding"
        );
        assert_ne!(
            rows[scrollbar_row][109], " ",
            "detail timestamp ends before the padding"
        );
        std::fs::write(output.join("scrollbar.txt"), rows[scrollbar_row].concat()).unwrap();
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
        serde_json::json!({"verdict":"pass","cases":cases,"clipped_head_has_vertical":true,"running_has_no_bottom_corner":true,"scroll_to_head_restores_corner":true,"detail_scrollbar_column":111})
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
