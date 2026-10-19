use super::*;

fn visible(cell: &ratatui::buffer::Cell) -> &str {
    cell.symbol()
        .rsplit("\x1b\\")
        .find(|part| !part.is_empty() && !part.starts_with("\x1b]8"))
        .unwrap_or("")
        .split("\x1b]8")
        .next()
        .unwrap()
}

fn text(buf: &Buffer, area: Rect, y: u16) -> String {
    (area.x..area.right())
        .map(|x| visible(&buf[(x, y)]))
        .collect()
}

#[test]
fn section_labels_stay_centered_before_right_values() {
    let now = Utc::now();
    let mut row = session("header", "Header task", SessionStatus::NeedsInput, 0);
    row.prompt_at = Some(now - Duration::seconds(64));
    row.last_record_at = Some(now);
    row.turn_messages = 10;
    row.last_message = Some("Reply".into());
    row.last_message_at = Some(now - Duration::seconds(7));
    row.activity = Some(crate::model::Activity {
        paths: Vec::new(),
        exit_code: None,
        kind: crate::log::Kind::Tool,
        label: "Thinking".into(),
        at: now - Duration::seconds(7),
    });
    let output = tempfile::NamedTempFile::new().unwrap();
    row.background.push(crate::model::BackgroundTask {
        description: "Background work".into(),
        output: Some(output.path().into()),
        started_at: now,
    });
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    for width in 15..=61 {
        let area = Rect::new(5, 3, width, 40);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        for (label, value) in [
            ("Last activity", "7s ago"),
            ("Background", ""),
            ("Update", "7s ago"),
            ("Prompt", "1m 4s"),
            ("Needs input", ""),
        ] {
            let y = (area.y..area.bottom())
                .find(|&y| text(&buf, area, y).contains(label))
                .unwrap();
            let rendered = text(&buf, area, y);
            let start = (usize::from(width) - label.len()) / 2;
            assert_eq!(rendered.find(label), Some(start), "{width}: {rendered}");
            assert!(!rendered.contains("message"));
            if !value.is_empty() && usize::from(width) - start - label.len() > value.len() {
                assert!(rendered.ends_with(value), "{rendered}");
            }
            if !value.is_empty() {
                let tail = rendered[start + label.len()..].trim();
                assert!(
                    tail.is_empty() || value.starts_with(tail.trim_end_matches('…')),
                    "{rendered}"
                );
            }
        }
    }
}

#[test]
fn status_rule_keeps_github_pr_hyperlink_and_visible_click_target() {
    let mut row = session("project", "A task", SessionStatus::Ready, 0);
    row.git_branch = Some("hidden-branch".into());
    row.pr = Some(crate::model::PullRequest {
        number: 13,
        url: "https://github.com/blockchainian/claude/pull/13".into(),
    });
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    let area = Rect::new(0, 0, 160, 50);
    let mut buf = Buffer::empty(area);
    view.render(area, &mut buf);
    let links = view.viewport.borrow().details_links.clone();
    let (rect, target) = links
        .iter()
        .find(|(_, target)| {
            target
                .to_str()
                .unwrap()
                .starts_with("vscode://blockchainian.git/view-pr?")
        })
        .unwrap();
    assert_eq!(text(&buf, *rect, rect.y), "#13");
    assert!(
        buf[(rect.x, rect.y)]
            .symbol()
            .contains("\x1b]8;;vscode://blockchainian.git/view-pr?")
    );
    assert_eq!(buf[(rect.x, rect.y)].fg, style::thread_color(0));
    assert_eq!(
        click_mouse(
            &mut view,
            mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y)
        ),
        Some(Action::OpenPath(target.clone()))
    );
}
