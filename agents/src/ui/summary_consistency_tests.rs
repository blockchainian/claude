use super::*;

fn recorded_summaries() -> Vec<crate::event::Event> {
    use crate::providers::Provider;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let prose = crate::providers::claude::event_reader::tail_session(
        &root.join("transcript.jsonl"),
        usize::MAX,
        true,
    )
    .unwrap()
    .into_iter()
    .find(|event| event.kind == crate::event::Kind::Assistant && event.body.contains('`'))
    .unwrap();
    let read = crate::providers::claude::event_reader::tail(
        &root.join("claude-tool-calls.jsonl"),
        usize::MAX,
    )
    .unwrap()
    .into_iter()
    .find(|event| event.calls.iter().any(|call| call.name == "Read"))
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let provider =
        crate::providers::codex::CodexProvider::new(dir.path().into());
    let mut row = session("failed", "Failure", SessionStatus::Working, 0);
    row.transcript = Some(root.join("codex-command-failed.jsonl"));
    let failed = provider
        .event_panel(&row, usize::MAX, false)
        .unwrap()
        .pop()
        .unwrap();
    assert!(!read.paths.is_empty());
    assert_eq!(failed.calls[0].exit_code, Some(1));
    let bold = crate::providers::claude::event_reader::tail_session(
        &root.join("claude-notification-reply.jsonl"),
        usize::MAX,
        true,
    )
    .unwrap()
    .into_iter()
    .find(|event| event.kind == crate::event::Kind::Assistant && event.body.contains("**"))
    .unwrap();
    vec![prose, bold, read, failed]
}

fn assert_summary_surface(event: &crate::event::Event, width: u16, child: bool) -> String {
    let now = event.started_at;
    let mut row = session("summary", "Summary", SessionStatus::Working, 0);
    row.cwd = PathBuf::from("/workspace");
    row.activity = Some(event.activity());
    row.prompt.clear();
    row.last_message = None;
    let mut view = test_view(Grouping::Project, true);
    if child {
        let mut parent = session("parent", "Parent", SessionStatus::Ready, 0);
        parent.prompt.clear();
        row.parent = Some(parent.id.clone());
        view.set_sessions(vec![parent, row], false);
        view.selected = view
            .rows
            .iter()
            .position(|row| row.id == session_key("parent"))
            .unwrap();
    } else {
        view.set_sessions(vec![row], false);
    }
    let area = Rect::new(0, 0, width, 24);
    let mut actual = Buffer::empty(area);
    view.render_task_details(area, &mut actual, now);
    let line = super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
        super::super::event_list::body_line(event, usize::from(width), Path::new("/workspace")),
        usize::from(width),
    );
    let mut expected = Buffer::empty(Rect::new(0, 0, width, 1));
    line.clone().render(expected.area, &mut expected);
    let rendered = actual
        .content
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(|c| cell_text(c.symbol()))
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let y = rendered
        .iter()
        .position(|row| {
            row.trim()
                == expected
                    .content
                    .iter()
                    .map(|c| cell_text(c.symbol()))
                    .collect::<String>()
                    .trim()
        })
        .unwrap_or_else(|| {
            panic!(
                "missing {:?}, child={child}, width={width}:\n{}",
                line.to_string(),
                rendered.join("\n")
            )
        });
    let indent = (usize::from(width) - line.width()) / 2;
    for x in 0..line.width().min(usize::from(width)) {
        let got = &actual[((indent + x) as u16, y as u16)];
        let want = &expected[(x as u16, 0)];
        assert_eq!(
            cell_text(got.symbol()),
            cell_text(want.symbol()),
            "child={child}, width={width}, column={x}"
        );
        assert_eq!(
            got.style(),
            want.style(),
            "child={child}, width={width}, column={x}, text={:?}",
            line.to_string()
        );
    }
    if !child {
        let expected_links =
            super::super::links::path_links(&event.body, &event.paths, Path::new("/workspace"));
        for link in expected_links {
            assert!(
                view.viewport
                    .borrow()
                    .task_details_links
                    .iter()
                    .any(|(_, target)| *target == link.target),
                "missing activity path link: {link:?}"
            );
        }
    }
    let mut snapshot = format!(
        "{} width={width}\n{}\n",
        if child { "Subagent" } else { "Last activity" },
        actual
            .content
            .chunks(usize::from(width))
            .nth(y)
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(x, _)| *x == 0 || actual[((x - 1) as u16, y as u16)].symbol().width() < 2)
            .map(|(_, cell)| cell.symbol())
            .collect::<String>()
            .trim_end()
    );
    let mut previous = None;
    for x in 0..indent + line.width().min(usize::from(width)) {
        if x > 0 && actual[((x - 1) as u16, y as u16)].symbol().width() > 1 {
            continue;
        }
        let style = actual[(x as u16, y as u16)].style();
        if previous != Some(style) {
            snapshot.push_str(&format!("{x}: {style:?}\n"));
            previous = Some(style);
        }
    }
    snapshot
}

#[test]
fn recorded_last_activity_matches_event_summary() {
    for event in recorded_summaries() {
        for width in [24, 64, 120] {
            assert_summary_surface(&event, width, false);
        }
    }
}

#[test]
fn recorded_subagent_matches_event_summary() {
    for event in recorded_summaries() {
        for width in [24, 64, 120] {
            assert_summary_surface(&event, width, true);
        }
    }
}

#[test]
fn recorded_summary_surfaces_snapshot() {
    let mut snapshot = String::new();
    for event in recorded_summaries() {
        for child in [false, true] {
            snapshot.push_str(&assert_summary_surface(&event, 120, child));
        }
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui_snapshots/summary_surfaces.txt");
    if std::env::var_os("UPDATE_SUMMARY_SNAPSHOT").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}

#[test]
fn recorded_read_with_spaces_keeps_provider_path_range() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("folder with spaces").join("tmux.conf");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "").unwrap();
    let mut record: serde_json::Value =
        include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["message"]["content"][0]["name"] == "Read")
            .unwrap();
    record["message"]["content"][0]["input"]["file_path"] = file.to_str().unwrap().into();
    let transcript = dir.path().join("read.jsonl");
    std::fs::write(&transcript, format!("{record}\n")).unwrap();
    let event = crate::providers::claude::event_reader::tail(&transcript, usize::MAX)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(&event.body[event.paths[0].clone()], file.to_str().unwrap());
    for width in [24, 64, 120] {
        for child in [false, true] {
            assert_summary_surface(&event, width, child);
        }
    }
}
