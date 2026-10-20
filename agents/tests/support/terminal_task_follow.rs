use super::*;

const WIDTH: u16 = 200;
const HEIGHT: u16 = 50;

fn registry_entry(id: &str, name: &str, cwd: &Path) -> serde_json::Value {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    assert_eq!(
        written, size,
        "libproc must read the probe process start time"
    );
    let info = unsafe { info.assume_init() };
    let start = chrono::DateTime::from_timestamp(info.pbi_start_tvsec as i64, 0).unwrap();
    let mut entry: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/registry.json")).unwrap();
    entry["pid"] = std::process::id().into();
    entry["procStart"] = start.format("%a %b %e %H:%M:%S %Y").to_string().into();
    entry["sessionId"] = id.into();
    entry["cwd"] = cwd.to_str().unwrap().into();
    entry["name"] = name.into();
    entry["status"] = "idle".into();
    entry["updatedAt"] = chrono::Utc::now().timestamp_millis().into();
    entry
}

fn write_entry(path: &Path, entry: &serde_json::Value) {
    let temporary = path.with_extension("pending");
    std::fs::write(&temporary, serde_json::to_vec(entry).unwrap()).unwrap();
    std::fs::rename(temporary, path).unwrap();
}

fn selected(capture: &str, name: &str) -> bool {
    ['❯', '❮']
        .into_iter()
        .any(|marker| capture.contains(&format!("{name} {marker}")))
}

fn select_task(tmux: &Tmux, name: &str) {
    let capture = tmux.capture_when(|capture| {
        capture.lines().any(|line| {
            line.contains(name) && (line.contains("Ready") || line.contains("Inactive"))
        })
    });
    let row = capture
        .lines()
        .position(|line| {
            line.contains(name) && (line.contains("Ready") || line.contains("Inactive"))
        })
        .unwrap_or_else(|| panic!("task {name} is not listed: {capture}"));
    tmux.mouse(0, 5, row, true);
    tmux.wait_for(|capture| selected(capture, name));
}

fn assert_follow_screen(tmux: &Tmux, active: bool, task: &str, name: &str) -> String {
    tmux.wait_for(|capture| selected(capture, task));
    let raw = tmux.run(&["capture-pane", "-t", "probe", "-p", "-e"]);
    let parsed = captured_screen(&raw, WIDTH, HEIGHT);
    let screen = parsed.screen();
    let header = screen.rows(0, WIDTH).next().unwrap();
    let inactive = header[..header.find("Inactive").unwrap()].width() as u16;
    assert_eq!(
        screen.cell(0, 2).unwrap().bgcolor() != vt100::Color::Default,
        active,
        "{raw}"
    );
    assert_eq!(
        screen.cell(0, inactive).unwrap().bgcolor() != vt100::Color::Default,
        !active,
        "{raw}"
    );
    let path = std::env::temp_dir().join(format!("agents-task-follow-{name}.txt"));
    std::fs::write(&path, raw).unwrap();
    path.display().to_string()
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_task_follow_only_returns_to_active() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("claude");
    let project = config.join("projects/project");
    let registry = config.join("sessions");
    let codex = directory.path().join("codex");
    for path in [&project, &registry, &codex] {
        std::fs::create_dir_all(path).unwrap();
    }
    let ids = [
        "4165651d-b9e3-458d-8612-f8114e312363",
        "d6e5280c-6bcd-42c4-8bb9-6ff96d1b0744",
    ];
    let names = ["Follow A", "Follow B"];
    let mut entries = Vec::new();
    for (id, name) in ids.into_iter().zip(names) {
        let records = include_str!("../fixtures/transcript.jsonl")
            .lines()
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                record["sessionId"] = id.into();
                record["cwd"] = directory.path().to_str().unwrap().into();
                if record["type"] == "ai-title" {
                    record["aiTitle"] = name.into();
                }
                record.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(project.join(format!("{id}.jsonl")), records + "\n").unwrap();
        let entry = registry_entry(id, name, directory.path());
        let path = registry.join(format!("{id}.json"));
        write_entry(&path, &entry);
        entries.push((path, entry));
    }
    let binary =
        std::env::var_os("AGENTS_PROBE_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_agents").into());
    let binary = Path::new(&binary).canonicalize().unwrap();
    let tmux = Tmux::new(format!("agents-task-follow-{}", std::process::id()));
    tmux.run(&[
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
        &format!("CODEX_HOME={}", codex.display()),
        "-e",
        &format!("CODEX_MANAGER_STATE_DIR={}", directory.path().display()),
        binary.to_str().unwrap(),
    ]);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Active 2"))
    });
    select_task(&tmux, "Follow A");
    std::fs::remove_file(&entries[0].0).unwrap();
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Inactive 1"))
    });
    let stopped = assert_follow_screen(&tmux, true, "Follow B", "stopped");

    write_entry(&entries[0].0, &entries[0].1);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Active 2"))
    });
    select_task(&tmux, "Follow A");
    tmux.run(&["send-keys", "-t", "probe", "r"]);
    tmux.wait_for(|capture| capture.contains("Rename ❯ Follow A"));
    std::fs::remove_file(&entries[0].0).unwrap();
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Inactive 1"))
    });
    let renamed = assert_follow_screen(&tmux, true, "Follow B", "rename-stopped");
    let capture = tmux.run(&["capture-pane", "-t", "probe", "-p"]);
    assert!(!capture.contains("Rename ❯"), "{capture}");

    write_entry(&entries[0].0, &entries[0].1);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Active 2"))
    });
    std::fs::remove_file(&entries[1].0).unwrap();
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Inactive 1"))
    });
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    select_task(&tmux, "Follow B");
    assert_follow_screen(&tmux, false, "Follow B", "inactive");
    write_entry(&entries[1].0, &entries[1].1);
    tmux.wait_for(|capture| {
        capture
            .lines()
            .next()
            .is_some_and(|line| line.contains("Active 2"))
    });
    let resumed = assert_follow_screen(&tmux, true, "Follow B", "resumed");
    println!(
        "{}",
        serde_json::json!({
            "verdict": "pass",
            "active_to_inactive": "pass",
            "rename_to_inactive": "pass",
            "inactive_to_active": "pass",
            "captures": [stopped, renamed, resumed],
        })
    );
}

