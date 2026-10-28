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
    let deadline = Instant::now() + Duration::from_secs(20);
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
fn start(name: &str, records: &str, custom: bool) -> (tempfile::TempDir, Tmux) {
    let dir = tempfile::tempdir().unwrap();
    let claude = dir.path().join("claude");
    let codex = dir.path().join("codex");
    let project = claude.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir(&codex).unwrap();
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        records,
    )
    .unwrap();
    if custom {
        let path = dir.path().join("probe.tmTheme");
        std::fs::write(&path, r##"<?xml version="1.0"?><plist version="1.0"><dict><key>settings</key><array><dict><key>settings</key><dict><key>foreground</key><string>#102030</string><key>selection</key><string>#123456</string><key>accent</key><string>#654321</string></dict></dict><dict><key>scope</key><string>markup.quote</string><key>settings</key><dict><key>foreground</key><string>#234567</string></dict></dict><dict><key>scope</key><string>markup.raw.block</string><key>settings</key><dict><key>foreground</key><string>#345678</string></dict></dict><dict><key>scope</key><string>markup.deleted</string><key>settings</key><dict><key>foreground</key><string>#456789</string></dict></dict><dict><key>scope</key><string>markup.inserted</string><key>settings</key><dict><key>foreground</key><string>#56789a</string></dict></dict><dict><key>scope</key><string>message.error</string><key>settings</key><dict><key>foreground</key><string>#6789ab</string></dict></dict></array></dict></plist>"##).unwrap();
        std::fs::write(
            claude.join("agents.json"),
            serde_json::json!({"config":{"light_theme":path,"dark_theme":path},"state":{}})
                .to_string(),
        )
        .unwrap();
    }
    let tmux = launch(name, dir.path());
    (dir, tmux)
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
fn upstream_markdown() -> String {
    let mut records: Vec<serde_json::Value> = include_str!("fixtures/transcript.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    records[1]["message"]["content"][0]["text"] = "> QuoteProbe\n\n```\nRawProbe\n```\n".into();
    records.into_iter().map(|r| format!("{r}\n")).collect()
}
fn upstream_patch() -> String {
    let source: Vec<serde_json::Value> = include_str!("fixtures/claude-tool-calls.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let mut records: Vec<serde_json::Value> = include_str!("fixtures/transcript.jsonl")
        .lines()
        .take(1)
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    records.extend_from_slice(&source[4..6]);
    records[2]["toolUseResult"]["structuredPatch"][0]["lines"] =
        serde_json::json!(["-DeletedProbe", "+InsertedProbe"]);
    records.into_iter().map(|r| format!("{r}\n")).collect()
}
fn evidence(name: &str, raw: &str) {
    let dir = std::env::temp_dir().join("agents-theme-probe");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.ansi")), raw).unwrap();
}

#[test]
fn terminal_theme_colors_follow_bundled_and_custom_themes() {
    let mut checks = Vec::new();
    for custom in [false, true] {
        for patch in [false, true] {
            let name = format!(
                "{}-{}",
                if custom { "custom" } else { "bundled" },
                if patch { "patch" } else { "markdown" }
            );
            let (_dir, tmux) = start(
                &name,
                &if patch {
                    upstream_patch()
                } else {
                    upstream_markdown()
                },
                custom,
            );
            for light in [true, false, true] {
                tmux.run(&[
                    "send-keys",
                    "-t",
                    "probe",
                    "-l",
                    if light {
                        "\x1b[?997;2n"
                    } else {
                        "\x1b[?997;1n"
                    },
                ]);
                let green = if light {
                    vt100::Color::Rgb(17, 99, 41)
                } else {
                    vt100::Color::Rgb(74, 139, 79)
                };
                let red = if light {
                    vt100::Color::Rgb(130, 7, 30)
                } else {
                    vt100::Color::Rgb(160, 99, 93)
                };
                let expected: Vec<(&str, vt100::Color)> = if patch {
                    vec![
                        (
                            "-DeletedProbe",
                            if custom {
                                vt100::Color::Rgb(69, 103, 137)
                            } else {
                                red
                            },
                        ),
                        (
                            "+InsertedProbe",
                            if custom {
                                vt100::Color::Rgb(86, 120, 154)
                            } else {
                                green
                            },
                        ),
                    ]
                } else {
                    vec![
                        (
                            "QuoteProbe",
                            if custom {
                                vt100::Color::Rgb(35, 69, 103)
                            } else {
                                green
                            },
                        ),
                        (
                            "RawProbe",
                            if custom {
                                vt100::Color::Rgb(52, 86, 120)
                            } else {
                                vt100::Color::Idx(6)
                            },
                        ),
                    ]
                };
                let raw = wait(&tmux, |s| {
                    expected
                        .iter()
                        .all(|(m, c)| cell(s, m).is_some_and(|cell| cell.fgcolor() == *c))
                });
                evidence(
                    &format!("{name}-{}", if light { "light" } else { "dark" }),
                    &raw,
                );
                let parsed = screen(&raw);
                let s = parsed.screen();
                let selected = cell(s, "Inactive 1").expect("selected inactive tab");
                assert_eq!(
                    selected.bgcolor(),
                    if custom {
                        vt100::Color::Rgb(18, 52, 86)
                    } else {
                        vt100::Color::Idx(if light { 111 } else { 233 })
                    },
                    "{raw}"
                );
                checks.push(
                    serde_json::json!({"theme":name,"light":light,"content":true,"selection":true}),
                );
            }
        }
    }
    let verdict = serde_json::json!({"verdict":"pass","checks":checks,"limitation":"Error labels and focused prompt require separate real-app probes"});
    let path = std::env::temp_dir().join("agents-theme-probe/verdict.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    println!("{}", path.display());
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
