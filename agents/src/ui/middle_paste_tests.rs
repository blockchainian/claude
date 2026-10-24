use super::*;

fn paste_at(view: &mut View, x: u16, y: u16) {
    let action = view.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Middle), x, y));
    assert_eq!(
        action,
        Some(Action::PasteText),
        "at {x},{y}; title {:?}; search {} log {} detail {}",
        view.viewport.borrow().title_input,
        view.search.focused(),
        view.event_list.search.input.focused(),
        view.event_detail.search.input.focused()
    );
    view.handle_paste("贴🦀");
}

#[test]
fn middle_paste_prompt_hits_display_cells_and_line_ends() {
    for (text, x, y, expected, cursor) in [
        ("", 0, 0, "贴🦀", "贴🦀".len()),
        ("", 80, 0, "贴🦀", "贴🦀".len()),
        ("剪贴板", 80, 0, "剪贴板贴🦀", "剪贴板贴🦀".len()),
        ("剪贴板后", 4, 0, "剪贴板贴🦀后", "剪贴板贴🦀".len()),
        ("剪贴板后", 5, 0, "剪贴板贴🦀后", "剪贴板贴🦀".len()),
        ("🦀后", 1, 0, "🦀贴🦀后", "🦀贴🦀".len()),
        (
            "第一行\n第二行",
            80,
            0,
            "第一行贴🦀\n第二行",
            "第一行贴🦀".len(),
        ),
        (
            "第一行\n第二行",
            80,
            1,
            "第一行\n第二行贴🦀",
            "第一行\n第二行贴🦀".len(),
        ),
    ] {
        let mut view = fixture();
        view.input_box.editor.set_text(text);
        view.input_box.editor.move_caret(crate::line_edit::Motion::TextStart);
        screen(&view, 160, 40);
        let prompt = view.viewport.borrow().prompt;
        // The first line has the task label; later lines start at the padding.
        let prefix = format!("{} {PROMPT_MARKER} ", view.input_label()).width() as u16;
        view.active_panel = Panel::EventList;
        paste_at(
            &mut view,
            prompt.x + 1 + if y == 0 { prefix } else { 0 } + x,
            prompt.y + y,
        );
        assert!(view.prompt_focused());
        assert!(view.active_panel == Panel::Prompt);
        assert_eq!(view.input_box.editor.text(), expected);
        assert_eq!(view.input_box.editor.cursor(), cursor);
    }
}

#[test]
fn middle_paste_non_inputs_do_nothing() {
    let mut view = fixture();
    view.prepare_prompt();
    view.input_box.editor.set_text("kept");
    screen(&view, 160, 40);
    let viewport = view.viewport.borrow();
    let areas = [
        viewport.list,
        viewport.event_panel,
        viewport.event_detail,
        viewport.task_details,
    ];
    drop(viewport);
    for area in areas {
        assert_eq!(
            view.handle_mouse(mouse(
                MouseEventKind::Down(MouseButton::Middle),
                area.x,
                area.y
            )),
            None
        );
        assert_eq!(view.input_box.editor.text(), "kept");
    }
}

#[test]
fn middle_paste_title_and_search_use_the_clicked_editor() {
    let mut view = fixture();
    view.rename_target = view.selected_id().cloned();
    view.rename.set_text("剪贴板后");
    screen(&view, 160, 40);
    let title = view.viewport.borrow().title_input;
    paste_at(&mut view, title.x + 5, title.y);
    assert_eq!(view.rename.text(), "剪贴板贴🦀后");
    assert_eq!(view.rename.cursor(), "剪贴板贴🦀".len());

    for panel in [Panel::TaskList, Panel::EventList, Panel::EventDetail] {
        let mut view = fixture();
        view.active_panel = panel;
        if view.active_panel == Panel::Prompt {
            view.active_panel = Panel::TaskList;
        }
        view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        screen(&view, 160, 40);
        let layout = view.center_layout(Rect::new(0, 0, 160, 40));
        let area = match panel {
            Panel::TaskList => layout.search,
            Panel::EventList => super::super::search_input::search_row(layout.event_panel),
            _ => {
                super::super::search_input::search_row(layout.event_detail.inner(Margin::new(1, 0)))
            }
        };
        paste_at(&mut view, area.right() - 1, area.y);
        if panel == Panel::TaskList {
            assert_eq!(view.search.text(), "贴🦀");
            assert_eq!(
                view.search.caret(area),
                Some((
                    (area.x + "Search ❯ 贴🦀".width() as u16).min(area.right() - 1),
                    area.y
                ))
            );
        } else {
            let input = match panel {
                Panel::EventList => &view.event_list.search.input,
                _ => &view.event_detail.search.input,
            };
            assert_eq!(input.text(), "贴🦀");
            let mut editor: crate::line_edit::LineEdit = super::super::line_edit::LineEdit::default();
            editor.set_text("贴🦀");
            let caret = editor.window(usize::from(area.width.saturating_sub(9))).caret_column;
            assert_eq!(input.caret(area), Some((area.x + 9 + caret, area.y)));
        }
    }
}

#[test]
fn middle_paste_branch_moves_focus_from_prompt() {
    let mut view = fixture();
    let mut editor = crate::ui::line_edit::LineEdit::default();
    editor.set_text("剪贴板后");
    view.selector = Some(Selector::new(
        draft_controls::SelectorKind::Branch {
            draft: crate::model::DraftKey {
                millis: 1,
                counter: 0,
            },
            branches: vec!["main".into()],
            editor,
            editing: true,
        },
        0,
    ));
    view.active_panel = Panel::Prompt;
    screen(&view, 160, 40);
    let area = view.viewport.borrow().task_details_input;
    paste_at(
        &mut view,
        area.x + "New branch ❯ ".width() as u16 + 5,
        area.y,
    );
    assert!(view.active_panel == Panel::TaskDetails);
    assert!(!view.prompt_focused());
    let editor = view.selector.as_ref().unwrap().branch_editor().unwrap();
    assert_eq!(editor.text(), "剪贴板贴🦀后");
    assert_eq!(editor.cursor(), "剪贴板贴🦀".len());
}

#[test]
fn middle_paste_wrapped_rows_and_blank_rows_keep_line_ends() {
    for (column, row, expected) in [
        (80, 0, "abcd贴🦀ef\n尾"),
        (0, 1, "abcde贴🦀f\n尾"),
        (80, 1, "abcdef贴🦀\n尾"),
        (80, 8, "abcdef\n尾贴🦀"),
    ] {
        let mut prompt = prompt_box::PromptBox::default();
        prompt.editor.set_text("abcdef\n尾");

        let area = Rect::new(0, 0, 8, 10);
        // Label "x ❯ " leaves four cells on the first visual row.
        prompt.paste_mouse(
            area,
            "x",
            mouse(MouseEventKind::Down(MouseButton::Middle), column, row),
        );
        prompt.paste("贴🦀");
        assert_eq!(prompt.editor.text(), expected);
    }
}

#[test]
fn middle_paste_narrow_input_uses_content_end() {
    let mut editor: crate::line_edit::LineEdit = crate::ui::line_edit::LineEdit::default();
    editor.set_text("剪贴板");
    editor.move_caret(crate::line_edit::Motion::TextStart);
    render::click_input_row(&mut editor, Rect::new(0, 0, 2, 1), "Search ❯ ", 1);
    editor.insert("贴🦀");
    assert_eq!(editor.text(), "剪贴板贴🦀");
    assert_eq!(editor.cursor(), "剪贴板贴🦀".len());
}
