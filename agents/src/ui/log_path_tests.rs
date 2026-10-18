use super::*;

#[test]
fn cwd_paths_match_in_tool_titles_bodies_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("agents/src/ui/draft_flow_tests.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "fn main() {}\n").unwrap();
    for name in ["Read", "Edit"] {
        let mut record: serde_json::Value =
            include_str!("../../tests/fixtures/claude-tool-calls.jsonl")
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .find(|record| record["message"]["content"][0]["name"] == name)
                .unwrap();
        record["message"]["content"][0]["input"]["file_path"] = file.to_str().unwrap().into();
        let transcript = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(transcript.path(), format!("{record}\n")).unwrap();
        let event = crate::providers::claude::log::tail(transcript.path(), 1)
            .unwrap()
            .remove(0);
        let layout = detail_layout(&event, 160, dir.path(), event.started_at);
        assert!(
            layout.lines[0]
                .to_string()
                .contains("agents/src/ui/draft_flow_tests.rs")
        );
        assert!(
            !layout.lines[0]
                .to_string()
                .contains(dir.path().to_str().unwrap()),
            "{name}: {:?}",
            layout.lines[0]
        );
        let links = layout.links[0].clone();
        let rows = [text_selection::DrawnRow {
            segment: 0,
            x: 0,
            y: 0,
            cells: layout.rows[0].1.clone(),
        }];
        let drawn = super::links::drawn_links(&rows, &links);
        assert!(!drawn.is_empty(), "{name}: title lost its link");
        assert_eq!(drawn[0].1, links[0].target);
        assert!(
            links[0]
                .target
                .to_string_lossy()
                .contains(file.to_str().unwrap())
        );

        let mut prose = event.clone();
        prose.calls.clear();
        prose.kind = Kind::Assistant;
        prose.body = format!("See `{}`:272-274", file.display());
        assert_eq!(
            body_line(&prose, 300, dir.path()).to_string(),
            "See agents/src/ui/draft_flow_tests.rs:272-274"
        );
        let layout = detail_layout(&prose, 160, dir.path(), prose.started_at);
        let text = layout
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("agents/src/ui/draft_flow_tests.rs"), "{text}");
        assert!(!text.contains(dir.path().to_str().unwrap()), "{text}");
    }
}

#[test]
fn markdown_path_labels_are_relative_and_keep_the_absolute_destination() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("src/lib.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let transcript = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        transcript.path(),
        include_str!("../../tests/fixtures/transcript.jsonl"),
    )
    .unwrap();
    let mut event = crate::providers::claude::log::tail(transcript.path(), usize::MAX)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == Kind::Assistant)
        .unwrap();
    event.body = format!("See [{}]({})", file.display(), file.display());
    assert_eq!(
        body_line(&event, 200, dir.path()).to_string(),
        "See src/lib.rs"
    );
    let layout = detail_layout(&event, 160, dir.path(), event.started_at);
    assert!(
        layout
            .lines
            .iter()
            .any(|line| line.to_string().contains("See src/lib.rs"))
    );
    let drawn = layout
        .rows
        .iter()
        .enumerate()
        .flat_map(|(row, (id, cells))| {
            super::links::drawn_links(
                &[text_selection::DrawnRow {
                    segment: *id,
                    x: 0,
                    y: row as u16,
                    cells: cells.clone(),
                }],
                &layout.links[*id],
            )
        })
        .collect::<Vec<_>>();
    assert!(
        drawn
            .iter()
            .any(|(rect, target)| rect.width == 10 && target == &file),
        "{drawn:?}"
    );
}
