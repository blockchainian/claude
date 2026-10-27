use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn capture_when(
    tmux: &Tmux,
    deadline: Instant,
    evidence: &Path,
    ready: impl Fn(&str) -> bool,
) -> String {
    loop {
        let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
        std::fs::write(evidence, &capture).unwrap();
        if ready(&capture) {
            return capture;
        }
        assert!(Instant::now() < deadline, "terminal timed out: {capture}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires tmux; real worktree confirmation dialogs at 80 and 120 columns"]
fn worktree_confirmation_options_show_directory_and_ellipsis() {
    let deadline = Instant::now() + Duration::from_secs(150);
    let evidence = Path::new("/tmp")
        .canonicalize()
        .unwrap()
        .join(format!("agents-worktree-dialog-{}", std::process::id()));
    std::fs::create_dir_all(&evidence).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let result = std::panic::catch_unwind(|| {
        let directory = tempfile::tempdir().unwrap();
        let primary = directory.path().join("repository");
        std::fs::create_dir(&primary).unwrap();
        git(&primary, &["init", "-b", "main"]);
        git(
            &primary,
            &[
                "-c",
                "user.name=Probe",
                "-c",
                "user.email=probe@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "Probe",
            ],
        );
        let worktree = directory.path().join("fix-claude-session-input");
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-b",
                "linked",
                worktree.to_str().unwrap(),
            ],
        );
        let claude = directory.path().join("claude");
        let codex = directory.path().join("codex");
        std::fs::create_dir(&claude).unwrap();
        std::fs::create_dir(&codex).unwrap();
        let transcript = codex.join("rollout.jsonl");
        std::fs::write(&transcript, include_str!("fixtures/codex-rollout.jsonl")).unwrap();
        let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
        db.execute_batch(include_str!("fixtures/codex-state.sql"))
            .unwrap();
        db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
            .unwrap();
        db.execute(
            "UPDATE threads SET id='0d38effb-f83f-4ebd-8539-a6d4c0938503', rollout_path=?1, cwd=?2, archived=1, title='Worktree dialog probe', name='Worktree dialog probe'",
            rusqlite::params![transcript.to_str().unwrap(), worktree.to_str().unwrap()],
        )
        .unwrap();
        drop(db);

        let mut checks = Vec::new();
        for width in [80, 120] {
            let tmux = Tmux::new(format!(
                "agents-worktree-dialog-{width}-{}",
                std::process::id()
            ));
            tmux.run(&[
                "new-session",
                "-d",
                "-s",
                "probe",
                "-x",
                &width.to_string(),
                "-y",
                "40",
                "-e",
                &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
                "-e",
                &format!("CODEX_HOME={}", codex.display()),
                "-e",
                &format!("CLAUDE_CODE_TMPDIR={}", directory.path().display()),
                "-e",
                &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
                binary.to_str().unwrap(),
            ]);
            let progress = evidence.join(format!("progress-{width}.txt"));
            capture_when(&tmux, deadline, &progress, |text| {
                text.contains("Inactive 1")
            });
            tmux.run(&["send-keys", "-t", "probe", "Tab"]);
            capture_when(&tmux, deadline, &progress, |text| {
                text.contains("Worktree dialog probe")
            });
            for (action, key, moves, expected) in [
                (
                    "delete",
                    "BSpace",
                    2,
                    if width == 80 {
                        "❯ 3. Permanently delete task, child agents and worktree (fix-claude-sessi…"
                    } else {
                        "❯ 3. Permanently delete task, child agents and worktree (fix-claude-session…"
                    },
                ),
                (
                    "archive",
                    "a",
                    1,
                    "❯ 2. Archive and delete worktree (fix-claude-session-input)",
                ),
            ] {
                tmux.run(&["send-keys", "-t", "probe", key]);
                capture_when(&tmux, deadline, &progress, |text| {
                    text.contains("and delete worktree")
                        || text.contains("child agents and worktree")
                });
                for _ in 0..moves {
                    tmux.run(&["send-keys", "-t", "probe", "Down"]);
                }
                let path = evidence.join(format!("{action}-{width}.txt"));
                let capture = capture_when(&tmux, deadline, &path, |text| {
                    text.contains(&format!("❯ {}.", moves + 1))
                });
                let option = capture
                    .lines()
                    .find(|line| line.contains(&format!("❯ {}.", moves + 1)))
                    .unwrap();
                checks.push(serde_json::json!({
                    "action": action,
                    "terminal_width": width,
                    "capture": path,
                    "option": option,
                    "expected": expected,
                    "passed": option.contains(expected),
                }));
                tmux.run(&["send-keys", "-t", "probe", "Escape"]);
                capture_when(&tmux, deadline, &progress, |text| {
                    !text.contains("child agents and worktree")
                        && !text.contains("Archive and delete worktree")
                });
                assert!(
                    worktree.exists(),
                    "opening a dialog must preserve the worktree"
                );
            }
            git(&primary, &["checkout", "-B", "feature"]);
            git(&worktree, &["checkout", "main"]);
            tmux.run(&["send-keys", "-t", "probe", "BSpace"]);
            capture_when(&tmux, deadline, &progress, |text| {
                text.contains("1. Cancel")
            });
            tmux.run(&["send-keys", "-t", "probe", "3"]);
            tmux.run(&["send-keys", "-t", "probe", "End"]);
            let path = evidence.join(format!("delete-trunk-{width}.txt"));
            let capture = capture_when(&tmux, deadline, &path, |text| {
                text.contains("❯ 2. Permanently delete task and child agents")
            });
            checks.push(serde_json::json!({
                "action": "delete-trunk",
                "terminal_width": width,
                "capture": path,
                "passed": !capture.contains("3. Permanently delete") && worktree.exists(),
            }));
            tmux.run(&["send-keys", "-t", "probe", "Escape"]);
            capture_when(&tmux, deadline, &progress, |text| {
                !text.contains("1. Cancel")
            });
            git(&worktree, &["checkout", "linked"]);
        }
        assert!(
            checks.iter().all(|check| check["passed"] == true),
            "option text differs: {}",
            serde_json::to_string_pretty(&checks).unwrap()
        );
        checks
    });
    let verdict = match &result {
        Ok(checks) => serde_json::json!({"passed": true, "binary": binary, "checks": checks}),
        Err(_) => serde_json::json!({"passed": false, "binary": binary}),
    };
    let verdict_path = evidence.join("verdict.json");
    std::fs::write(&verdict_path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{}", verdict_path.display());
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
