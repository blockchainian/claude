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
fn three_parts_and_dots_fill_the_content_width() {
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
        let mut view = test_view(Grouping::Project, true);
        view.set_sessions(vec![row], false);
        let area = Rect::new(5, 3, width, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        for parts in [
            ["Project", "agents-panel", "#13"],
            ["Prompt", "10 messages", "1m 4s"],
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
            let gap = (usize::from(width) - parts.iter().map(|part| part.len()).sum::<usize>()) / 2;
            let middle = parts[0].len() + gap;
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
            let expected = [
                parts[0].len() + (gap - 1) / 2,
                middle + parts[1].len() + (right - middle - parts[1].len() - 1) / 2,
            ];
            assert_eq!(dots, expected, "{text}");
            for x in dots {
                assert!(
                    buf[(area.x + x as u16, y)]
                        .modifier
                        .contains(ratatui::style::Modifier::DIM)
                );
            }
            if parts[0] == "Project" {
                assert_eq!(buf[(area.x + middle as u16, y)].fg, style::thread_color(0));
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
fn narrow_three_part_headers_use_compact_truncation() {
    let now = Utc::now();
    let mut row = session("header", "Header task", SessionStatus::Working, 0);
    row.git_branch = Some("agents-panel".into());
    row.pr = Some(crate::model::PullRequest {
        number: 13,
        url: "https://github.com/blockchainian/claude/pull/13".into(),
    });
    row.prompt_at = Some(now - Duration::seconds(64));
    row.turn_messages = 10;
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(vec![row], false);
    for width in 1..29 {
        let area = Rect::new(0, 0, width, 24);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, now);
        if width >= 7 {
            for text in [
                "Project · agents-panel · #13",
                "Prompt · 10 messages · 1m 4s",
            ] {
                let expected =
                    super::super::line_truncation::truncate_line_with_ellipsis_if_overflow(
                        ratatui::text::Line::from(text),
                        usize::from(width),
                    )
                    .to_string();
                assert!(
                    buf.content.chunks(usize::from(width)).any(|row| row
                        .iter()
                        .map(visible)
                        .collect::<String>()
                        .trim_end()
                        == expected),
                    "{width}: {expected}"
                );
            }
        }
    }
}
