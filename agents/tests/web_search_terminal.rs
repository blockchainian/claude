use std::path::Path;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

#[path = "support/tmux.rs"]
mod tmux;
use tmux::Tmux;

impl Tmux {
    fn capture_when(&self, ready: impl Fn(&str) -> bool, evidence: &Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let capture = self.run(&["capture-pane", "-t", "probe", "-p"]);
            std::fs::write(evidence, &capture).unwrap();
            if ready(&capture) {
                return capture;
            }
            assert!(
                Instant::now() < deadline,
                "terminal did not become ready: {capture}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn click(&self, column: usize, row: usize) {
        self.run(&[
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
    }
}

#[test]
fn web_search_details_and_browser_link() {
    use std::os::unix::fs::PermissionsExt;

    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("scratchpad/web-search-probe");
    std::fs::create_dir_all(&evidence).unwrap();
    let evidence = evidence.canonicalize().unwrap();
    let capture_path = evidence.join("terminal.txt");
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    std::fs::create_dir_all(&project).unwrap();
    let contents = include_str!("fixtures/claude-web-search.jsonl");
    std::fs::write(
        project.join("4165651d-b9e3-458d-8612-f8114e312363.jsonl"),
        contents,
    )
    .unwrap();
    let codex = directory.path().join("codex");
    std::fs::create_dir(&codex).unwrap();
    let bin = directory.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let opened = evidence.join("opened-url.txt");
    let open_status = evidence.join("open-status.txt");
    let _ = std::fs::remove_file(&opened);
    let _ = std::fs::remove_file(&open_status);
    // Observe the real open invocation, then let macOS launch the browser.
    let open = bin.join("open");
    std::fs::write(&open, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$AGENTS_PROBE_OPEN_LOG\"\n/usr/bin/open \"$@\"\nstatus=$?\nprintf '%s\\n' \"$status\" > \"$AGENTS_PROBE_OPEN_LOG.status\"\nexit \"$status\"\n").unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-web-search-{}", std::process::id()));
    tmux.run(&[
        "new-session",
        "-d",
        "-s",
        "probe",
        "-x",
        "200",
        "-y",
        "70",
        "-e",
        &format!("CLAUDE_CONFIG_DIR={}", config.display()),
        "-e",
        &format!("CLAUDE_CODE_TMPDIR={}", directory.path().display()),
        "-e",
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        "-e",
        &format!("PATH={}:{}", bin.display(), std::env::var("PATH").unwrap()),
        "-e",
        &format!("AGENTS_PROBE_OPEN_LOG={}", opened.display()),
        "/usr/bin/env",
        &format!("PATH={}:{}", bin.display(), std::env::var("PATH").unwrap()),
        binary.to_str().unwrap(),
    ]);
    tmux.capture_when(
        |capture| {
            capture
                .lines()
                .next()
                .is_some_and(|line| line.contains("Inactive 1"))
        },
        &capture_path,
    );
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let capture = tmux.capture_when(|capture| capture.contains("WebSearch"), &capture_path);
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("WebSearch"))
        .unwrap();
    let column = line[..line.find("WebSearch").unwrap()].width();
    tmux.click(column, row);
    let capture = tmux.capture_when(
        |capture| {
            capture.contains("code.claude.com") && capture.contains("The default location is")
        },
        &capture_path,
    );
    let records: Vec<serde_json::Value> = include_str!("fixtures/claude-web-search.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let query = records[0]["message"]["content"][0]["input"]["query"]
        .as_str()
        .unwrap();
    assert!(capture.contains(query), "{capture}");
    assert!(!capture.contains("\"mode\""), "{capture}");
    assert!(
        !capture.contains("Web search results for query:"),
        "{capture}"
    );
    assert!(!capture.contains("Links: ["), "{capture}");
    assert!(capture.contains("Title"), "{capture}");
    let (row, line) = capture
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("code.claude.com"))
        .unwrap();
    let column = line[..line.find("code.claude.com").unwrap()].width();
    let table_rows: Vec<_> = capture.lines().collect();
    assert!(table_rows[row - 1].contains("────────"), "{capture}");
    for (offset, result) in records[1]["toolUseResult"]["results"][0]["content"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let title = result["title"].as_str().unwrap();
        let rendered = table_rows[row + offset];
        assert!(rendered.contains(title), "{capture}");
        assert_eq!(rendered[..rendered.find(title).unwrap()].width(), column);
    }
    tmux.click(column, row);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !opened.with_extension("txt.status").exists() {
        assert!(Instant::now() < deadline, "click did not invoke open");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::fs::rename(opened.with_extension("txt.status"), &open_status).unwrap();
    assert_eq!(std::fs::read_to_string(&open_status).unwrap().trim(), "0");
    let url = records[1]["toolUseResult"]["results"][0]["content"][0]["url"]
        .as_str()
        .unwrap();
    assert_eq!(std::fs::read_to_string(&opened).unwrap().trim(), url);
    let verdict = serde_json::json!({"verdict":"pass", "input":"query only", "output":"single column linked titles and Markdown summary", "click":"real macOS open", "open_status":0, "url":url, "capture":capture_path, "open_log":opened, "binary":binary});
    std::fs::write(
        evidence.join("verdict.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
