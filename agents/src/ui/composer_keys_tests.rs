use super::*;

fn focused(text: &str, width: u16) -> PromptBox {
    let mut prompt = PromptBox::default();
    prompt.paste(text);
    prompt.height(width, "Task");
    prompt
}
fn press(prompt: &mut PromptBox, code: KeyCode, modifiers: KeyModifiers) -> Option<PromptAction> {
    prompt.handle_key(KeyEvent::new(code, modifiers))
}
#[test]
fn vertical_keys_keep_column_across_short_and_wide_lines() {
    let mut prompt = focused("abcd\nx\n你好ab", 80);
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), 6);
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), 4);
    press(&mut prompt, KeyCode::Down, KeyModifiers::NONE);
    press(&mut prompt, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), prompt.editor.text().len());
    press(&mut prompt, KeyCode::Left, KeyModifiers::NONE);
    press(&mut prompt, KeyCode::Char('p'), KeyModifiers::CONTROL);
    press(&mut prompt, KeyCode::Char('p'), KeyModifiers::CONTROL);
    assert_eq!(prompt.editor.cursor(), 4);
}
#[test]
fn vertical_keys_follow_wrapped_rows_and_graphemes() {
    let mut prompt = focused("ab你好cdefg", 13); // Task prefix leaves six cells.
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), 5);
    press(&mut prompt, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), prompt.editor.text().len());
}
#[test]
fn newline_keys_preserve_editor_and_release_is_ignored() {
    for (code, modifiers) in [
        (KeyCode::Enter, KeyModifiers::SHIFT),
        (KeyCode::Enter, KeyModifiers::ALT),
        (KeyCode::Char('j'), KeyModifiers::CONTROL),
        (KeyCode::Char('\n'), KeyModifiers::NONE),
    ] {
        let mut prompt = focused("hello", 80);
        assert_eq!(press(&mut prompt, code, modifiers), None);
        assert_eq!(prompt.editor.text(), "hello\n");
        let mut release = KeyEvent::new(code, modifiers);
        release.kind = KeyEventKind::Release;
        prompt.handle_key(release);
        assert_eq!(prompt.editor.text(), "hello\n");
    }
}

#[test]
fn history_recalls_at_visual_boundaries_and_restores_draft() {
    let mut prompt = focused("first prompt", 80);
    press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE);
    prompt.submitting = false;
    prompt.editor.set_text("second prompt");
    press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE);
    prompt.submitting = false;
    prompt.editor.set_text("draft\nlast");
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.text(), "draft\nlast");
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.text(), "second prompt");
    press(&mut prompt, KeyCode::Char('!'), KeyModifiers::NONE);
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.text(), "first prompt");
    press(&mut prompt, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(prompt.editor.text(), "second prompt");
    press(&mut prompt, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(prompt.editor.text(), "draft\nlast");
}
#[test]
fn reverse_search_accepts_without_sending_and_cancel_restores_caret() {
    let mut prompt = focused("cargo test", 80);
    press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE);
    prompt.submitting = false;
    prompt.editor.set_text("git status");
    press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE);
    prompt.submitting = false;
    prompt.editor.set_text("draft");
    press(&mut prompt, KeyCode::Left, KeyModifiers::NONE);
    let cursor = prompt.editor.cursor();
    press(&mut prompt, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert_eq!(prompt.editor.text(), "draft");
    for c in "cargo".chars() {
        press(&mut prompt, KeyCode::Char(c), KeyModifiers::NONE);
    }
    assert_eq!(prompt.editor.text(), "cargo test");
    assert_eq!(press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE), None);
    assert_eq!(prompt.editor.text(), "cargo test");
    prompt.editor.set_text("draft");
    press(&mut prompt, KeyCode::Left, KeyModifiers::NONE);
    press(&mut prompt, KeyCode::Char('r'), KeyModifiers::CONTROL);
    press(&mut prompt, KeyCode::Char('g'), KeyModifiers::NONE);
    press(&mut prompt, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(prompt.editor.text(), "draft");
    assert_eq!(prompt.editor.cursor(), cursor);
}

#[test]
fn history_is_shared_but_each_box_keeps_its_draft() {
    let mut first = focused("shared prompt", 80);
    press(&mut first, KeyCode::Enter, KeyModifiers::NONE);
    let mut second = focused("other draft", 80);
    second.history = first.history.clone();
    press(&mut second, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(second.editor.text(), "shared prompt");
    press(&mut second, KeyCode::Char('!'), KeyModifiers::NONE);
    press(&mut second, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(second.editor.text(), "other draft");
    press(&mut second, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(second.editor.text(), "shared prompt");
}
#[test]
fn history_search_steps_unique_matches_both_ways_and_handles_paste() {
    let mut prompt = focused("git diff", 80);
    for text in ["git status", "git diff", "cargo test", "git status"] {
        prompt.editor.set_text(text);
        prompt.submitting = false;
        press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE);
    }
    prompt.editor.set_text("draft");
    press(&mut prompt, KeyCode::Char('r'), KeyModifiers::CONTROL);
    prompt.paste("GIT");
    assert_eq!(prompt.editor.text(), "git status");
    press(&mut prompt, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert_eq!(prompt.editor.text(), "git diff");
    press(&mut prompt, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert_eq!(prompt.editor.text(), "git diff");
    press(&mut prompt, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(prompt.editor.text(), "git status");
    press(&mut prompt, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(prompt.editor.text(), "draft");
}
#[test]
fn vertical_stops_match_rendered_caret_after_resize_and_exact_wrap() {
    let mut prompt = focused("abcdef", 13);
    assert_eq!(prompt.layout(13, "Task").caret, (0, 1));
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), 0);
    press(&mut prompt, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), 6);
    prompt.editor.set_text("你e\u{301}好\nshort");
    prompt.height(80, "Task");
    press(&mut prompt, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(prompt.editor.cursor(), "你e\u{301}好".len());
    prompt.height(13, "Task");
    press(&mut prompt, KeyCode::Down, KeyModifiers::NONE);
    assert!(
        prompt
            .editor
            .text()
            .is_char_boundary(prompt.editor.cursor())
    );
}

#[test]
fn enhanced_control_codes_keep_clear_and_history_bindings() {
    let mut prompt = focused("cargo test", 80);
    press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE);
    prompt.submitting = false;
    prompt.editor.set_text("draft");
    press(&mut prompt, KeyCode::Char('\u{12}'), KeyModifiers::NONE);
    prompt.paste("cargo");
    assert_eq!(prompt.editor.text(), "cargo test");
    press(&mut prompt, KeyCode::Enter, KeyModifiers::NONE);
    press(&mut prompt, KeyCode::Char('\u{3}'), KeyModifiers::NONE);
    assert!(prompt.editor.is_empty());
    assert_eq!(
        press(&mut prompt, KeyCode::Char('\u{3}'), KeyModifiers::NONE),
        Some(PromptAction::Quit)
    );
}

#[test]
fn history_search_receives_slash_before_panel_search_routing() {
    let mut view = View::new(Grouping::Project, false);
    view.active_panel = Panel::Prompt;
    let mut entry = LineEdit::default();
    entry.set_text("path/to/file");
    view.prompt_history.borrow_mut().push(entry);
    view.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    view.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    assert_eq!(view.input_box.editor.text(), "path/to/file");
}
