use super::*;

mod details_budget {
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
                output: output.path().into(),
                started_at: row.updated_at,
            });
        }
        let mut view = test_view(Grouping::Project, true);
        view.status_filter = 0;
        view.set_sessions(vec![row], false);
        let rows = panel(&view);
        assert!(rows.iter().any(|line| line == "Queued work"));
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
            ["Prompt text…", "", "Queued work"]
        );
        let attention = rows
            .iter()
            .position(|line| line.trim() == "Needs attention…")
            .unwrap();
        assert!(background < prompt && prompt < attention);
        assert_eq!(attention, rows.len() - 1);
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
        view.render_details(area, &mut buf, Utc::now());
        let row_text = |y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>();
        assert_eq!(row_text(0).trim(), "Prompt");
        assert_eq!(row_text(1).trim(), "First row");
        assert_eq!(buf[(0, 2)].symbol(), "…");
    }

    fn panel(view: &View) -> Vec<String> {
        let area = Rect::new(0, 0, 120, 19);
        let mut buf = Buffer::empty(area);
        view.render_details(area, &mut buf, Utc::now());
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
        for header in ["Prompt", "Update", "Needs attention"] {
            assert!(
                rest.iter().any(|line| line.trim() == header),
                "{header}: {rest:?}"
            );
        }
        assert_eq!(rest[18], "Waiting for your response.");
        let prompt = rest
            .iter()
            .position(|line| line.trim() == "Prompt")
            .unwrap();
        let update = rest
            .iter()
            .position(|line| line.trim() == "Update")
            .unwrap();
        assert!(
            rest[prompt + 1..update]
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
                .details_scroll
                .reveal
                .iter()
                .sum::<usize>(),
            1
        );
        let scrolled = panel(&view);
        assert_eq!(scrolled[0], rest[1]);
        assert_eq!(scrolled[18], rest[18]);
        assert_eq!(scrolled[prompt], "Prompt row 1");
        view.scroll_content_row(false, false, prompt as u16);
        assert_eq!(panel(&view), rest);
    }
}

mod details_curtain {
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
        view.render_details(area, &mut buf, view.rows[view.selected].updated_at);
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
    fn fully_open_prompt_hands_off_to_message() {
        let mut view = fixture();
        let before = panel(&view);
        let pointer = (0..19)
            .find(|&y| text(&before, y).starts_with("Prompt row 1"))
            .unwrap();
        let hidden = view.viewport.borrow().details_scroll.hidden[0];
        assert!(hidden > 0);
        for _ in 0..hidden {
            view.scroll_content_row(true, false, pointer);
            panel(&view);
        }
        let opened = panel(&view);
        let update = (0..19)
            .find(|&y| text(&opened, y).trim() == "Update")
            .unwrap();
        assert!((1..update).all(|y| !text(&opened, y).ends_with('…')));
        assert_eq!(text(&opened, update - 2), "Prompt row 12");
        view.scroll_content_row(true, false, pointer);
        let after = panel(&view);
        assert_eq!(
            view.viewport.borrow().details_scroll.reveal,
            vec![hidden, 1, 0]
        );
        assert_eq!(text(&after, 18), text(&opened, 18));
        let message_pointer = (0..19)
            .find(|&y| text(&after, y).starts_with("Reply row"))
            .unwrap();
        let message_hidden = view.viewport.borrow().details_scroll.hidden[1];
        for _ in 1..message_hidden {
            view.scroll_content_row(true, false, message_pointer);
            panel(&view);
        }
        let fully_open = panel(&view);
        assert!((1..19).any(|y| text(&fully_open, y) == "Reply row 7"));
        assert_eq!(
            view.viewport.borrow().details_scroll.reveal,
            vec![hidden, message_hidden, 0]
        );
        view.scroll_content_row(true, false, message_pointer);
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
                output: output.path().into(),
                started_at: row.updated_at,
            });
        }
        let before = panel(&view);
        let queued = (0..19)
            .find(|&y| text(&before, y) == "Queued prompt")
            .unwrap();
        assert_eq!(view.viewport.borrow().details_scroll.fallback_max, None);
        let pointer = (0..19)
            .find(|&y| text(&before, y).starts_with("Prompt row 1"))
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
            .details_links
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
            assert!(viewport.details_scroll.hidden[0] > 0);
            viewport.details_scroll.content_top + viewport.details_scroll.bodies[0].start as u16
        };
        view.scroll_content_row(true, false, pointer);
        let after = draw(&view);
        assert_eq!(view.viewport.borrow().details_scroll.reveal[0], 1);
        for x in link_area.x..link_area.right() {
            assert_eq!(after[(x, link_area.y)], before[(x, link_area.y)]);
        }
        assert!(
            view.viewport
                .borrow()
                .details_links
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
            .find(|&y| text(&before, y) == "Prompt row 3")
            .unwrap();
        for _ in 0..4 {
            view.scroll_content_row(true, false, pointer);
            panel(&view);
        }
        assert_eq!(view.viewport.borrow().details_scroll.reveal, vec![4, 0, 0]);
        view.rows[view.selected].prompt = (1..=8)
            .map(|i| format!("Prompt row {i}"))
            .collect::<Vec<_>>()
            .join("  \n");
        panel(&view);
        assert_eq!(view.viewport.borrow().details_scroll.reveal, vec![2, 0, 0]);
        assert_eq!(view.viewport.borrow().details_scroll.hidden[0], 2);
        view.rows[view.selected].last_message = None;
        panel(&view);
        assert_eq!(view.viewport.borrow().details_scroll.reveal, vec![0, 0]);
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
            .find(|&y| text(&before, y) == "Reply row 3")
            .unwrap();
        let tail = (0..19)
            .find(|&y| text(&before, y).trim() == "Needs attention")
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
        assert_eq!(text(&after, pointer), "Reply row 4");
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
            .find(|&y| text(&before, y).trim() == "Needs attention")
            .unwrap();
        view.handle_mouse(mouse(MouseEventKind::ScrollDown, 0, tail));
        assert_eq!(panel(&view), before);
    }
}
