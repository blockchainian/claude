use super::*;

mod task_details_budget {
    use super::*;
    use crate::ui::render::allocate;

    #[test]
    fn everything_fits() {
        assert_eq!(allocate(&[12, 6, 3], 25), vec![12, 6, 3]);
    }

    #[test]
    fn shares_rows_in_display_order() {
        assert_eq!(allocate(&[12, 6, 3], 8), vec![3, 3, 2]);
        assert_eq!(allocate(&[1, 12, 3], 10), vec![1, 6, 3]);
    }

    #[test]
    fn single_section_uses_budget() {
        assert_eq!(allocate(&[12], 5), vec![5]);
    }

    #[test]
    fn budget_below_floors() {
        for budget in 0..3 {
            let rows = allocate(&[12, 6, 3], budget);
            assert!(rows.iter().all(|&rows| rows <= 1));
            assert_eq!(rows.iter().sum::<usize>(), budget);
        }
        assert_eq!(allocate(&[0, 12, 0], 2), vec![0, 2, 0]);
    }

    #[test]
    fn background_gets_space_before_bodies_and_cuts_whole_boxes() {
        use std::io::Write;
        let mut output = tempfile::NamedTempFile::new().unwrap();
        writeln!(output, "Running checks").unwrap();
        let mut row = session(
            "background-budget",
            "Background task",
            SessionStatus::NeedsInput,
            0,
        );
        row.prompt = "Prompt text\n\n".repeat(12);
        row.queued = vec!["Queued work".into()];
        for i in 0..8 {
            row.background.push(crate::model::BackgroundTask {
                description: format!("Check {i}"),
                output: Some(output.path().into()),
                started_at: row.updated_at,
            });
        }
        let mut view = test_view(Grouping::Project, true);
        view.status_filter = 0;
        view.set_sessions(vec![row], false);
        let rows = panel(&view);
        assert!(
            rows.iter()
                .any(|line| line == &format!("{}Queued work", " ".repeat(54)))
        );
        assert!(
            rows.iter()
                .any(|line| line.starts_with("─") && line.contains("Check 4")),
            "{rows:?}"
        );
        assert!(!rows.iter().any(|line| line.contains("Check 5")));
        let prompt = rows
            .iter()
            .position(|line| line.trim() == "Prompt")
            .unwrap();
        let background = rows
            .iter()
            .position(|line| line.trim() == "Background")
            .unwrap();
        assert!(background < prompt);
        assert_eq!(
            rows[background + 1..prompt]
                .iter()
                .filter(|line| *line == "…")
                .count(),
            1
        );
        assert_eq!(
            &rows[prompt + 1..prompt + 4],
            [
                format!("{}Prompt text…", " ".repeat(54)),
                String::new(),
                format!("{}Queued work", " ".repeat(54))
            ]
        );
        let attention = rows
            .iter()
            .position(|line| line.trim() == "Needs input…")
            .unwrap();
        assert!(background < prompt && prompt < attention);
        assert_eq!(background, 0);
    }

    #[test]
    fn tiny_panel_keeps_plain_bottom_cut() {
        let mut row = session("tiny", "Tiny task", SessionStatus::NeedsInput, 0);
        row.prompt = "First row\n\nSecond row\n\nThird row".into();
        let mut view = test_view(Grouping::Project, true);
        view.status_filter = 0;
        view.set_sessions(vec![row], false);
        let area = Rect::new(0, 0, 120, 3);
        let mut buf = Buffer::empty(area);
        view.render_task_details(area, &mut buf, Utc::now());
        let row_text = |y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>();
        assert_eq!(row_text(0).trim(), "Prompt");
        assert_eq!(row_text(1).trim(), "First row");
        assert_eq!(buf[(0, 2)].symbol(), "…");
    }

