use super::*;

fn visible(cell: &ratatui::buffer::Cell) -> &str {
    cell_text(cell.symbol())
        .rsplit("\x1b\\")
        .find(|part| !part.is_empty() && !part.starts_with("\x1b]8"))
        .unwrap_or("")
        .split("\x1b]8")
        .next()
        .unwrap()
}

#[test]
fn prompt_cache_countdown_precedes_wall_time_and_expiry_follows_status() {
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
            let pause = "⏸\u{fe0e}";
            if seconds.is_some_and(|seconds| seconds < 1) {
                let status = format!("{} {pause} ", View::status(&view.rows[view.selected]).0);
                assert!(row.contains(&status), "{width} {seconds:?}: {row}");
                let x = row[..row.find(pause).unwrap()].chars().count() as u16;
                assert!(
                    buf[(x, y)]
                        .modifier
                        .contains(ratatui::style::Modifier::BOLD)
                );
            } else {
                assert!(!row.contains(pause), "{width} {seconds:?}: {row}");
            }
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
fn claude_subagent_prompt_cache_countdown_shows_only_in_its_last_minute() {
    let now = DateTime::UNIX_EPOCH + Duration::days(20_000);
    for (seconds, shown) in [(59, true), (60, false), (180, false)] {
        let mut view = fixture();
        let selected = &mut view.rows[view.selected];
        selected.parent = Some(session_key("43"));
        selected.started_at = Some(now - Duration::hours(2));
        selected.cache_expires_at = Some(now + Duration::seconds(seconds));
        let mut buf = Buffer::empty(Rect::new(0, 0, 220, 50));
        view.render_at(buf.area, &mut buf, now);
        let shows = (0..buf.area.height).any(|y| {
            (0..buf.area.width)
                .map(|x| visible(&buf[(x, y)]))
                .collect::<String>()
                .contains(" · 2h 0m")
        });
        assert_eq!(shows, shown, "{seconds}");
    }
}

#[test]
fn status_rule_flanks_status_with_folder_and_optional_pr_links() {
    for pr in [false, true] {
        for width in [100, 149, 150, 220] {
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
            let task_details = view.viewport.borrow().task_details;
            let links = view.viewport.borrow().task_details_links.clone();
            let (folder, _) = links
                .iter()
                .find(|(_, target)| crate::ui::link_path(target) == crate::test_project())
                .unwrap();
            let row = (task_details.x..task_details.right())
                .map(|x| visible(&buf[(x, folder.y)]))
                .collect::<String>();
            assert!(!row.contains("Ready"), "{width}: {row}");
            assert!(
                row.contains(view.rows[view.selected].display_title()),
                "{width}: {row}"
            );
            assert_eq!(row.contains("hidden-branch"), !pr, "{width}: {row}");
            assert_eq!(folder.x, task_details.x);
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
                    task_details.right()
                        - u16::from(
                            crate::ui::render::ScreenLayout::for_width(width)
                                != crate::ui::render::ScreenLayout::Large
                        )
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
                        Some(Action::Open(target.clone()))
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
        view.project_groups = view
            .rows
            .iter()
            .map(|session| ProjectGroup::for_session(session, &view.task_repositories))
            .collect();
        let mut buf = Buffer::empty(Rect::new(0, 0, 150, 50));
        view.render(buf.area, &mut buf);
        let (folder, target) = view.viewport.borrow().task_details_links[0].clone();
        assert_eq!(crate::ui::link_path(&target), cwd);
        assert_eq!(
            (folder.x..folder.right())
                .map(|x| visible(&buf[(x, folder.y)]))
                .collect::<String>(),
            "repository"
        );
        let mut task = session("tree", "T", SessionStatus::Ready, 0);
        task.cwd = cwd.clone();
        let mut list = test_view(Grouping::Project, false);
        list.set_sessions(vec![task], false);
        assert_eq!(list.group_heading(0, 80), "repository");
    }
}

#[test]
fn status_rule_names_a_missing_directory_without_a_link() {
    let mut view = fixture();
    let selected = &mut view.rows[view.selected];
    selected.cwd = "/nonexistent/build".into();
    view.project_groups = view
        .rows
        .iter()
        .map(|session| ProjectGroup::for_session(session, &view.task_repositories))
        .collect();
    let mut buf = Buffer::empty(Rect::new(0, 0, 150, 50));
    view.render(buf.area, &mut buf);
    let viewport = view.viewport.borrow();
    let rule = (0..buf.area.width)
        .map(|x| visible(&buf[(x, viewport.task_details.bottom())]))
        .collect::<String>();
    assert!(rule.contains("build"), "{rule}");
    assert!(
        viewport
            .task_details_links
            .iter()
            .all(|(_, target)| crate::ui::link_path(target) != Path::new("/nonexistent/build"))
    );
}