#[test]
#[ignore = "requires tmux; run with --ignored --nocapture"]
fn terminal_status_tabs_filter_children_and_preserve_ancestor_status() {
    let listed = |capture: &str, title: &str| {
        capture
            .lines()
            .skip(2)
            .take(26)
            .any(|line| line.split('│').next().unwrap().contains(title))
    };
    let (directory, tmux, transcript) = start_probe("status-tabs");
    let children = transcript.with_extension("").join("subagents");
    std::fs::create_dir_all(&children).unwrap();
    let records = include_str!("../fixtures/subagent.jsonl");
    for (id, title, contents) in [
        ("done", "Finished child", records),
        ("working", "Working child", records.lines().next().unwrap()),
    ] {
        std::fs::write(children.join(format!("agent-{id}.jsonl")), contents).unwrap();
        std::fs::write(
            children.join(format!("agent-{id}.meta.json")),
            serde_json::json!({"description": title}).to_string(),
        )
        .unwrap();
    }
    let registry = directory.path().join("claude/sessions");
    std::fs::create_dir_all(&registry).unwrap();
    let id = transcript.file_stem().unwrap().to_str().unwrap();
    let mut entry = registry_entry(id, "Live parent", directory.path());
    entry["status"] = "busy".into();
    entry["updatedAt"] =
        (chrono::Utc::now() - chrono::Duration::hours(2) - chrono::Duration::minutes(1))
            .timestamp_millis()
            .into();
    let path = registry.join(format!("{id}.json"));
    write_entry(&path, &entry);
    let inactive = tmux.capture_when(|capture| {
        capture.contains("Live parent")
            && listed(capture, "Finished child")
            && !listed(capture, "Working child")
            && capture.contains("Inactive 2")
            && capture.contains("Active 2")
    });
    let parent_row = inactive
        .lines()
        .find(|line| line.contains("Live parent") && line.contains('●'))
        .unwrap();
    assert!(
        parent_row.split('│').next().unwrap().contains("Working"),
        "{inactive}"
    );
    assert!(
        parent_row.split('│').next().unwrap().contains("2h ago"),
        "{inactive}"
    );
    let child_row = inactive
        .lines()
        .find(|line| line.contains("Finished child"))
        .unwrap();
    assert!(
        child_row.split('│').next().unwrap().contains("Inactive"),
        "{inactive}"
    );
    tmux.run(&["send-keys", "-t", "probe", "BTab"]);
    let active = tmux.capture_when(|capture| {
        listed(capture, "Working child") && !listed(capture, "Finished child")
    });
    let parent_row = active
        .lines()
        .find(|line| line.contains("Live parent") && line.contains('●'))
        .unwrap();
    assert!(
        parent_row.split('│').next().unwrap().contains("Working"),
        "{active}"
    );
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let default_selection = tmux.capture_when(|capture| {
        selected(capture, "Live parent") && listed(capture, "Finished child")
    });
    let first_task = default_selection
        .lines()
        .skip(2)
        .find(|line| line.split('│').next().unwrap().contains('●'))
        .unwrap();
    assert!(first_task.contains("Live parent"), "{default_selection}");
    tmux.run(&["send-keys", "-t", "probe", "Down"]);
    tmux.wait_for(|capture| selected(capture, "Finished child"));
    tmux.run(&["send-keys", "-t", "probe", "BTab"]);
    tmux.wait_for(|capture| listed(capture, "Working child"));
    tmux.run(&["send-keys", "-t", "probe", "Tab"]);
    let remembered_selection = tmux.capture_when(|capture| selected(capture, "Finished child"));
    tmux.run(&["send-keys", "-t", "probe", "Home"]);
    tmux.wait_for(|capture| selected(capture, "Live parent") && listed(capture, "Finished child"));
    entry["status"] = "idle".into();
    write_entry(&path, &entry);
    let refreshed = tmux.capture_when(|capture| {
        selected(capture, "Live parent")
            && listed(capture, "Finished child")
            && capture
                .lines()
                .nth(1)
                .is_some_and(|line| line.contains("Ready"))
    });
    let parent_row = refreshed
        .lines()
        .find(|line| line.contains("Live parent") && line.contains('○'))
        .unwrap();
    assert!(
        parent_row.split('│').next().unwrap().contains("Ready"),
        "{refreshed}"
    );
    assert!(
        parent_row.split('│').next().unwrap().contains("2h ago"),
        "{refreshed}"
    );
    let verdict = serde_json::json!({"passed": true, "active": active, "inactive": inactive, "default_selection": default_selection, "remembered_selection": remembered_selection, "refreshed": refreshed});
    if let Some(path) = std::env::var_os("AGENTS_STATUS_TABS_VERDICT") {
        std::fs::write(path, serde_json::to_vec_pretty(&verdict).unwrap()).unwrap();
    }
    println!("{verdict}");
}
