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
fn prompt_cache_countdown_uses_error_style_before_wall_time() {
    let now = DateTime::UNIX_EPOCH + Duration::days(20_000);
    for seconds in [
        None,
        Some(-1),
        Some(0),
        Some(1),
        Some(725),
        Some(900),
        Some(901),
    ] {
        for width in [149, 220] {
            let mut view = fixture();
            let selected = &mut view.rows[view.selected];
            selected.started_at = Some(now - Duration::hours(2));
            selected.cache_expires_at = seconds.map(|seconds| now + Duration::seconds(seconds));
            let mut buf = Buffer::empty(Rect::new(0, 0, width, 50));
            view.render_at(buf.area, &mut buf, now);
            let y = (0..buf.area.height)
                .find(|&y| {
                    (0..width)
                        .map(|x| visible(&buf[(x, y)]))
                        .collect::<String>()
                        .contains("2h 0m")
                })
                .unwrap();
            let row = (0..width)
                .map(|x| visible(&buf[(x, y)]))
                .collect::<String>();
            if let Some(seconds) = seconds.filter(|seconds| (1..=900).contains(seconds)) {
                let countdown = formatting::format_duration(seconds);
                let expected = format!("{countdown} · 2h 0m");
                assert!(row.contains(&expected), "{width} {seconds}: {row}");
                let x = row[..row.find(&expected).unwrap()].chars().count() as u16;
                assert_eq!(buf[(x, y)].fg, style::error_style().fg.unwrap());
                assert!(!buf[(x, y)].modifier.contains(ratatui::style::Modifier::DIM));
                let wall_x = x + countdown.len() as u16 + 3;
                assert!(
                    buf[(wall_x, y)]
                        .modifier
                        .contains(ratatui::style::Modifier::DIM)
                );
            } else {
                assert!(!row.contains(" · 2h 0m"), "{seconds:?}: {row}");
            }
        }
    }
}

#[test]
fn status_rule_flanks_status_with_folder_and_optional_pr_links() {
    for pr in [false, true] {
        for width in [80, 149, 150, 220] {
            let mut view = fixture();
            let selected = &mut view.rows[view.selected];
            selected.title = "T".into();
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
            assert!(!row.contains("Ready"), "{width}: {row}");
            assert!(
                row.contains(view.rows[view.selected].display_title()),
                "{width}: {row}"
            );
            assert_eq!(row.contains("hidden-branch"), !pr, "{width}: {row}");
            assert_eq!(folder.x, details.x);
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
                assert_eq!(rect.right(), details.right() - u16::from(width < 150));
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

#[test]
fn status_rule_names_a_worktree_task_by_its_repository_and_opens_the_worktree() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repository");
    let worktree = directory.path().join("feature-tree");
    std::fs::create_dir(&repository).unwrap();
    for arguments in [
        &["init", "-b", "main"][..],
        &[
            "-c",
            "user.name=Probe",
            "-c",
            "user.email=probe@example.test",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
        &[
            "worktree",
            "add",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    ] {
        let output = std::process::Command::new("git")
            .args(arguments)
            .current_dir(&repository)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    for cwd in [&repository, &worktree] {
        let mut view = fixture();
        let selected = &mut view.rows[view.selected];
        selected.title = "T".into();
        selected.cwd = cwd.clone();
        view.project_groups = view.rows.iter().map(ProjectGroup::for_session).collect();
        let mut buf = Buffer::empty(Rect::new(0, 0, 150, 50));
        view.render(buf.area, &mut buf);
        let (folder, target) = view.viewport.borrow().details_links[0].clone();
        assert_eq!(&target, cwd);
        assert_eq!(
            (folder.x..folder.right())
                .map(|x| visible(&buf[(x, folder.y)]))
                .collect::<String>(),
            "repository"
        );
    }
}
