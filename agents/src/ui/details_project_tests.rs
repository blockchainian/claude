use super::*;
use unicode_width::UnicodeWidthStr;

#[test]
fn project_justifies_directory_and_one_right_value_with_visible_links() {
    for (pr, branch, right) in [
        (true, Some("main"), Some("#833")),
        (false, Some("main"), Some("main")),
        (false, None, None),
    ] {
        for width in [1, 5, 6, 8, 13, 20, 40] {
            let mut view = fixture();
            let selected = &mut view.rows[view.selected];
            selected.prompt = "Short prompt".into();
            selected.last_message = None;
            selected.activity = None;
            selected.git_branch = branch.map(str::to_owned);
            selected.pr = pr.then(|| crate::model::PullRequest {
                number: 833,
                url: "https://example.com/pull/833".into(),
            });
            let id = selected.id.clone();
            let area = Rect::new(7, 4, width, 100);
            let y = area.bottom() - 3;
            let mut buf = Buffer::empty(Rect::new(0, 0, area.right(), area.bottom()));
            view.render_details(area, &mut buf, Utc::now());
            let text = (area.x..area.right())
                .map(|x| {
                    let symbol = buf[(x, y)].symbol();
                    symbol
                        .rsplit("\x1b\\")
                        .find(|part| !part.is_empty() && !part.starts_with("\x1b]8"))
                        .unwrap_or("")
                        .split("\x1b]8")
                        .next()
                        .unwrap()
                })
                .collect::<String>();
            let right = right.filter(|right| usize::from(width) >= right.width() + 2);
            let links = view.viewport.borrow().details_links.clone();
            let path = links
                .iter()
                .find(|(_, target)| target == std::path::Path::new("/tmp/project"))
                .unwrap()
                .0;
            assert_eq!(path.x, area.x);
            assert_eq!(path.y, y);
            assert!(path.width > 0);
            if width == 40 {
                assert!(text.starts_with("/tmp/project"), "{text}");
            } else if width == 13 && right.is_some() {
                assert!(text.starts_with("/t…/"), "{text}");
            }
            if let Some(right) = right {
                assert!(text.ends_with(right), "{width}: {text}");
                let start = area.right() - right.width() as u16;
                assert!(path.right() < start);
                let expected_style = if pr {
                    view.title_style(&id)
                } else {
                    style::time_style()
                };
                let mut reference = ratatui::buffer::Cell::default();
                reference.set_style(expected_style);
                assert_eq!(buf[(start, y)].style(), reference.style());
            } else {
                assert_eq!(links.len(), 1);
            }
            assert!(!text.contains('·'));
            if pr && right.is_some() {
                assert!(!text.contains("main"));
            }
            let path_text = (path.x..path.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>();
            if width < 12 {
                assert!(path_text.ends_with('…'), "{path_text}");
            }
            for x in area.x..area.right() {
                let target = links.iter().find(|(rect, _)| rect.contains((x, y).into()));
                let expected = if path.contains((x, y).into()) {
                    Some(PathBuf::from("/tmp/project"))
                } else if pr && right.is_some() && x >= area.right() - 4 {
                    Some(PathBuf::from("https://example.com/pull/833"))
                } else {
                    None
                };
                assert_eq!(
                    target.map(|(_, target)| target.clone()),
                    expected,
                    "{width}: x={x}: {text}"
                );
                if let Some(target) = expected {
                    assert_eq!(
                        click_mouse(
                            &mut view,
                            mouse(MouseEventKind::Down(MouseButton::Left), x, y)
                        ),
                        Some(Action::OpenPath(target))
                    );
                }
            }
        }
    }
}