    fn panel(view: &View) -> Vec<String> {
        let area = Rect::new(0, 0, 120, 19);
        let mut buf = Buffer::empty(area);
        view.render_task_details(area, &mut buf, Utc::now());
        buf.content
            .chunks(120)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    #[test]
    fn rest_budget_and_wheel_opens_prompt() {
        let mut row = session("budget", "Budget task", SessionStatus::NeedsInput, 0);
        row.prompt = (1..=12)
            .map(|i| format!("Prompt row {i}"))
            .collect::<Vec<_>>()
            .join("  \n");
        row.last_message = Some(
            (1..=6)
                .map(|i| format!("Reply row {i}"))
                .collect::<Vec<_>>()
                .join("  \n"),
        );
        assert_eq!(crate::ui::markdown::render(&row.prompt, 120).len(), 12);
        assert_eq!(
            crate::ui::markdown::render(row.last_message.as_ref().unwrap(), 120).len(),
            6
        );
        let mut view = test_view(Grouping::Project, true);
        view.status_filter = 0;
        view.set_sessions(vec![row], false);
        let rest = panel(&view);
        for header in ["Prompt", "Update", "Needs input"] {
            assert!(
                rest.iter().any(|line| line.trim() == header),
                "{header}: {rest:?}"
            );
        }
        let attention = rest
            .iter()
            .position(|line| line.trim() == "Needs input")
            .unwrap();
        assert_eq!(
            rest[attention + 2],
            format!("{}Waiting for your response.", " ".repeat(47))
        );
        let prompt = rest
            .iter()
            .position(|line| line.trim() == "Prompt")
            .unwrap();
        assert!(rest[..prompt].iter().any(|line| line.trim() == "Update"));
        assert!(
            rest[prompt + 1..attention]
                .iter()
                .rev()
                .find(|line| !line.is_empty())
                .unwrap()
                .ends_with('…')
        );
        view.scroll_content_row(true, false, (prompt + 1) as u16);
        assert_eq!(
            view.viewport
                .borrow()
                .task_details_scroll
                .reveal
                .iter()
                .sum::<usize>(),
            1
        );
        let scrolled = panel(&view);
        assert_eq!(scrolled[0], rest[1]);
        assert_eq!(
            scrolled[attention - 2],
            format!("{}Prompt row 7…", " ".repeat(53))
        );
        assert_eq!(scrolled[attention..], rest[attention..]);
        assert_eq!(scrolled[prompt], format!("{}Prompt row 1", " ".repeat(53)));
        view.scroll_content_row(false, false, prompt as u16);
        assert_eq!(panel(&view), rest);
    }
}

mod task_details_curtain {
    use super::*;

    fn fixture() -> View {
        let mut row = session("curtain", "Curtain task", SessionStatus::NeedsInput, 0);
        row.prompt = (1..=12)
            .map(|i| format!("Prompt row {i}"))
            .collect::<Vec<_>>()
            .join("  \n");
        row.last_message = Some(
            (1..=7)
                .map(|i| format!("Reply row {i}"))
                .collect::<Vec<_>>()
                .join("  \n"),
        );
        let mut view = test_view(Grouping::Project, true);
        view.status_filter = 0;
        view.set_sessions(vec![row], false);
        view
    }

