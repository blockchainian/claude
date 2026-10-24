use super::*;
use std::ops::Range;

impl<P: Placeholder> LineEdit<P> {
    fn assert_boundary(&self, offset: usize) {
        assert!(
            offset == self.text.len()
                || self
                    .text
                    .grapheme_indices(true)
                    .any(|(byte, _)| byte == offset),
            "caret must be a grapheme boundary"
        );
    }

    pub(crate) fn set_cursor(&mut self, offset: usize) {
        self.assert_boundary(offset);
        self.home();
        self.cursor = self.placeholder_caret(offset);
        self.clear_selection();
    }

    pub(crate) fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub(crate) fn select(&mut self, anchor: usize, end: usize) {
        self.assert_boundary(anchor);
        self.set_cursor(end);
        self.selection = Some((anchor, end));
    }

    pub(crate) fn selection_range(&self) -> Option<Range<usize>> {
        let (anchor, end) = self.selection?;
        (anchor != end).then(|| {
            self.placeholder_boundary(anchor.min(end), false)..self.placeholder_boundary(anchor.max(end), true)
        })
    }

    pub(crate) fn selected_text(&self) -> Option<&str> {
        Some(&self.text[self.selection_range()?])
    }

    pub(crate) fn delete_selection(&mut self) -> bool {
        self.replace_selection("")
    }

    pub(crate) fn replace_selection(&mut self, text: impl ToString) -> bool {
        let Some(range) = self.selection_range() else {
            return false;
        };
        let text = text.to_string();
        let range = self.replace_placeholders(range, &text);
        self.cursor = range.start + text.len();
        self.align_cursor();
        self.set_cursor(self.cursor);
        true
    }

    /// Consume selection deletion; insertion still uses the editor's normal path.
    pub(crate) fn selection_key(&mut self, key: KeyEvent) -> bool {
        let modifiers = key.modifiers - KeyModifiers::SHIFT;
        match (key.code, modifiers) {
            (KeyCode::Backspace | KeyCode::Delete, KeyModifiers::NONE)
            | (KeyCode::Char('h' | 'd'), KeyModifiers::CONTROL) => self.delete_selection(),
            (
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Home
                | KeyCode::End,
                KeyModifiers::NONE,
            )
            | (
                KeyCode::Char('a' | 'b' | 'e' | 'f' | 'n' | 'p' | 'k' | 'u' | 'w'),
                KeyModifiers::CONTROL,
            )
            | (KeyCode::Char('b' | 'f' | 'd'), KeyModifiers::ALT) => {
                self.clear_selection();
                false
            }
            _ => false,
        }
    }
}
