use super::*;
use ratatui::style::Modifier;

#[test]
fn task_cursor_preserves_cell_colors_in_both_themes_and_color_modes() {
    let _guard = crate::theme::TEST_LOCK.lock().unwrap();
    for light in [false, true] {
        crate::theme::use_default_accents(light);
        for (no_color, colors) in [(false, true), (true, true), (false, false)] {
            let selection = style::selection_style_for(no_color, colors, light);
            style::with_selection_style(selection, || {
                for use_colors in [false, true] {
                    let mut view = test_view(Grouping::Project, use_colors);
                    view.set_sessions(
                        vec![
                            session("cursor", "Accent 任务", SessionStatus::Working, 120),
                            session("other", "Other task", SessionStatus::Ready, 240),
                        ],
                        false,
                    );
                    let target = view
                        .rows
                        .iter()
                        .position(|row| row.id == session_key("cursor"))
                        .unwrap();
                    let other = view
                        .rows
                        .iter()
                        .position(|row| row.id == session_key("other"))
                        .unwrap();
                    let area = Rect::new(2, 1, 80, 8);
                    let outer = Rect::new(0, 0, 84, 10);
                    let now = Utc::now();
                    view.set_selected(other);
                    let mut plain = Buffer::empty(outer);
                    view.render_center_rows(area, &mut plain, now);
                    let y = (area.y..area.bottom())
                        .find(|&y| {
                            (area.x..area.right())
                                .map(|x| plain[(x, y)].symbol())
                                .collect::<String>()
                                .contains("Accent")
                        })
                        .unwrap();
                    assert_eq!(plain[(area.x, y)].fg, Color::Green);
                    assert_eq!(
                        plain[(area.x + 2, y)].fg,
                        if use_colors {
                            view.title_color(&session_key("cursor"))
                        } else {
                            Color::Reset
                        }
                    );
                    assert!(
                        plain[(area.right() - 1, y)]
                            .modifier
                            .contains(Modifier::DIM)
                    );
                    view.set_selected(target);
                    let mut selected = Buffer::empty(outer);
                    view.render_center_rows(area, &mut selected, now);
                    for x in area.x - 1..=area.right() {
                        let before = &plain[(x, y)];
                        let after = &selected[(x, y)];
                        assert_eq!(
                            after.fg, before.fg,
                            "light={light}, no_color={no_color}, colors={colors}, use_colors={use_colors}, x={x}"
                        );
                        assert!(after.modifier.contains(Modifier::BOLD));
                        assert_eq!(after.bg, selection.bg.unwrap_or(Color::Reset));
                        assert_eq!(
                            after.modifier.contains(Modifier::DIM),
                            before.modifier.contains(Modifier::DIM)
                        );
                        assert_eq!(
                            after.modifier.contains(Modifier::REVERSED),
                            no_color || !colors
                        );
                    }
                }
            });
        }
    }
    crate::theme::use_default_accents(false);
}

#[test]
fn needs_input_status_uses_the_attention_title_style() {
    let mut view = test_view(Grouping::Project, true);
    view.set_sessions(
        vec![session(
            "waiting",
            "Waiting task",
            SessionStatus::NeedsInput,
            120,
        )],
        false,
    );
    let area = Rect::new(0, 0, 80, 4);
    let mut buffer = Buffer::empty(area);
    view.render_center_rows(area, &mut buffer, Utc::now());
    let (x, y) = (area.top()..area.bottom())
        .find_map(|y| {
            let row: String = (area.left()..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            row.find("Needs input")
                .map(|byte| (u16::try_from(row[..byte].chars().count()).unwrap(), y))
        })
        .unwrap();
    let attention = style::attention_title_style();
    assert_eq!(buffer[(x, y)].fg, attention.fg.unwrap_or(Color::Reset));
    assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
}
