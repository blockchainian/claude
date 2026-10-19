use super::*;
use crate::providers::Provider;

fn recorded_view() -> (tempfile::TempDir, View) {
    let directory = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(directory.path().join("state_5.sqlite")).unwrap();
    db.execute_batch(include_str!("../../tests/fixtures/codex-state.sql"))
        .unwrap();
    db.execute("DELETE FROM threads WHERE id NOT IN ('thread-0', 'thread-1', 'thread-2', 'thread-3', 'thread-4', 'thread-5')", []).unwrap();
    for index in 0..6 {
        std::fs::create_dir(directory.path().join(format!("project-{index}"))).unwrap();
        db.execute(
            "UPDATE threads SET cwd=?1 WHERE id=?2",
            rusqlite::params![
                directory
                    .path()
                    .join(format!("project-{index}"))
                    .to_str()
                    .unwrap(),
                format!("thread-{index}")
            ],
        )
        .unwrap();
    }
    let mut provider = crate::providers::codex::CodexProvider::new(
        directory.path().into(),
        directory.path().join("manager"),
    );
    let mut view = test_view(Grouping::Project, false);
    view.status_filter = 1;
    view.set_sessions(provider.refresh(100).sessions, false);
    let selected = view.visible_indices()[0];
    view.set_selected(selected);
    view.rows[selected].last_message = None;
    view.rows[selected].activity = None;
    // Keep the recorded task, with enough linked prompt rows to reach selector whitespace.
    view.rows[selected].prompt = std::iter::repeat_n("[Overlay](https://example.com/overlay)", 30)
        .collect::<Vec<_>>()
        .join("\n\n");
    (directory, view)
}

fn draw(view: &View, buffer: &mut Buffer) {
    view.render(buffer.area, buffer);
}

fn assert_covered(
    view: &mut View,
    buffer: &Buffer,
    cover: Rect,
    before: &Buffer,
    check_underlines: bool,
) {
    let links = before
        .content
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| {
            let x = index as u16 % before.area.width;
            let y = index as u16 / before.area.width;
            (cover.contains((x, y).into()) && cell.symbol().contains("\x1b[4:4m")).then_some((x, y))
        })
        .collect::<Vec<_>>();
    assert!(
        !links.is_empty(),
        "a recorded link must be covered by {cover:?}"
    );
    if check_underlines {
        for y in cover.y..cover.bottom() {
            for x in cover.x..cover.right() {
                assert!(
                    !buffer[(x, y)].symbol().contains("\x1b[4:4m"),
                    "underline at {x},{y}"
                );
            }
        }
    }
    let mut blank_clicks = 0;
    for (x, y) in links {
        if view
            .viewport
            .borrow()
            .picker_rows
            .iter()
            .any(|(r, _)| r.contains((x, y).into()))
            || view
                .viewport
                .borrow()
                .confirm_options
                .iter()
                .any(|(r, _)| r.contains((x, y).into()))
        {
            continue;
        }
        blank_clicks += 1;
        for _ in 0..2 {
            assert_eq!(
                click_mouse(view, mouse(MouseEventKind::Down(MouseButton::Left), x, y)),
                None
            );
            assert_eq!(
                view.flush_pending_click(Instant::now() + std::time::Duration::from_secs(1)),
                None
            );
            assert!(view.pending_click.is_none());
            assert!(view.text_selection.borrow().is_none());
        }
    }
    assert!(
        blank_clicks > 0,
        "covered links must include overlay whitespace"
    );
}

