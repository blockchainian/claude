use super::*;
use crate::images::ImageAttachment;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn image() -> ImageAttachment {
    ImageAttachment::from_image(image::load_from_memory(&crate::images::fixture_png()).unwrap())
        .unwrap()
}
fn key(editor: &mut LineEdit, code: KeyCode, modifiers: KeyModifiers) {
    assert!(editor.handle(KeyEvent::new(code, modifiers)));
}

#[test]
fn image_placeholders_move_and_delete_as_one_element() {
    for (movement, deletion) in [
        (KeyCode::Home, KeyCode::Delete),
        (KeyCode::End, KeyCode::Backspace),
    ] {
        let mut editor = LineEdit::default();
        let image = image();
        let path = image.path().to_owned();
        editor.insert_image(image);
        assert_eq!(editor.text(), "[Image #1]");
        key(&mut editor, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(editor.cursor(), 0);
        key(&mut editor, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(editor.cursor(), "[Image #1]".len());
        key(&mut editor, movement, KeyModifiers::NONE);
        key(&mut editor, deletion, KeyModifiers::NONE);
        assert!(editor.is_empty());
        assert!(editor.image_paths().is_empty());
        assert!(!path.exists());
    }
}

#[test]
fn edits_keep_image_ranges_and_ordinary_placeholder_text_distinct() {
    let mut editor = LineEdit::default();
    editor.insert("你好 ");
    editor.insert_image(image());
    editor.insert(" text [Image #1]");
    assert_eq!(editor.prompt_text(), "你好  text [Image #1]");
    key(&mut editor, KeyCode::Home, KeyModifiers::NONE);
    editor.insert("prefix ");
    assert_eq!(editor.prompt_text(), "prefix 你好  text [Image #1]");
    assert_eq!(editor.image_paths().len(), 1);
    key(&mut editor, KeyCode::End, KeyModifiers::NONE);
    editor.insert_image(image());
    assert!(editor.text().ends_with("[Image #2]"));
    key(&mut editor, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert_eq!(editor.image_paths().len(), 1);
    assert!(!editor.text().contains("[Image #2]"));
}

#[test]
fn copied_draft_keeps_png_until_last_reference_is_removed() {
    let mut editor = LineEdit::default();
    editor.insert_image(image());
    let path = editor.image_paths().remove(0);
    let draft = editor.clone();
    editor.clear();
    assert!(path.is_file());
    drop(draft);
    assert!(!path.exists());
}

#[test]
fn mouse_selection_expands_to_whole_image_and_typing_removes_it() {
    let mut editor = LineEdit::default();
    editor.insert("before ");
    editor.insert_image(image());
    editor.insert(" after");
    editor.select("before [I".len(), "before [Image #".len());
    assert_eq!(editor.selected_text(), Some("[Image #1]"));
    editor.insert("replacement");
    assert_eq!(editor.text(), "before replacement after");
    assert!(editor.image_paths().is_empty());
}

#[test]
fn wrapped_vertical_movement_never_places_the_caret_inside_an_image() {
    let mut editor = LineEdit::default();
    editor.insert("a\n");
    editor.insert_image(image());
    editor.insert("\nz");
    for down in [false, false, true, true] {
        editor.move_vertical(6, 0, down);
        assert!(!(3..12).contains(&editor.cursor()), "{}", editor.cursor());
    }
}

#[test]
fn failed_image_send_restores_attachments_beside_the_next_draft() {
    let mut prompt = PromptBox::default();
    prompt.editor.insert("first ");
    prompt.editor.insert_image(image());
    let first = prompt.editor.image_paths()[0].clone();
    prompt.begin_send("first ");
    assert!(prompt.editor.is_empty());
    assert!(first.is_file());
    prompt.editor.insert("next ");
    prompt.editor.insert_image(image());
    let next = prompt.editor.image_paths()[0].clone();
    prompt.finish_send(false);
    assert_eq!(prompt.editor.prompt_text(), "first \nnext ");
    assert_eq!(prompt.editor.image_paths(), vec![first, next]);
    key(&mut prompt.editor, KeyCode::Home, KeyModifiers::NONE);
    for _ in 0..2 {
        key(
            &mut prompt.editor,
            KeyCode::Char('k'),
            KeyModifiers::CONTROL,
        );
    }
    assert!(prompt.editor.image_paths().is_empty());
}

#[test]
fn recalled_history_keeps_the_image_attachment() {
    let mut prompt = PromptBox::default();
    prompt.editor.insert("describe ");
    prompt.editor.insert_image(image());
    let path = prompt.editor.image_paths()[0].clone();
    prompt.record_submission();
    prompt.editor.clear();
    prompt.recall_history(false);
    assert_eq!(prompt.editor.image_paths(), vec![path]);
    assert_eq!(prompt.editor.prompt_text(), "describe ");
}

#[test]
fn image_only_submit_carries_an_attachment_without_placeholder_text() {
    let mut prompt = PromptBox::default();
    prompt.editor.insert_image(image());
    assert_eq!(
        prompt.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
        Some(PromptAction::PasteImage)
    );
    assert_eq!(
        prompt.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(PromptAction::Submit(String::new()))
    );
    assert_eq!(prompt.editor.image_paths().len(), 1);
}

#[test]
fn history_search_keeps_distinct_images_with_the_same_caption() {
    let mut prompt = PromptBox::default();
    let mut paths = Vec::new();
    for _ in 0..2 {
        prompt.editor = LineEdit::default();
        prompt.editor.insert("describe ");
        prompt.editor.insert_image(image());
        paths.push(prompt.editor.image_paths()[0].clone());
        prompt.record_submission();
    }
    prompt.editor.clear();
    prompt.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    for path in paths.iter().rev() {
        prompt.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        assert_eq!(prompt.editor.image_paths(), vec![path.clone()]);
    }
}

#[test]
fn image_caret_stays_on_grapheme_boundaries_with_combining_text() {
    let mut editor = LineEdit::default();
    editor.insert_image(image());
    editor.insert("\u{301}");
    editor.home();
    editor.right();
    assert_eq!(editor.cursor(), editor.text().len());
    assert!(
        editor
            .visual_positions(80, 0)
            .iter()
            .any(|p| p.0 == editor.cursor())
    );
    editor.backspace();
    assert!(editor.is_empty());
    assert!(editor.image_paths().is_empty());
}

#[test]
fn remaining_images_keep_contiguous_numbers_after_deletion() {
    let mut editor = LineEdit::default();
    editor.insert_image(image());
    editor.insert_image(image());
    let second = editor.image_paths()[1].clone();
    editor.home();
    editor.delete();
    assert_eq!(editor.text(), "[Image #1]");
    assert_eq!(editor.image_paths(), vec![second]);
    editor.insert_image(image());
    assert_eq!(editor.text(), "[Image #1][Image #2]");
}

#[test]
fn renumbering_across_ten_images_preserves_caret_and_attachment_order() {
    let mut editor = LineEdit::default();
    for _ in 0..11 {
        editor.insert_image(image());
    }
    let paths = editor.image_paths()[1..].to_vec();
    editor.home();
    editor.delete();
    editor.end();
    editor.insert(" suffix");
    assert_eq!(editor.prompt_text(), " suffix");
    assert_eq!(editor.image_paths(), paths);
    assert_eq!(editor.cursor(), editor.text().len());
    assert!(editor.text().ends_with("[Image #10] suffix"));
    editor.home();
    for _ in 0..10 {
        editor.delete();
    }
    assert_eq!(editor.text(), " suffix");
    assert!(editor.image_paths().is_empty());
}

#[test]
fn failed_image_send_allows_deleted_attachments_to_be_cleaned_up() {
    let mut prompt = PromptBox::default();
    prompt.editor.insert_image(image());
    let path = prompt.editor.image_paths()[0].clone();
    prompt.begin_send("");
    prompt.finish_send(false);
    prompt.editor.clear();
    assert!(!path.exists());
}

#[test]
fn successful_image_send_keeps_the_png_after_acknowledgement() {
    let mut prompt = PromptBox::default();
    prompt.editor.insert_image(image());
    let path = prompt.editor.image_paths()[0].clone();
    prompt.begin_send("");
    prompt.finish_send(true);
    assert!(prompt.editor.is_empty());
    assert!(path.is_file());
    std::fs::remove_file(path).unwrap();
}
