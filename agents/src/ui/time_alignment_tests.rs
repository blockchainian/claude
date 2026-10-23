use super::super::render::ScreenLayout;
use super::*;

fn frame(width: u16) -> (View, Buffer) {
    let (mut view, now) = event_fixture();
    let row = &mut view.rows[view.selected];
    row.activity = Some(crate::model::Activity {
        kind: crate::event::Kind::Tool,
        label: "Check files".into(),
        at: now - Duration::seconds(45),
        paths: Vec::new(),
        exit_code: None,
    });
    row.last_message = Some("Checking files".into());
    row.last_message_at = Some(now - Duration::seconds(45));
    row.prompt_at = Some(now - Duration::seconds(66));
    row.started_at = Some(now - Duration::seconds(66));
    row.updated_at = now - Duration::seconds(45);
    row.background.push(crate::model::BackgroundTask {
        description: "Check background".into(),
        output: None,
        started_at: now - Duration::seconds(66),
    });
    row.tasks.push(crate::model::SessionTask {
        id: "1".into(),
        subject: "Check alignment".into(),
        status: crate::model::TaskStatus::InProgress,
    });
    let area = Rect::new(0, 0, width, 60);
    let mut buf = Buffer::empty(area);
    view.render_at(area, &mut buf, now);
    (view, buf)
}

fn text(buf: &Buffer, area: Rect) -> String {
    (area.x..area.right())
        .map(|x| buf[(x, area.y)].symbol())
        .collect()
}

fn end_of(buf: &Buffer, area: Rect, label: &str) -> (u16, u16) {
    for y in area.y..area.bottom() {
        let line = text(
            buf,
            Rect {
                y,
                height: 1,
                ..area
            },
        );
        if let Some(start) = line.find(label) {
            return (area.x + line[..start + label.len()].width() as u16 - 1, y);
        }
    }
    panic!("missing {label:?} in {area:?}");
}

fn label_end(view: &View, buf: &Buffer) -> u16 {
    let layout = view.center_layout(buf.area);
    let y = layout.task_details.y - 1;
    let x = (layout.task_details.x..layout.task_details.right() + 1)
        .rev()
        .find(|&x| buf[(x, y)].symbol() == "╶")
        .expect("status rule right label");
    x - 1
}

fn assert_event_times(view: &View, buf: &Buffer) {
    let layout = view.center_layout(buf.area);
    let expected = buf.area.width - 3;
    assert_eq!(end_of(buf, layout.event_panel, "35s").0, expected);
    assert_eq!(
        end_of(buf, view.viewport.borrow().event_detail, "35s").0,
        expected
    );
}

#[test]
fn section_times_align_with_frame_labels_in_large_medium_small() {
    for width in [160, 120, 90] {
        for (label, value) in [
            ("Last activity", "45s ago"),
            ("Update", "45s ago"),
            ("Prompt", "1m 6s"),
            ("Tasks", "1/1"),
        ] {
            let (mut view, mut buf) = frame(width);
            let selected = view.selected;
            let now = view.rows[selected].updated_at + Duration::seconds(45);
            let row = &mut view.rows[selected];
            row.background.clear();
            if label == "Tasks" {
                row.activity = None;
                row.last_message = None;
            } else {
                row.tasks.clear();
            }
            buf.reset();
            view.render_at(buf.area, &mut buf, now);
            let layout = view.center_layout(buf.area);
            let expected = label_end(&view, &buf);
            if layout.kind != ScreenLayout::Large {
                assert_eq!(expected, width - 3);
                let (_, event_time) = super::super::event_list::event_row_areas(layout.event_panel);
                assert_eq!(event_time.right() - 1, expected);
            }
            let (_, y) = end_of(&buf, layout.task_details, label);
            assert_eq!(
                end_of(
                    &buf,
                    Rect {
                        y,
                        height: 1,
                        ..layout.task_details
                    },
                    value
                )
                .0,
                expected,
                "{width} {label}"
            );
        }
    }
}

#[test]
fn background_times_and_rule_tails_align_in_large_medium_small() {
    for width in [160, 120, 90] {
        let (view, buf) = frame(width);
        let layout = view.center_layout(buf.area);
        let expected = label_end(&view, &buf);
        assert_event_times(&view, &buf);
        let (_, y) = end_of(&buf, layout.task_details, "Check background");
        assert_eq!(
            end_of(
                &buf,
                Rect {
                    y,
                    height: 1,
                    ..layout.task_details
                },
                "1m 6s"
            )
            .0,
            expected,
            "{width} background"
        );
        assert_eq!(buf[(expected + 1, y)].symbol(), "╶", "{width}");
        assert_eq!(
            buf[(expected + 2, y)].symbol(),
            if layout.kind == ScreenLayout::Large {
                "│"
            } else {
                "─"
            },
            "{width}"
        );
        let (_, event_time) = super::super::event_list::event_row_areas(layout.event_panel);
        assert_eq!(event_time.right() - 1, width - 3);
    }
}

#[test]
fn task_list_time_and_status_columns_in_large_medium_small() {
    for width in [160, 120, 90] {
        let (view, buf) = frame(width);
        let layout = view.center_layout(buf.area);
        let (_, y) = end_of(&buf, layout.list, "Task log example");
        if layout.kind == ScreenLayout::Large {
            continue; // The large list deliberately omits its time when metadata cannot fit.
        }
        let expected = if layout.kind == ScreenLayout::Small {
            label_end(&view, &buf)
        } else {
            layout.list.right() - 1
        };
        assert_eq!(
            end_of(
                &buf,
                Rect {
                    y,
                    height: 1,
                    ..layout.list
                },
                "45s ago"
            )
            .0,
            expected,
            "{width} list"
        );
        let status_end = end_of(
            &buf,
            Rect {
                y,
                height: 1,
                ..layout.list
            },
            "Working",
        )
        .0;
        assert_eq!(status_end, expected - 15, "{width} status spacing");
    }
}