    fn panel(view: &View) -> Buffer {
        let area = Rect::new(0, 0, 120, 19);
        let mut buf = Buffer::empty(area);
        view.render_task_details(area, &mut buf, view.rows[view.selected].updated_at);
        buf
    }

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn targets_cut_body_or_first_cut_below() {
        use crate::ui::input::curtain_target;
        let bodies = [2..6, 8..12, 14..17];
        let reveal = [0, 0, 0];
        let hidden = [4, 2, 0];
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 3, true), Some(0));
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 7, true), Some(1));
        assert_eq!(
            curtain_target(&bodies, &[4, 0, 0], &hidden, 3, true),
            Some(1)
        );
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 13, true), None);
        assert_eq!(curtain_target(&[], &[], &[], 0, true), None);
    }

    #[test]
    fn targets_opened_body_then_below_then_above() {
        use crate::ui::input::curtain_target;
        let bodies = [2..6, 8..12, 14..17];
        let reveal = [1, 0, 2];
        let hidden = [4, 2, 2];
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 3, false), Some(0));
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 9, false), Some(2));
        assert_eq!(
            curtain_target(&bodies, &reveal, &hidden, 18, false),
            Some(2)
        );
        assert_eq!(
            curtain_target(&bodies, &[1, 0, 0], &hidden, 9, false),
            Some(0)
        );
        assert_eq!(curtain_target(&bodies, &[0, 0, 0], &hidden, 3, false), None);
        assert_eq!(curtain_target(&bodies, &reveal, &hidden, 1, false), Some(0));
    }

    #[test]
    fn fully_open_message_hands_off_to_prompt() {
        let mut view = fixture();
        let before = panel(&view);
        let pointer = (0..19)
            .find(|&y| text(&before, y).trim_start().starts_with("Reply row 1"))
            .unwrap();
        let hidden = view.viewport.borrow().task_details_scroll.hidden[0];
        assert!(hidden > 0);
        for _ in 0..hidden {
            view.scroll_content_row(true, false, pointer);
            panel(&view);
        }
        let opened = panel(&view);
        let prompt = (0..19)
            .find(|&y| text(&opened, y).trim() == "Prompt")
            .unwrap();
        assert!((1..prompt).all(|y| !text(&opened, y).ends_with('…')));
        assert_eq!(
            text(&opened, prompt - 2),
            format!("{}Reply row 7", " ".repeat(54))
        );
        view.scroll_content_row(true, false, pointer);
        let after = panel(&view);
        assert_eq!(
            view.viewport.borrow().task_details_scroll.reveal,
            vec![hidden, 1, 0]
        );
        let attention = (0..19)
            .find(|&y| text(&after, y).trim() == "Needs input")
            .unwrap();
        assert_eq!(
            text(&after, attention - 2),
            format!("{}Prompt row 7…", " ".repeat(53))
        );
        let prompt_pointer = (0..19)
            .find(|&y| text(&after, y).trim_start().starts_with("Prompt row"))
            .unwrap();
        let prompt_hidden = view.viewport.borrow().task_details_scroll.hidden[1];
        for _ in 1..prompt_hidden {
            view.scroll_content_row(true, false, prompt_pointer);
            panel(&view);
        }
        let fully_open = panel(&view);
        assert!(
            (1..19).any(|y| text(&fully_open, y) == format!("{}Prompt row 12", " ".repeat(53)))
        );
        assert_eq!(
            view.viewport.borrow().task_details_scroll.reveal,
            vec![hidden, prompt_hidden, 0]
        );
        view.scroll_content_row(true, false, prompt_pointer);
        assert_eq!(panel(&view), fully_open);
    }

    #[test]
    fn queued_background_cells_stay_fixed() {
        use std::io::Write;
        let mut output = tempfile::NamedTempFile::new().unwrap();
        writeln!(output, "Background output").unwrap();
        let mut view = fixture();
        let row = &mut view.rows[view.selected];
        row.queued = vec!["Queued prompt".into()];
        for i in 0..8 {
            row.background.push(crate::model::BackgroundTask {
                description: format!("Check {i}"),
                output: Some(output.path().into()),
                started_at: row.updated_at,
            });
        }
        let before = panel(&view);
        let queued = (0..19)
            .find(|&y| text(&before, y).trim_start() == "Queued prompt")
            .unwrap();
        assert_eq!(
            view.viewport.borrow().task_details_scroll.fallback_max,
            None
        );
        let pointer = (0..19)
            .find(|&y| text(&before, y).trim_start().starts_with("Prompt row 1"))
            .unwrap();
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, pointer));
        let after = panel(&view);
        for y in queued..19 {
            for x in 0..120 {
                assert_eq!(after[(x, y)], before[(x, y)], "static tail {x},{y}");
            }
        }
        view.handle_mouse(mouse(MouseEventKind::ScrollUp, 0, pointer));
        assert_eq!(panel(&view), before);
    }

    #[test]
    fn status_rule_hyperlink_stays_fixed_when_prompt_opens() {
        let mut view = fixture();
        view.rows[view.selected].pr = Some(crate::model::PullRequest {
            number: 833,
            url: "https://example.com/pull/833".into(),
        });
        let area = Rect::new(0, 0, 240, 24);
        let draw = |view: &View| {
            let mut buf = Buffer::empty(area);
            view.render_at(area, &mut buf, view.rows[view.selected].updated_at);
            buf
        };
        let before = draw(&view);
        let (link_area, target) = view
            .viewport
            .borrow()
            .task_details_links
            .iter()
            .find(|(_, target)| {
                target
                    .to_string_lossy()
                    .contains("https://example.com/pull/833")
            })
            .cloned()
            .expect("pull request on status rule");
        let cell = &before[(link_area.x, link_area.y)];
        assert!(cell.symbol().contains("https://example.com/pull/833"));
        assert!(matches!(
            cell.diff_option,
            ratatui::buffer::CellDiffOption::ForcedWidth(_)
        ));
        let pointer = {
            let viewport = view.viewport.borrow();
            assert!(viewport.task_details_scroll.hidden[1] > 0);
            viewport.task_details_scroll.content_top
                + viewport.task_details_scroll.bodies[1].start as u16
        };
        view.scroll_content_row(true, false, pointer);
        let after = draw(&view);
        assert_eq!(view.viewport.borrow().task_details_scroll.reveal[1], 1);
        for x in link_area.x..link_area.right() {
            assert_eq!(after[(x, link_area.y)], before[(x, link_area.y)]);
        }
        assert!(
            view.viewport
                .borrow()
                .task_details_links
                .contains(&(link_area, target))
        );
        view.scroll_content_row(false, false, pointer);
        assert_eq!(draw(&view), before);
    }

    #[test]
    fn content_updates_clamp_and_body_count_changes_reset() {
        let mut view = fixture();
        let before = panel(&view);
        let pointer = (0..19)
            .find(|&y| text(&before, y).trim_start() == "Prompt row 3")
            .unwrap();
        for _ in 0..4 {
            view.scroll_content_row(true, false, pointer);
            panel(&view);
        }
        assert_eq!(
            view.viewport.borrow().task_details_scroll.reveal,
            vec![0, 4, 0]
        );
        view.rows[view.selected].prompt = (1..=8)
            .map(|i| format!("Prompt row {i}"))
            .collect::<Vec<_>>()
            .join("  \n");
        panel(&view);
        assert_eq!(
            view.viewport.borrow().task_details_scroll.reveal,
            vec![0, 2, 0]
        );
        assert_eq!(view.viewport.borrow().task_details_scroll.hidden[1], 2);
        view.rows[view.selected].last_message = None;
        panel(&view);
        assert_eq!(
            view.viewport.borrow().task_details_scroll.reveal,
            vec![0, 0]
        );
    }

    #[test]
    fn opening_message_scrolls_body_and_pins_tail_and_closes_exactly() {
        let mut view = fixture();
        view.rows[view.selected].last_message = Some(
            (1..=12)
                .map(|i| format!("Reply row {i}"))
                .collect::<Vec<_>>()
                .join("  \n"),
        );
        let before = panel(&view);
        let pointer = (0..19)
            .find(|&y| text(&before, y).trim_start() == "Reply row 3")
            .unwrap();
        let tail = (0..19)
            .find(|&y| text(&before, y).trim() == "Prompt")
            .unwrap();
        view.scroll_content_row(true, false, pointer);
        let after = panel(&view);
        for x in 0..120 {
            for y in 0..pointer {
                assert_eq!(after[(x, y)], before[(x, y + 1)], "shift {x},{y}");
            }
            for y in tail..19 {
                assert_eq!(after[(x, y)], before[(x, y)], "tail {x},{y}");
            }
        }
        assert_eq!(
            text(&after, pointer),
            format!("{}Reply row 4", " ".repeat(54))
        );
        let cut_before = (pointer..tail)
            .map(|y| text(&before, y))
            .find(|s| s.ends_with('…'))
            .unwrap();
        let cut_after = (pointer..tail)
            .map(|y| text(&after, y))
            .find(|s| s.ends_with('…'))
            .unwrap();
        assert_ne!(cut_before, cut_after);
        view.scroll_content_row(false, false, pointer);
        assert_eq!(panel(&view), before);
    }

    #[test]
    fn wheel_below_last_cut_does_nothing() {
        let mut view = fixture();
        view.rows[view.selected].queued = vec!["Queued prompt".into()];
        let before = panel(&view);
        let tail = (0..19)
            .find(|&y| text(&before, y).trim() == "Queued prompt")
            .unwrap();
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, tail));
        assert_eq!(panel(&view), before);
    }
}

