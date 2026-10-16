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

#[test]
fn details_groups_are_right_aligned() {
    let now = Utc::now();
    for width in 32..=61 {
        let mut row = session("header", "Header task", SessionStatus::Working, 0);
        row.git_branch = Some("agents-panel".into());
        row.pr = Some(crate::model::PullRequest {
            number: 13,
            url: "https://github.com/blockchainian/claude/pull/13".into(),
        });
        row.prompt_at = Some(now - Duration::seconds(64));
        row.turn_messages = 10;
        row.effort = Some("high".into());
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        let area = Rect::new(5, 3, width, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        for parts in [
            ["Project", "agents-panel", "#13"],
            ["Prompt", "10 messages", "1m 4s"],
            ["Model", "high", "Unknown"],
        ] {
            let y = (area.y..area.bottom())
                .find(|&y| {
                    (area.x..area.right())
                        .map(|x| visible(&buf[(x, y)]))
                        .collect::<String>()
                        .starts_with(parts[0])
                })
                .unwrap();
            let text = (area.x..area.right())
                .map(|x| visible(&buf[(x, y)]))
                .collect::<String>();
            let middle = usize::from(width) - parts[1].len() - 3 - parts[2].len();
            let right = usize::from(width) - parts[2].len();
            assert_eq!(
                text.find(parts[1])
                    .map(|index| text[..index].chars().count()),
                Some(middle),
                "{text}"
            );
            assert_eq!(
                text.find(parts[2])
                    .map(|index| text[..index].chars().count()),
                Some(right),
                "{text}"
            );
            let dots = text
                .chars()
                .enumerate()
                .filter_map(|(x, c)| (c == '·').then_some(x))
                .collect::<Vec<_>>();
            let expected = [middle + parts[1].len() + 1];
            assert_eq!(dots, expected, "{text}");
            for x in dots {
                assert!(
                    buf[(area.x + x as u16, y)]
                        .modifier
                        .contains(ratatui::style::Modifier::DIM)
                );
            }
            if parts[0] == "Project" {
                assert_eq!(
                    buf[(area.x + middle as u16, y)].style(),
                    buf[(area.x, y)].style()
                );
                assert_eq!(
                    click_mouse(
                        &mut view,
                        mouse(
                            MouseEventKind::Down(MouseButton::Left),
                            area.x + right as u16,
                            y
                        )
                    ),
                    Some(Action::OpenPath(
                        super::super::vscode::pull_request_link(
                            "https://github.com/blockchainian/claude/pull/13",
                            std::path::Path::new("/tmp/project")
                        )
                        .unwrap()
                        .into()
                    ))
                );
            }
        }
    }
}

#[test]
fn narrow_details_groups_preserve_labels() {
    let now = Utc::now();
    let mut row = session("header", "Header task", SessionStatus::Working, 0);
    row.git_branch = Some("agents-panel".into());
    row.pr = Some(crate::model::PullRequest {
        number: 13,
        url: "https://github.com/blockchainian/claude/pull/13".into(),
    });
    row.prompt_at = Some(now - Duration::seconds(64));
    row.turn_messages = 10;
    row.effort = Some("high".into());
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    for width in 1..29 {
        let area = Rect::new(0, 0, width, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        if width >= 7 {
            for (label, group) in [
                ("Project", "agents-panel · #13"),
                ("Prompt", "10 messages · 1m 4s"),
                ("Model", "high · Unknown"),
            ] {
                let available = usize::from(width).saturating_sub(label.len() + 1);
                let group = if available == 0 {
                    String::new()
                } else if group.chars().count() > available {
                    group.chars().take(available - 1).collect::<String>() + "…"
                } else {
                    group.to_owned()
                };
                let expected = format!(
                    "{label}{}{group}",
                    " ".repeat(usize::from(width) - label.len() - group.chars().count())
                );
                assert!(
                    buf.content.chunks(usize::from(width)).any(|row| row
                        .iter()
                        .map(visible)
                        .collect::<String>()
                        .trim_end()
                        == expected.trim_end()),
                    "{width}: {expected}"
                );
            }
        }
    }
}
