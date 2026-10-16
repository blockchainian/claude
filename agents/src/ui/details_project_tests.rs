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
fn status_rule_flanks_status_with_folder_and_optional_pr_links() {
    for pr in [false, true] {
        for width in [80, 149, 150, 220] {
            let mut view = fixture();
            let selected = &mut view.rows[view.selected];
            selected.git_branch = Some("hidden-branch".into());
            selected.pr = pr.then(|| crate::model::PullRequest {
                number: 833,
                url: "https://example.com/pull/833".into(),
            });
            let id = selected.id.clone();
            let mut buf = Buffer::empty(Rect::new(0, 0, width, 50));
            view.render(buf.area, &mut buf);
            let details = view.viewport.borrow().details;
            let links = view.viewport.borrow().details_links.clone();
            let (folder, _) = links
                .iter()
                .find(|(_, target)| target == std::path::Path::new("/tmp/project"))
                .unwrap();
            let row = (details.x..details.right())
                .map(|x| visible(&buf[(x, folder.y)]))
                .collect::<String>();
            assert!(row.contains("Ready"), "{width}: {row}");
            assert!(!row.contains("hidden-branch"));
            assert_eq!(folder.x, details.x + 1);
            assert_eq!(
                (folder.x..folder.right())
                    .map(|x| visible(&buf[(x, folder.y)]))
                    .collect::<String>(),
                "project"
            );
            let mut reference = ratatui::buffer::Cell::default();
            reference.set_style(style::time_style());
            assert_eq!(buf[(folder.x, folder.y)].style(), reference.style());
            assert_eq!(links.len(), if pr { 2 } else { 1 });
            if pr {
                let (rect, _) = &links[1];
                assert_eq!(rect.y, folder.y);
                assert_eq!(
                    rect.right(),
                    details.right() - if width >= 150 { 1 } else { 2 }
                );
                assert_eq!(
                    (rect.x..rect.right())
                        .map(|x| visible(&buf[(x, rect.y)]))
                        .collect::<String>(),
                    "#833"
                );
                assert!(
                    buf[(rect.x, rect.y)]
                        .symbol()
                        .contains("\x1b]8;;https://example.com/pull/833")
                );
                reference = ratatui::buffer::Cell::default();
                reference.set_style(view.title_style(&id));
                assert_eq!(buf[(rect.x, rect.y)].style(), reference.style());
            } else {
                assert!(!row.contains('#'));
            }
            for (rect, target) in links {
                for x in rect.x..rect.right() {
                    assert_eq!(
                        click_mouse(
                            &mut view,
                            mouse(MouseEventKind::Down(MouseButton::Left), x, rect.y)
                        ),
                        Some(Action::OpenPath(target.clone()))
                    );
                }
            }
        }
    }
}