fn picker(kind: &str, check_underlines: bool) {
    for width in [120, 200] {
        use super::super::draft_controls::{ControlTarget, SelectorKind};
        let (_directory, mut view) = recorded_view();
        let area = Rect::new(0, 0, width, 60);
        let mut before = Buffer::empty(area);
        draw(&view, &mut before);
        let target = ControlTarget::Session(view.selected_id().unwrap().clone());
        let choice = match kind {
            "model" => SelectorKind::Model {
                target,
                values: vec!["recorded".into()],
                labels: vec!["Recorded model".into()],
            },
            "effort" => SelectorKind::Effort {
                target,
                values: vec!["high".into()],
                labels: vec!["high".into()],
            },
            "repository" => SelectorKind::Repository {
                draft: crate::model::DraftId {
                    millis: 0,
                    counter: 0,
                },
                dir: _directory.path().into(),
                entries: crate::repository::browse_entries(_directory.path()),
            },
            "branch" => SelectorKind::Branch {
                draft: crate::model::DraftId {
                    millis: 0,
                    counter: 0,
                },
                branches: vec!["main".into()],
                editor: super::super::line_edit::LineEdit::default(),
                editing: false,
            },
            _ => unreachable!(),
        };
        view.selector = Some(Selector::new(choice, 0));
        let mut after = Buffer::empty(area);
        draw(&view, &mut after);
        let cover = view.viewport.borrow().details;
        assert_covered(&mut view, &after, cover, &before, check_underlines);
    }
}

#[test]
fn overlay_model_covers_links_and_clicks() {
    picker("model", true);
}
#[test]
fn overlay_effort_covers_links_and_clicks() {
    picker("effort", true);
}
#[test]
fn overlay_branch_covers_links_and_clicks() {
    picker("branch", true);
}
#[test]
fn overlay_repository_covers_links_and_clicks() {
    picker("repository", true);
}

#[test]
fn overlay_help_clears_previous_links_and_clicks() {
    let (_directory, mut view) = recorded_view();
    let area = Rect::new(0, 0, 120, 60);
    let mut before = Buffer::empty(area);
    draw(&view, &mut before);
    view.help = true;
    let mut after = Buffer::empty(area);
    draw(&view, &mut after);
    assert!(view.viewport.borrow().list_links.is_empty());
    let layout = view.center_layout(area);
    let cover = Rect::new(
        1,
        layout.header.bottom(),
        area.width - 2,
        layout.footer.y - layout.header.bottom(),
    );
    assert_covered(&mut view, &after, cover, &before, true);
}

#[test]
fn overlay_confirmation_covers_links_and_clicks() {
    let (_directory, mut view) = recorded_view();
    let area = Rect::new(0, 0, 120, 60);
    let mut before = Buffer::empty(area);
    draw(&view, &mut before);
    view.confirm_dialog = Some(ConfirmDialog {
        action: ConfirmAction::Delete(view.selected_id().unwrap().clone()),
        title: "Delete recorded task?".into(),
        confirm: false,
    });
    let mut after = Buffer::empty(area);
    draw(&view, &mut after);
    let find = |symbol: char| {
        let index = after
            .content
            .iter()
            .position(|c| c.symbol().contains(symbol))
            .unwrap() as u16;
        (index % area.width, index / area.width)
    };
    let (x, y) = find('┌');
    let (right, bottom) = find('┘');
    assert_covered(
        &mut view,
        &after,
        Rect::new(x, y, right - x + 1, bottom - y + 1),
        &before,
        true,
    );
}

#[test]
fn overlay_notice_covers_links_and_clicks() {
    let (_directory, mut view) = recorded_view();
    let area = Rect::new(0, 0, 120, 18);
    let mut before = Buffer::empty(area);
    draw(&view, &mut before);
    let list = view.center_layout(area).list;
    view.set_notice(Some("Notice".into()));
    let mut after = Buffer::empty(area);
    draw(&view, &mut after);
    let cover = Rect::new(list.x, list.bottom() - 1, list.width, 1);
    for x in cover.x..cover.right() {
        assert!(!after[(x, cover.y)].symbol().contains("\x1b[4:4m"));
        assert_eq!(
            click_mouse(
                &mut view,
                mouse(MouseEventKind::Down(MouseButton::Left), x, cover.y)
            ),
            None
        );
        assert!(view.text_selection.borrow().is_none());
    }
}

