//! Real application color probe; isolated homes and upstream transcript envelopes.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;
#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;
const WIDTH: u16 = 200;
const HEIGHT: u16 = 80;

fn capture(tmux: &Tmux) -> String {
    tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"])
}
fn screen(raw: &str) -> vt100::Parser {
    let mut parser = vt100::Parser::new(HEIGHT, WIDTH, 0);
    for (row, line) in raw.lines().enumerate() {
        parser.process(format!("\x1b[{};1H{line}", row + 1).as_bytes());
    }
    parser
}
fn wait(tmux: &Tmux, predicate: impl Fn(&vt100::Screen) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let raw = capture(tmux);
        if predicate(screen(&raw).screen()) {
            return raw;
        }
        assert!(
            Instant::now() < deadline,
            "theme probe timed out; quote={:?}; raw={:?}; deleted={:?}; inserted={:?}: {raw}",
            cell(screen(&raw).screen(), "QuoteProbe").map(|c| c.fgcolor()),
            cell(screen(&raw).screen(), "RawProbe").map(|c| c.fgcolor()),
            cell(screen(&raw).screen(), "-DeletedProbe").map(|c| c.fgcolor()),
            cell(screen(&raw).screen(), "+InsertedProbe").map(|c| c.fgcolor())
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn cell<'a>(screen: &'a vt100::Screen, marker: &str) -> Option<&'a vt100::Cell> {
    screen.rows(0, WIDTH).enumerate().find_map(|(row, line)| {
        let byte = line.find(marker)?;
        screen.cell(row as u16, line[..byte].width() as u16)
    })
}
fn binary() -> PathBuf {
    PathBuf::from(
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into()),
    )
    .canonicalize()
    .unwrap()
}
fn launch(name: &str, dir: &std::path::Path) -> Tmux {
    let claude = dir.join("claude");
    let codex = dir.join("codex");
    let tmux = Tmux::new(format!("agents-theme-{name}-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "200",
        "-y",
        "80",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", claude.display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", dir.display()),
        binary().to_str().unwrap(),
    ]);
    wait(&tmux, |s| s.contents().contains("Inactive 1"));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    tmux
}
fn evidence(name: &str, raw: &str) {
    let dir = std::env::temp_dir().join("agents-theme-probe");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.ansi")), raw).unwrap();
}

#[test]
fn terminal_prompt_accent_and_send_error_use_theme_colors() {
    let dir = tempfile::tempdir().unwrap();
    let codex = dir.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    std::fs::create_dir(dir.path().join("claude")).unwrap();
    let transcript = codex.join("rollout.jsonl");
    std::fs::write(
        &transcript,
        include_str!("fixtures/codex-command-failed.jsonl"),
    )
    .unwrap();
    let db = rusqlite::Connection::open(codex.join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id != 'thread-0'", [])
        .unwrap();
    db.execute(
        "UPDATE threads SET rollout_path=?1, cwd=?2, archived=0, name='Composer'",
        rusqlite::params![transcript.to_str().unwrap(), dir.path().to_str().unwrap()],
    )
    .unwrap();
    let tmux = launch("composer", dir.path());
    let raw = wait(&tmux, |s| s.contents().contains("Composer ❯"));
    let parsed = screen(&raw);
    let (row, text) = parsed
        .screen()
        .rows(0, WIDTH)
        .enumerate()
        .find(|(_, line)| line.contains("Composer ❯"))
        .unwrap();
    let column = text[..text.find('❯').unwrap()].width();
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        &format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column + 1,
            row + 1,
            column + 1,
            row + 1
        ),
    ]);
    tmux.run(&["send-keys", "-t", "probe", "-l", "retry me"]);
    let raw = wait(&tmux, |s| {
        cell(s, "❯ retry me").is_some_and(|c| c.fgcolor() == vt100::Color::Idx(6))
    });
    evidence("composer-accent", &raw);
    tmux.run(&["send-keys", "-t", "probe", "Enter"]);
    // The isolated home has no daemon; the real send reports its error.
    let raw = wait(&tmux, |s| {
        cell(s, "❮ retry me").is_some_and(|c| c.fgcolor() == vt100::Color::Rgb(160, 99, 93))
    });
    evidence("composer-error", &raw);
    let parsed = screen(&raw);
    let (row, text) = parsed
        .screen()
        .rows(0, WIDTH)
        .enumerate()
        .find(|(_, line)| line.contains("Exec command"))
        .unwrap();
    let column = text[..text.find("Exec command").unwrap()].width();
    tmux.run(&[
        "send-keys",
        "-t",
        "probe",
        "-l",
        &format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column + 1,
            row + 1,
            column + 1,
            row + 1
        ),
    ]);
    tmux.run(&["send-keys", "-t", "probe", "/"]);
    let raw = wait(&tmux, |s| {
        cell(s, "Search ❯").is_some_and(|c| c.fgcolor() == vt100::Color::Idx(6))
    });
    evidence("search-accent", &raw);
    let verdict = serde_json::json!({"verdict":"pass","prompt_accent":"ANSI 6","search_accent":"ANSI 6","error":"#a0635d"});
    let path = std::env::temp_dir().join("agents-theme-probe/composer-verdict.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{}", path.display());
}
