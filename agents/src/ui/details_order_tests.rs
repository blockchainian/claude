use super::*;

#[test]
fn details_section_order_preserves_clicks_and_copies() {
    for activity in [false, true] {
        let output = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(output.path(), "background output\n").unwrap();
        let mut view = fixture();
        let selected = &mut view.rows[view.selected];
        selected.status = SessionStatus::NeedsInput;
        selected.prompt = "Moved prompt".into();
        selected.queued = vec!["Queued prompt".into()];
        selected.tasks = vec![crate::model::SessionTask {
            id: "1".into(),
            subject: "Moved task".into(),
            status: crate::model::TaskStatus::Pending,
        }];
        selected.last_message = Some("Moved reply".into());
        selected.last_message_at = Some(selected.updated_at);
        selected.activity = activity.then(|| crate::model::Activity {
            exit_code: None,
            kind: crate::log::Kind::Tool,
            label: "Moved activity".into(),
            at: selected.updated_at + Duration::seconds(1),
        });
        selected.background.push(crate::model::BackgroundTask {
            description: "Moved background".into(),
            output: output.path().into(),
            started_at: selected.updated_at,
        });
        let now = selected.updated_at;
        let area = Rect::new(5, 3, 80, 40);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        let rendered = buf
            .content
            .chunks(80)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let mut sections = vec![];
        if activity {
            sections.push("Last activity");
        }
        sections.extend([
            "Background",
            "Update",
            "Prompt",
            "Queued prompt",
            "Needs input",
            "Tasks",
        ]);
        assert!(!rendered.contains("Project"));
        assert!(!rendered.contains("Unknown"));
        let positions: Vec<_> = sections
            .iter()
            .map(|text| rendered.find(text).unwrap())
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{rendered}"
        );
        let point = |text: &str| {
            let (x, y) = rendered_cell(&rendered, text);
            (x + area.x, y + area.y)
        };
        let mut copies = vec![
            ("Moved reply", "Copied update"),
            ("Moved prompt", "Copied prompt"),
        ];
        if activity {
            copies.push(("Moved activity", "Copied latest activity"));
        }
        for (body, copied) in copies {
            let (x, y) = point(body);
            let press = mouse(MouseEventKind::Down(MouseButton::Left), x, y);
            assert_eq!(click_mouse(&mut view, press), None);
            assert_eq!(
                click_mouse(&mut view, press),
                Some(Action::Copy {
                    text: body.into(),
                    label: copied.into(),
                })
            );
        }
        let (x, y) = point("Moved background");
        assert_eq!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, y)
            ),
            None
        );
        assert_eq!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, y)
            ),
            None
        );
        assert_eq!(
            super::super::log::open_text(view.visible_log_event().unwrap()),
            "background output"
        );
    }
}