#[test]
fn overlay_picker_blank_click_does_not_open_or_copy() {
    picker("model", false);
}

#[test]
fn overlay_confirmation_emits_left_border_after_wide_text() {
    use ratatui::{backend::Backend, prelude::CrosstermBackend};
    let (_directory, view) = recorded_view();
    for area in [Rect::new(0, 0, 120, 30), Rect::new(5, 3, 120, 30)] {
        let dialog = ConfirmDialog {
            action: ConfirmAction::Delete(view.selected_id().unwrap().clone()),
            title: "Recorded task".into(),
            confirm: false,
        };
        let mut probe = Buffer::empty(area);
        view.render_confirm_dialog(&dialog, area, &mut probe);
        let cover = view.viewport.borrow().overlays.last().unwrap().area;
        for (text, linked) in [("中", false), ("a", false), ("中", true), ("a", true)] {
            let mut before = Buffer::empty(area);
            for y in cover.y..cover.bottom() {
                before.set_string(cover.x - 1, y, text, Style::default());
                if linked {
                    super::super::links::underline(
                        &mut before,
                        &[(
                            Rect::new(cover.x - 1, y, text.width() as u16, 1),
                            _directory.path().into(),
                        )],
                    );
                }
            }
            let mut after = before.clone();
            view.render_confirm_dialog(&dialog, area, &mut after);
            let updates = before.diff(&after);
            for y in cover.y..cover.bottom() {
                let border = if y == cover.y {
                    "┌"
                } else if y + 1 == cover.bottom() {
                    "└"
                } else {
                    "│"
                };
                assert!(
                    updates.iter().any(|(x, row, cell)| *x == cover.x
                        && *row == y
                        && cell.symbol() == border),
                    "{border} at {},{y} was skipped after {text}",
                    cover.x
                );
            }
            let mut bytes = Vec::new();
            CrosstermBackend::new(&mut bytes)
                .draw(updates.into_iter())
                .unwrap();
            let mut terminal = vt100::Parser::new(area.bottom(), area.right(), 0);
            terminal.process(&bytes);
            assert_eq!(
                terminal.screen().cell(cover.y, cover.x).unwrap().contents(),
                "┌"
            );
            for y in cover.y..cover.bottom() {
                if text == "中" {
                    assert_eq!(after[(cover.x - 1, y)], ratatui::buffer::Cell::default());
                } else {
                    assert_eq!(after[(cover.x - 1, y)], before[(cover.x - 1, y)]);
                }
            }
        }
    }
}

#[test]
fn overlay_confirmation_clears_cut_right_half_and_keeps_whole_neighbor() {
    let (_directory, view) = recorded_view();
    let area = Rect::new(0, 0, 120, 30);
    let dialog = ConfirmDialog {
        action: ConfirmAction::Delete(view.selected_id().unwrap().clone()),
        title: "Recorded task".into(),
        confirm: false,
    };
    let mut probe = Buffer::empty(area);
    view.render_confirm_dialog(&dialog, area, &mut probe);
    let cover = view.viewport.borrow().overlays.last().unwrap().area;
    for cut in [true, false] {
        let mut before = Buffer::empty(area);
        for y in cover.y..cover.bottom() {
            before.set_string(cover.right() - u16::from(cut), y, "中", Style::default());
            // The panel background also covers a wide character's trailing cell.
            before[(cover.right(), y)].set_bg(ratatui::style::Color::Red);
        }
        let mut after = before.clone();
        view.render_confirm_dialog(&dialog, area, &mut after);
        for y in cover.y..cover.bottom() {
            if cut {
                assert_eq!(
                    after[(cover.right(), y)],
                    ratatui::buffer::Cell::default(),
                    "cut right half at {y}"
                );
            } else {
                assert_eq!(
                    after[(cover.right(), y)],
                    before[(cover.right(), y)],
                    "whole neighbor at {y}"
                );
            }
        }
    }
}