#[test]
fn task_details_border_double_clicks_scroll_both_ends_in_both_layouts() {
    for width in [100, 149, 150, 200] {
        for height in [12, 30] {
            for bottom in [false, true] {
                let mut view = simple_fixture();
                let row = &mut view.rows[view.selected];
                row.prompt = (0..80).map(|i| format!("Prompt row {i}  \n")).collect();
                row.last_message = Some(row.prompt.clone());
                screen(&view, width, height);
                let task_details = view.viewport.borrow().task_details;
                assert!(!task_details.is_empty());
                let pointer = task_details.y;
                // Start between the boundaries, in both curtain and plain-offset layouts.
                assert!(
                    view.viewport
                        .borrow_mut()
                        .task_details_scroll
                        .scroll_window(true, pointer)
                );
                screen(&view, width, height);
                let before = view.viewport.borrow().task_details_scroll.position();
                let max = {
                    let viewport = view.viewport.borrow();
                    let state = &viewport.task_details_scroll;
                    state
                        .fallback_max
                        .unwrap_or_else(|| state.hidden.iter().sum())
                };
                assert!(before > 0 && before < max, "{width}x{height}");
                let click = mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    (task_details.x..task_details.right())
                        .find(|&x| {
                            let position = (
                                x,
                                if bottom {
                                    task_details.bottom()
                                } else {
                                    task_details.y - 1
                                },
                            )
                                .into();
                            let viewport = view.viewport.borrow();
                            !viewport.task_details_title.contains(position)
                                && !viewport
                                    .field_targets
                                    .iter()
                                    .any(|(rect, _)| rect.contains(position))
                                && !viewport
                                    .task_details_links
                                    .iter()
                                    .any(|(rect, _)| rect.contains(position))
                        })
                        .expect("plain border cell"),
                    if bottom {
                        task_details.bottom()
                    } else {
                        task_details.y - 1
                    },
                );
                assert_eq!(click_mouse(&mut view, click), None);
                assert_eq!(
                    view.viewport.borrow().task_details_scroll.position(),
                    before
                );
                assert_eq!(click_mouse(&mut view, click), None);
                screen(&view, width, height);
                assert_eq!(
                    view.viewport.borrow().task_details_scroll.position(),
                    if bottom { max } else { 0 },
                    "{width}x{height}, bottom {bottom}"
                );
                assert!(
                    !view
                        .viewport
                        .borrow_mut()
                        .task_details_scroll
                        .scroll_window(bottom, pointer)
                );
            }
        }
    }
}

