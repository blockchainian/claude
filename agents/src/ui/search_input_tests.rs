use super::search_input::{SearchInput, search_row};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn search_input_characters_and_spaces_are_consumed() {
    let mut input = SearchInput::default();
    input.focus();
    for character in "a 你/".chars() {
        assert!(input.handle_key(key(KeyCode::Char(character))));
    }
    assert_eq!(input.text(), "a 你/");
    assert!(input.handle_key(key(KeyCode::Backspace)));
    assert_eq!(input.text(), "a 你");
}

#[test]
fn search_input_up_and_down_pass_through() {
    let mut input = SearchInput::default();
    input.set_text("query");
    input.focus();
    for code in [KeyCode::Up, KeyCode::Down] {
        assert!(!input.handle_key(key(code)));
        assert_eq!(input.text(), "query");
        assert!(input.focused());
    }
}

#[test]
fn search_input_enter_blurs_and_keeps_query() {
    let mut input = SearchInput::default();
    input.focus();
    input.insert("query");
    assert!(input.handle_key(key(KeyCode::Enter)));
    assert!(!input.focused());
    assert!(input.applied());
    assert_eq!(input.text(), "query");
}

#[test]
fn search_input_escape_clears_and_blurs() {
    let mut input = SearchInput::default();
    input.set_text("query");
    input.focus();
    assert!(input.handle_key(key(KeyCode::Esc)));
    assert!(!input.focused());
    assert!(!input.applied());
    assert_eq!(input.text(), "");
}

#[test]
fn search_input_unfocused_keys_pass_through() {
    let mut input = SearchInput::default();
    for text in ["", "query"] {
        input.set_text(text);
        for code in [
            KeyCode::Char('a'),
            KeyCode::Char(' '),
            KeyCode::Char('/'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Tab,
            KeyCode::F(3),
        ] {
            assert!(!input.handle_key(key(code)), "{code:?}");
            assert_eq!(input.text(), text);
            assert!(!input.focused());
        }
    }
}

#[test]
fn search_input_focus_moves_caret_to_end_and_blur_preserves_text() {
    let row = Rect::new(3, 4, 30, 1);
    let mut input = SearchInput::default();
    assert!(!input.applied());
    input.focus();
    assert!(input.applied());
    input.blur();
    assert!(!input.applied());
    input.set_text("你好a");
    input.focus();
    assert_eq!(input.caret(row), Some((17, 4)));
    assert!(input.handle_key(key(KeyCode::Home)));
    input.blur();
    assert_eq!(input.text(), "你好a");
    assert_eq!(input.caret(row), None);
    input.focus();
    input.insert("b");
    assert_eq!(input.text(), "你好ab");
    input.close();
    assert!(!input.applied());
}

#[test]
fn search_input_click_positions_by_display_column() {
    let row = Rect::new(3, 4, 30, 1);
    let mut input = SearchInput::default();
    input.set_text("你ab");
    input.click(row, row.x + 9);
    assert!(input.focused());
    assert_eq!(input.caret(row), Some((14, 4)));
    input.insert("X");
    assert_eq!(input.text(), "你Xab");
}

#[test]
fn search_input_click_preserves_the_current_horizontal_window() {
    let row = Rect::new(3, 4, 14, 1);
    let mut input = SearchInput::default();
    input.set_text("abcdefghijklmnop");
    input.focus();
    assert!(input.handle_key(key(KeyCode::Home)));
    assert!(input.handle_key(key(KeyCode::Right)));
    input.blur();
    input.click(row, row.x + 10);
    assert!(input.focused());
    input.insert("X");
    assert_eq!(input.text(), "abXcdefghijklmnop");
}

#[test]
fn search_input_render_keeps_the_input_row_after_blur() {
    let row = Rect::new(3, 4, 20, 1);
    let mut input = SearchInput::default();
    input.set_text("abc");
    input.focus();
    let mut buf = Buffer::empty(row);
    input.render(row, &mut buf);
    let text: String = (row.x..row.right())
        .map(|x| buf[(x, row.y)].symbol())
        .collect();
    assert_eq!(text, "Search ❯ abc        ");
    assert!(buf[(row.x, row.y)].modifier.contains(Modifier::BOLD));
    let expected_style = ratatui::style::Style {
        fg: super::style::accent_color(),
        ..ratatui::style::Style::default()
    };
    let mut expected = Buffer::empty(row);
    expected.set_style(row, expected_style);
    assert_eq!(buf[(row.x, row.y)].fg, expected[(row.x, row.y)].fg);
    input.blur();
    let mut buf = Buffer::empty(row);
    input.render(row, &mut buf);
    let text: String = (row.x..row.right())
        .map(|x| buf[(x, row.y)].symbol())
        .collect();
    assert_eq!(text, "Search ❯ abc        ");
    assert_eq!(input.caret(row), None);
}

#[test]
fn search_input_caret_stays_in_row_and_empty_rows_are_hidden() {
    let mut input = SearchInput::default();
    input.set_text("abcdefghijklmnop");
    input.focus();
    for width in [1, 8, 9, 12] {
        let row = Rect::new(3, 4, width, 1);
        assert_eq!(input.caret(row), Some((row.right() - 1, row.y)));
    }
    for row in [
        Rect::default(),
        Rect::new(3, 4, 20, 0),
        Rect::new(3, 4, 0, 1),
    ] {
        assert_eq!(input.caret(row), None);
        input.render(row, &mut Buffer::empty(row));
    }
}

#[test]
fn search_input_search_row_is_bottom_row_unless_panel_is_too_short() {
    for height in [0, 1] {
        assert!(search_row(Rect::new(3, 4, 20, height)).is_empty());
    }
    assert_eq!(search_row(Rect::new(3, 4, 20, 2)), Rect::new(3, 5, 20, 1));
    assert_eq!(search_row(Rect::new(3, 4, 20, 8)), Rect::new(3, 11, 20, 1));
}
