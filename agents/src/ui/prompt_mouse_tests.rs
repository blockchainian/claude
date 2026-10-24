use super::*;

fn prompt(view: &mut View, text: &str, width: u16) -> (Rect, u16) {
    view.rows[view.selected].process = None;
    view.active_panel = Panel::Prompt;
    view.input_box.editor.set_text(text);
    screen(view, width, 40);
    let area = view.viewport.borrow().prompt.inner(Margin::new(1, 0));
    view.input_box.editor.move_caret(crate::line_edit::Motion::TextStart);
    let x = view.input_box.cursor(area, &view.input_label()).unwrap().0;
    view.input_box.editor.move_caret(crate::line_edit::Motion::TextEnd);
    screen(view, width, 40);
    (area, x)
}

#[test]
fn prompt_mouse_click_places_caret_on_wide_multiline_and_wrapped_cells() {
    let mut view = fixture();
    let text = "写工具详情计划\nNo reply needed.";
    let (area, x) = prompt(&mut view, text, 120);
    click_mouse(
        &mut view,
        mouse(MouseEventKind::Down(MouseButton::Left), x + 3, area.y),
    );
    assert_eq!(view.input_box.editor.cursor(), "写".len());
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x + 3,
            area.y + 1,
        ),
    );
    assert_eq!(view.input_box.editor.cursor(), "写工具详情计划\nNo ".len());
    click_mouse(
        &mut view,
        mouse(MouseEventKind::Down(MouseButton::Left), area.x, area.y),
    );
    assert_eq!(view.input_box.editor.cursor(), 0);
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    let (area, x) = prompt(&mut view, text, 60);
    // Scroll to the beginning before checking a soft-wrapped row, which starts at the
    // box's left edge.
    view.input_box.editor.move_caret(crate::line_edit::Motion::TextStart);
    screen(&view, 60, 40);
    click_mouse(
        &mut view,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            area.x + 2,
            area.y + 1,
        ),
    );
    let columns = usize::from(area.right() - x);
    assert_eq!(view.input_box.editor.cursor(), columns + 2);
    assert!(view.prompt_focused());
}

#[test]
fn prompt_mouse_drag_highlights_copies_and_replaces_selection() {
    for replacement in [KeyCode::Char('查'), KeyCode::Backspace, KeyCode::Delete] {
        let mut view = fixture();
        let (area, x) = prompt(&mut view, "写工具详情计划", 120);
        view.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            x + 2,
            area.y,
        ));
        view.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            x + 6,
            area.y,
        ));
        let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
        view.render(buffer.area, &mut buffer);
        for column in x + 2..x + 6 {
            assert!(
                buffer[(column, area.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::REVERSED),
                "column {column}; selection {:?}",
                view.input_box.editor.selection_range()
            );
        }
        assert!(
            matches!(view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 6, area.y)), Some(Action::Copy { text, .. }) if text == "工具")
        );
        key(&mut view, replacement);
        let expected = if matches!(replacement, KeyCode::Backspace | KeyCode::Delete) {
            "写详情计划"
        } else {
            "写查详情计划"
        };
        assert_eq!(view.input_box.editor.text(), expected);
    }
}

#[test]
fn prompt_mouse_reverse_multiline_selection_survives_release_and_paste_replaces_it() {
    let mut view = fixture();
    let (area, x) = prompt(&mut view, "写工具详情计划\nNo reply needed.", 120);
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x + 3,
        area.y + 1,
    ));
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        x + 2,
        area.y,
    ));
    assert!(
        matches!(view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 2, area.y)), Some(Action::Copy { text, .. }) if text == "工具详情计划\nNo ")
    );
    let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 40));
    view.render(buffer.area, &mut buffer);
    assert!(
        buffer[(x + 2, area.y)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
    view.handle_paste("查");
    assert_eq!(view.input_box.editor.text(), "写查reply needed.");
    assert!(view.input_box.editor.selection_range().is_none());
}

#[test]
fn prompt_mouse_wheel_scrolls_without_moving_caret_or_list() {
    let mut view = fixture();
    let text = include_str!("../../tests/fixtures/claude-peer-message.txt");
    let (area, x) = prompt(&mut view, text, 60);
    let caret = view.input_box.editor.cursor();
    let selected = view.selected;
    let before = screen(&view, 60, 40);
    view.handle_mouse(mouse(MouseEventKind::ScrollUp, x, area.y));
    let after = screen(&view, 60, 40);
    assert_ne!(before, after);
    assert_eq!(view.input_box.editor.cursor(), caret);
    assert_eq!(view.selected, selected);
    view.handle_mouse(mouse(MouseEventKind::ScrollDown, x, area.y));
    assert_eq!(screen(&view, 60, 40), before);
}

#[test]
fn prompt_mouse_replacement_happens_before_graphemes_join() {
    let mut view = fixture();
    let (area, x) = prompt(&mut view, "👩\u{200d}a👧", 120);
    view.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        x + 2,
        area.y,
    ));
    view.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        x + 3,
        area.y,
    ));
    view.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 3, area.y));
    assert_eq!(view.input_box.editor.selected_text(), Some("a"));
    key(&mut view, KeyCode::Char('x'));
    assert_eq!(view.input_box.editor.text(), "👩\u{200d}x👧");
}