#[test]
fn task_details_border_links_and_title_keep_actions_without_scrolling() {
    for width in [100, 149, 150, 200] {
        let mut view = simple_fixture();
        view.rows[view.selected].prompt = "Long prompt row  \n".repeat(80);
        screen(&view, width, 30);
        let pointer = view.viewport.borrow().task_details.y;
        assert!(
            view.viewport
                .borrow_mut()
                .task_details_scroll
                .scroll_window(true, pointer)
        );
        screen(&view, width, 30);
        let before = view.viewport.borrow().task_details_scroll.position();
        let links = view.viewport.borrow().task_details_links.clone();
        assert!(!links.is_empty());
        for (rect, target) in links {
            assert_eq!(rect.y, view.viewport.borrow().task_details.bottom());
            let click = mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y);
            for _ in 0..2 {
                assert_eq!(
                    click_mouse(&mut view, click),
                    Some(Action::OpenPath(target.clone()))
                );
                assert_eq!(
                    view.viewport.borrow().task_details_scroll.position(),
                    before
                );
            }
        }
        let title = view.viewport.borrow().task_details_title;
        assert!(!title.is_empty());
        let click = mouse(MouseEventKind::Down(MouseButton::Left), title.x, title.y);
        assert_eq!(click_mouse(&mut view, click), None);
        assert!(view.rename_target.is_none());
        assert_eq!(click_mouse(&mut view, click), None);
        assert_eq!(view.rename_target.as_ref(), view.selected_id());
        assert_eq!(
            view.viewport.borrow().task_details_scroll.position(),
            before
        );
    }
}
