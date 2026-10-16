use super::*;
use std::io::Write;

#[test]
fn background_entries_have_one_blank_row_and_clicks_follow_the_boxes() {
    let now = Utc::now();
    let mut output = tempfile::NamedTempFile::new().unwrap();
    output
        .write_all(include_bytes!(
            "../../tests/fixtures/background/detached.output"
        ))
        .unwrap();
    let mut parent = session("spacing", "Background spacing", SessionStatus::Ready, 0);
    for description in ["First task", "Second task"] {
        parent.background.push(crate::model::BackgroundTask {
            description: description.into(),
            output: output.path().into(),
            started_at: now,
        });
    }
    let mut child = session("spacing-child", "Child task", SessionStatus::Working, 0);
    child.parent = Some(parent.id.clone());
    child.last_message = Some("Reading files".into());
    let mut view = test_view(Grouping::Project, false);
    view.set_sessions(vec![parent, child], false);
    view.selected = view
        .rows
        .iter()
        .position(|row| row.id == session_key("spacing"))
        .unwrap();
    let area = Rect::new(0, 0, 60, 30);
    let mut buf = Buffer::empty(area);
    view.render_details(area, &mut buf, now);
    let rows: Vec<_> = buf
        .content
        .chunks(60)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    assert_eq!(rows[0], "Background");
    assert!(rows[1].contains("First task"));
    assert!(rows[3].is_empty(), "background entries need a blank row");
    assert!(rows[4].contains("Second task"));
    assert!(rows[6].is_empty());
    assert!(rows[7].contains("Child task"));
    let viewport = view.viewport.borrow();
    assert_eq!(viewport.details_background.len(), 2);
    assert_eq!(viewport.details_background[0].0, 1..3);
    assert_eq!(viewport.details_background[1].0, 4..6);
    assert!(
        viewport
            .details_background
            .iter()
            .all(|(rows, _)| !rows.contains(&3) && !rows.contains(&6))
    );
}
